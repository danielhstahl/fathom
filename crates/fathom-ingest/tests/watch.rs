//! `watch.rs` — the acceptance criteria of `e1.9` (`reco-949`), tested against real repositories, real
//! filesystem events and real git processes.
//!
//! The ticket's four criteria are each a different kind of difficult, so each gets its own approach rather
//! than one harness pretending to cover them:
//!
//! 1. *a commit made in another terminal appears within 1 s* — a fixture test driving the state machine with
//!    the **real** notification backend, with the poll deliberately parked out of reach so that only a
//!    filesystem event can satisfy it. The strict wall-clock form is `#[ignore]`d and reads its budget from
//!    `benchmarks/budgets.json`, the house rule for time assertions in a debug build.
//! 2. *idle CPU under 1 percent* — measured, not asserted away: the watcher's own busy-share meter over a
//!    quiet repository, compared against the budget file.
//! 3. *rebase, gc and repo replacement handled without a crash or a stuck spinner* — each pathological case
//!    is produced the way git produces it (real lock files, a real `rebase-merge` directory, a real `git init`
//!    over the top of the old `.git`) and the assertions are about the two failure modes: is a notice shown,
//!    and does a run eventually happen.
//! 4. *polling fallback kicks in automatically and is reported in doctor* — a notifier that stays silent is
//!    the only honest way to test this in a container with working inotify, and the probe that `doctor` runs
//!    is tested with a synthetic mount table for the same reason ADR-004 §14.2 gives for the store's NFS
//!    rules.
//!
//! Everything drives [`Watch::step`] directly, so a test is a sequence of states rather than a thread racing
//! a timer.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(any(feature = "watch", feature = "watch-poll"))]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Fixture;
use fathom_ingest::repo::Repository;
use fathom_ingest::watch::Fingerprint;
use fathom_ingest::watch::{
    self, Interest, InterestReason, Job, JobError, Notice, NoticeKind, Phase, Progress, Status,
    Watch,
};
use fathom_ingest::watch::{BackendChoice, BackendInfo, BackendKind, Sign, Source};

// ---------------------------------------------------------------------------------------------
// The parts a test drives the machine with
// ---------------------------------------------------------------------------------------------

/// The scripted stand-in for `fathom-cli`'s index job.
#[derive(Default)]
struct Recorder {
    calls: Vec<Call>,
    failures: usize,
    failure: Option<JobError>,
    /// Whether a finished run says it is complete. `false` is the budget-stopped case.
    complete: bool,
    /// What to report, so a test can assert the numbers reach the progress state.
    commits: u64,
    units: u64,
}

struct Call {
    reason: InterestReason,
    names: Vec<String>,
    head: String,
}

impl Recorder {
    fn count(&self) -> usize {
        self.calls.len()
    }

    fn names_of_last(&self) -> Vec<String> {
        self.calls
            .last()
            .map(|call| call.names.clone())
            .unwrap_or_default()
    }
}

impl Job for Recorder {
    fn refresh(
        &mut self,
        interest: &Interest,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        // Report a chunk first: a run that only speaks at the end cannot be tested for progress.
        progress(Progress {
            commits: self.commits,
            units: self.units,
            chunks: 1,
            complete: false,
        });
        if self.failures > 0 {
            self.failures -= 1;
            return Err(self
                .failure
                .clone()
                .unwrap_or_else(|| JobError::Transient("scripted failure".to_owned())));
        }
        self.calls.push(Call {
            reason: interest.reason,
            names: interest
                .changes
                .iter()
                .map(|change| change.name.clone())
                .collect(),
            head: interest.head.clone(),
        });
        Ok(Progress {
            commits: self.commits,
            units: self.units,
            chunks: 1,
            complete: self.complete,
        })
    }
}

/// A notifier that never says anything: the NFS case, produced on a machine whose inotify works fine.
struct SilentNotifier;

impl Source for SilentNotifier {
    fn wait(&mut self, timeout: Duration) -> Sign {
        std::thread::sleep(timeout);
        Sign::Nothing
    }

    fn info(&self) -> BackendInfo {
        BackendInfo {
            kind: BackendKind::Notification,
            name: "silent-notifier".to_owned(),
            reason: None,
        }
    }
}

/// Options tuned for a test that must finish in milliseconds, with every default kept for the fields a
/// production claim is made about.
fn options() -> watch::Options {
    watch::Options {
        initial_refresh: false,
        debounce: Duration::from_millis(20),
        settle_max: Duration::from_millis(500),
        poll_interval: Duration::from_millis(20),
        poll_interval_max: Duration::from_millis(200),
        tick: Duration::from_millis(5),
        stale_after: Duration::from_millis(800),
        ref_spam_threshold: 16,
        ref_spam_window: Duration::from_millis(400),
        throttle_max: Duration::from_millis(120),
        operation_settle: Duration::from_millis(40),
        operation_min_interval: Duration::from_secs(30),
        cpu_window: Duration::from_millis(200),
        error_backoff: Duration::from_millis(20),
        max_error_attempts: 3,
        ..watch::Options::default()
    }
}

/// A repository with one commit, ready to be watched.
fn repository(fixture: &Fixture) -> PathBuf {
    let root = fixture.init();
    fixture.commit(&root, "first", &[("a.txt", "one\n")]);
    root
}

fn watch(root: &Path) -> Watch {
    Watch::open(root, options()).expect("the fixture is a watchable repository")
}

/// Drive for a wall-clock span, collecting every emitted status.
fn drive(
    watch: &mut Watch,
    job: &mut Recorder,
    statuses: &mut Vec<Status>,
    for_how_long: Duration,
) {
    let until = Instant::now() + for_how_long;
    while Instant::now() < until {
        watch.step(job, &mut |status| statuses.push(status.clone()));
    }
}

/// Drive until a condition holds, and say how long that took. Fails with the last statuses, because a
/// state machine that did not reach a state has to be diagnosed, not just failed.
fn drive_until<F: Fn(&Recorder) -> bool>(
    watch: &mut Watch,
    job: &mut Recorder,
    statuses: &mut Vec<Status>,
    want: F,
    budget: Duration,
) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < budget {
        watch.step(job, &mut |status| statuses.push(status.clone()));
        if want(job) {
            return Some(started.elapsed());
        }
    }
    eprintln!(
        "last statuses:\n{}",
        statuses
            .iter()
            .rev()
            .take(8)
            .map(|status| format!("  {} (notices: {})", status.line(), status.notices.len()))
            .collect::<Vec<_>>()
            .join("\n")
    );
    None
}

/// Drive until the *state* is right, for the tests whose subject is the state machine itself.
fn drive_until_phase<F: Fn(&Watch) -> bool>(
    watch: &mut Watch,
    job: &mut Recorder,
    statuses: &mut Vec<Status>,
    want: F,
    budget: Duration,
) -> bool {
    let until = Instant::now() + budget;
    while Instant::now() < until {
        watch.step(job, &mut |status| statuses.push(status.clone()));
        if want(watch) {
            return true;
        }
    }
    false
}

fn notices(statuses: &[Status]) -> Vec<&Notice> {
    statuses
        .last()
        .map(|status| status.notices.iter().collect())
        .unwrap_or_default()
}

fn has_notice(statuses: &[Status], kind: NoticeKind) -> bool {
    notices(statuses).iter().any(|notice| notice.kind == kind)
}

/// A loose ref written the way git writes one: content-addressed bytes, then a rename into place. Used for
/// the ref-spam case, where the point is sixty refs arriving faster than `git` could be invoked sixty times.
fn write_loose_ref(git_dir: &Path, name: &str, target: &str) {
    let path = git_dir.join("refs/tags").join(name);
    std::fs::create_dir_all(path.parent().expect("refs/tags exists")).expect("refs/tags");
    let temp = path.with_extension("lock");
    std::fs::write(&temp, format!("{target}\n")).expect("write the temp ref");
    std::fs::rename(&temp, &path).expect("rename the ref into place");
}

/// A budget from `benchmarks/budgets.json`, read rather than copied so the test cannot drift from the file
/// CI treats as normative (`small` is the fixture's size class).
fn budget(family: &str, class: &str) -> f64 {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/budgets.json"),
    )
    .expect("benchmarks/budgets.json is in the tree");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("it is JSON");
    parsed["budgets"]
        .as_array()
        .expect("a budget list")
        .iter()
        .find(|entry| {
            entry["family"] == serde_json::json!(family)
                && entry["size_class"] == serde_json::json!(class)
        })
        .unwrap_or_else(|| panic!("{family} ({class}) is not in benchmarks/budgets.json"))["budget"]
        .as_f64()
        .expect("a budget is a number")
}

// ---------------------------------------------------------------------------------------------
// Acceptance criterion 1: a commit made in another terminal is seen, and named
// ---------------------------------------------------------------------------------------------

#[test]
#[cfg(feature = "watch")]
fn a_commit_made_in_another_terminal_reaches_the_state_machine() {
    // The poll is parked 60 s away, so nothing but a real filesystem notification can satisfy this test:
    // it is the notification path under test, not the fallback that would also have caught it.
    let fixture = Fixture::new("watch-notify");
    let root = repository(&fixture);
    let mut options = options();
    options.backend = BackendChoice::Notify;
    options.poll_interval = Duration::from_secs(60);
    options.poll_interval_max = Duration::from_secs(60);
    let mut watch = Watch::open(&root, options).expect("the fixture is watchable");
    assert_eq!(
        watch.backend().kind,
        BackendKind::Notification,
        "this test is about notifications: {:?}",
        watch.backend()
    );
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    // Burn the poll tick that is due immediately on open, so the poll provably cannot be what finds the
    // commit: the next one is a minute away.
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(60),
    );
    assert_eq!(job.count(), 0, "nothing has happened yet");

    let head = fixture.commit(&root, "made in the other terminal", &[("b.txt", "two\n")]);
    let seen = drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(5),
    )
    .expect("the notification backend reported the commit");

    assert!(
        seen < Duration::from_secs(5),
        "a notification should not need the whole budget: {seen:?}"
    );
    assert_eq!(
        job.calls[0].reason,
        InterestReason::Commits,
        "a moved HEAD is commits, not refs"
    );
    let names = job.names_of_last();
    assert!(names.iter().any(|name| name == "HEAD"), "{names:?}");
    assert!(
        names.iter().any(|name| name == "refs/heads/main"),
        "the branch that moved has to be named, not just HEAD: {names:?}"
    );
    let last = statuses.last().expect("at least one status was emitted");
    assert!(last.head.contains(&head[..7]), "{last:?}");
    assert_eq!(last.phase, Phase::Idle, "a finished run goes back to idle");
    assert!(
        watch.counters().notifications > 0,
        "the notification counter is the evidence this was not the poll"
    );
}

#[test]
#[cfg(feature = "watch")]
#[ignore = "wall-clock budget: run under `--release --ignored`, the house rule for time assertions"]
fn a_new_commit_is_named_within_the_detect_budget() {
    // Acceptance criterion one, in its strict form: from the commit to the watcher naming it must fit
    // `watch.detect.ms`, which is where the 1 s promise lives once it is a number in the budget file.
    let budget = budget("watch.detect.ms", "small");
    let fixture = Fixture::new("watch-budget");
    let root = repository(&fixture);
    let mut options = options();
    options.backend = BackendChoice::Notify;
    options.poll_interval = Duration::from_secs(60);
    options.poll_interval_max = Duration::from_secs(60);
    // The production debounce, not the test one: this is measuring the claim, not the plumbing.
    options.debounce = Duration::from_millis(150);
    let mut watch = Watch::open(&root, options).expect("watchable");
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(60),
    );

    fixture.commit(&root, "timed", &[("c.txt", "three\n")]);
    let seen = drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(10),
    )
    .expect("the commit was noticed");
    assert!(
        seen.as_secs_f64() * 1_000.0 < budget,
        "detection took {} ms against a {budget} ms budget",
        seen.as_millis()
    );
}

#[test]
fn a_new_commit_is_named_with_its_old_and_new_target() {
    let fixture = Fixture::new("watch-name");
    let root = repository(&fixture);
    let before = common::git_head(&root);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        commits: 1,
        units: 3,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();

    let after = fixture.commit(&root, "second", &[("a.txt", "one more\n")]);
    drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(3),
    )
    .expect("the commit was noticed by the poll");

    // The change carries both ends, because "main moved" is not something a timeline can use.
    let statuses_with_change = statuses
        .iter()
        .filter_map(|status| status.last_change.as_ref())
        .collect::<Vec<_>>();
    assert!(!statuses_with_change.is_empty(), "no change was ever shown");
    let head_change = statuses_with_change
        .iter()
        .find(|change| change.name == "HEAD")
        .expect("HEAD's move is the one the timeline follows");
    assert_eq!(head_change.from.as_deref(), Some(before.as_str()));
    assert_eq!(head_change.to.as_deref(), Some(after.as_str()));
    assert_eq!(head_change.kind, fathom_ingest::watch::ChangeKind::Moved);

    // And progress was emitted *during* the run, not only after it, and it named the same head.
    assert!(
        statuses
            .iter()
            .any(|status| matches!(status.phase, Phase::Indexing { units: 3, .. })),
        "the indexing state must carry the numbers"
    );
    assert!(
        job.calls[0].head.contains(&after[..7]),
        "the refresh is told what HEAD is now: {:?}",
        job.calls[0].head
    );
}

#[test]
fn a_ref_moving_without_head_is_reported_as_refs_not_commits() {
    let fixture = Fixture::new("watch-ref");
    let root = repository(&fixture);
    let head = common::git_head(&root);
    fixture.branch(&root, "elsewhere");
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();

    // Force a second commit onto the other branch without leaving this one, the way a background
    // `git fetch` or a worktree would: HEAD does not move, a branch does.
    let other = fixture.commit(&root, "on the other branch", &[("x.txt", "x\n")]);
    git(&root, &["reset", "-q", "--hard", &head]);
    git(&root, &["branch", "-f", "elsewhere", &other]);
    drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(3),
    )
    .expect("the moved branch was noticed");
    assert_eq!(job.calls[0].reason, InterestReason::Refs);
    assert!(
        job.names_of_last()
            .contains(&"refs/heads/elsewhere".to_owned())
    );
    assert!(
        !job.names_of_last().contains(&"HEAD".to_owned()),
        "HEAD did not move and must not be reported as having"
    );
}

#[test]
fn a_deleted_branch_is_a_removal_the_timeline_can_show() {
    let fixture = Fixture::new("watch-delete");
    let root = repository(&fixture);
    fixture.branch(&root, "topic");
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();

    git(&root, &["branch", "-D", "topic"]);
    drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(3),
    )
    .expect("the deletion was noticed");
    let shown = statuses
        .iter()
        .filter_map(|status| status.last_change.clone())
        .find(|change| change.name == "refs/heads/topic")
        .expect("the deleted branch is named");
    assert_eq!(shown.kind, fathom_ingest::watch::ChangeKind::Removed);
    assert!(shown.summary().contains("deleted"), "{}", shown.summary());
}

#[test]
fn the_cheap_pass_moves_when_git_writes_and_does_not_when_it_does_not() {
    // The fingerprint is the whole CPU argument, so its two directions both need an assertion: it must not
    // miss a real write, and it must not fire on nothing, or the idle cost claim is fiction.
    let fixture = Fixture::new("watch-fingerprint");
    let root = repository(&fixture);
    let repository = Repository::discover(&root).expect("the fixture is a repository");
    let first = Fingerprint::read(&repository, 10_000);
    let second = Fingerprint::read(&repository, 10_000);
    assert_eq!(
        first, second,
        "two reads of a quiet repository must be equal, or every poll looks like a change"
    );

    fixture.commit(&root, "third", &[("d.txt", "four\n")]);
    let third = Fingerprint::read(&repository, 10_000);
    assert_ne!(second, third, "a commit must move the fold");
    let moved = third.what_moved(&second);
    assert!(
        moved
            .iter()
            .any(|signal| matches!(*signal, "refs" | "HEAD")),
        "the fold has to say which part moved: {moved:?}"
    );
    assert!(
        !third.loose_refs_capped,
        "the walk cap must be reported when it binds"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance criterion 2: idle cost
// ---------------------------------------------------------------------------------------------

#[test]
fn a_quiet_repository_costs_a_fraction_of_a_core() {
    // Acceptance criterion two, measured by the watcher's own meter rather than by a stopwatch around a
    // sleep: the number is the same one `fathom watch` prints and `fathom doctor` can be asked about.
    let budget = budget("watch.idle_cpu.pct", "small");
    let fixture = Fixture::new("watch-idle");
    let root = repository(&fixture);
    let mut options = options();
    options.cpu_window = Duration::from_millis(250);
    options.poll_interval = Duration::from_millis(50);
    options.initial_refresh = true;
    let mut watch = Watch::open(&root, options).expect("watchable");
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(1_500) {
        watch.step(&mut job, &mut |status| statuses.push(status.clone()));
    }
    let status = watch.status();
    assert!(
        status.counters.poll_ticks >= 5,
        "the watcher must actually have polled: {:?}",
        status.counters
    );
    assert_eq!(status.phase, Phase::Idle, "a quiet repo is an idle watcher");
    assert!(
        status.cpu_share * 100.0 < budget,
        "idle watcher used {:.3}% of a core against a {budget}% budget ({} polls over {} ms)",
        status.cpu_share * 100.0,
        status.counters.poll_ticks,
        started.elapsed().as_millis()
    );
    assert_eq!(
        status.counters.index_runs, 1,
        "only the startup run: nothing else happened"
    );
}

#[test]
fn the_budget_stretches_the_poll_when_the_measured_share_exceeds_it() {
    // A budget with no enforcement is a comment. The cheapest way to over-run it in a test is to make the
    // job itself the over-run, then watch the watcher slow itself down.
    let fixture = Fixture::new("watch-budget-stretch");
    let root = repository(&fixture);
    let mut options = options();
    options.cpu_share = 0.001; // one tenth of a percent: any real work blows it
    options.cpu_window = Duration::from_millis(50);
    options.poll_interval_max = Duration::from_millis(160);
    let mut watch = Watch::open(&root, options).expect("watchable");
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    fixture.commit(&root, "work", &[("e.txt", "e\n")]);
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(900),
    );
    let status = watch.status();
    assert!(
        status.counters.stretches >= 1,
        "the poll should have stretched: {:?} at {} ms interval",
        status.counters,
        status.poll_interval.as_millis()
    );
    assert!(
        status.poll_interval > Duration::from_millis(20),
        "the interval in force must be above the floor: {:?}",
        status.poll_interval
    );
    assert!(
        has_notice(&statuses, NoticeKind::Budget),
        "over-running the budget must be said, not done quietly"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance criterion 3: the pathological cases
// ---------------------------------------------------------------------------------------------

#[test]
fn a_rebase_in_progress_spaces_runs_and_never_stalls() {
    // The ticket names "interactive rebase in progress" as a case that must not crash and must not stick.
    // A rebase stopped on a conflict can wait an hour for a human, so the watcher may not simply wait for
    // it to finish; it spaces its runs and then, at the settle ceiling, acts anyway.
    let fixture = Fixture::new("watch-rebase");
    let root = repository(&fixture);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    fixture.commit(&root, "before the rebase", &[("f.txt", "f\n")]);
    drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(3),
    )
    .expect("the first commit after startup is noticed");

    // Now the rebase starts, and keeps making commits.
    let git_dir = root.join(".git");
    let rebase = git_dir.join("rebase-merge");
    std::fs::create_dir_all(&rebase).expect("rebase-merge");
    std::fs::write(rebase.join("interactive"), "").expect("interactive");
    std::fs::write(rebase.join("msgnum"), b"2\n").expect("msgnum");
    std::fs::write(rebase.join("end"), b"5\n").expect("end");
    fixture.commit(&root, "picked 2", &[("g.txt", "g\n")]);

    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(150),
    );
    let before = job.count();
    assert_eq!(
        before, 1,
        "the rebase's commit must have been held back, not run"
    );
    assert!(
        has_notice(&statuses, NoticeKind::Operation),
        "a rebase in progress has to be visible: {:?}",
        statuses.last().map(|status| &status.notices)
    );
    assert!(
        statuses
            .last()
            .map(|status| status
                .notices
                .iter()
                .any(|notice| notice.text.contains("2 of 5")))
            .unwrap_or(false),
        "the notice should say where the rebase is: {:?}",
        statuses.last().map(|status| &status.notices)
    );
    assert!(
        watch.counters().deferrals > 0,
        "the run should have been deferred, not silently skipped"
    );

    // And it must not stay deferred forever: past the settle ceiling the watcher acts, rebase notwithstanding.
    let settled = drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > before,
        Duration::from_secs(3),
    );
    assert!(
        settled.is_some(),
        "the settle ceiling must force a run; a spinner that waits on a human is the failure this \
         test exists for"
    );
    assert!(
        has_notice(&statuses, NoticeKind::Cleared),
        "acting past the ceiling must say so"
    );

    // Finish the rebase: the notice goes away rather than lingering.
    std::fs::remove_dir_all(&rebase).expect("end the rebase");
    fixture.commit(&root, "rebased", &[("h.txt", "h\n")]);
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(300),
    );
    assert!(
        !statuses
            .last()
            .map(|status| status
                .notices
                .iter()
                .any(|n| n.kind == NoticeKind::Operation))
            .unwrap_or(false),
        "the operation notice must clear when the operation does"
    );
}

#[test]
fn a_gc_storm_defers_the_run_and_the_run_happens_when_it_clears() {
    // A gc lock means git is moving objects; reading the store mid-sweep is a wasted run at best. The guard
    // defers — and, unlike a rebase, gc finishes on its own, so the assertion is that the run follows the
    // lock rather than a deadline.
    let fixture = Fixture::new("watch-gc");
    let root = repository(&fixture);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    let git_dir = root.join(".git");
    std::fs::write(git_dir.join("gc.lock"), "").expect("gc.lock");
    std::fs::create_dir_all(git_dir.join("objects/pack")).expect("pack dir");
    std::fs::write(git_dir.join("objects/pack/pack-1234.pack.tmp"), "").expect("tmp pack");

    fixture.commit(&root, "during gc", &[("i.txt", "i\n")]);
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(200),
    );
    assert_eq!(job.count(), 0, "a gc in flight must hold the run back");
    assert!(
        has_notice(&statuses, NoticeKind::Maintenance),
        "and say that it did"
    );
    assert!(
        matches!(watch.phase(), Phase::Detecting { pending } if *pending >= 1),
        "the change is held, visibly: {:?}",
        watch.phase()
    );

    std::fs::remove_file(git_dir.join("gc.lock")).expect("release gc");
    std::fs::remove_file(git_dir.join("objects/pack/pack-1234.pack.tmp")).expect("finish the pack");
    let cleared = drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(3),
    );
    assert!(
        cleared.is_some(),
        "the deferred run must go ahead once gc releases its lock"
    );
}

#[test]
fn a_repository_replaced_under_the_watcher_is_reopened_not_crashed() {
    // `rm -rf .git && git init`, or a container restarted onto another mount: the handle is about a
    // directory that no longer exists, and the only honest recovery is to start again and say so.
    let fixture = Fixture::new("watch-replace");
    let root = repository(&fixture);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    let old_head = common::git_head(&root);

    // Swap the git directory for a different one at the same path.
    std::fs::rename(root.join(".git"), root.join(".git-was")).expect("move the git dir aside");
    git(&root, &["init", "-q", "--initial-branch=trunk", "."]);
    let new_head = fixture.commit(&root, "a different repository", &[("only.txt", "only\n")]);

    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(600),
    );
    let status = watch.status();
    assert_eq!(
        status.counters.reopens, 1,
        "the inode change must be noticed exactly once: {:?}",
        status.counters
    );
    assert!(
        status.counters.errors <= status.counters.reopens,
        "a replacement is a reopen, not an error storm: {:?}",
        status.counters
    );
    assert!(
        status.head.contains("trunk") || status.head.contains(&new_head[..7]),
        "after the reopen the watcher must be reading the new repository: {:?}",
        status.head
    );
    assert!(
        !status.head.contains(&old_head[..7]),
        "and must not still be reporting the old one"
    );
    assert!(
        has_notice(&statuses, NoticeKind::Repository),
        "and it must say the directory changed under it"
    );
    assert!(
        !matches!(status.phase, Phase::Stale { .. } | Phase::Error { .. }),
        "a replacement is handled, not survived: {:?}",
        status.phase
    );
}

#[test]
fn ref_spam_is_coalesced_and_throttled_instead_of_indexed_per_ref() {
    // A background process that moves 60 refs in a burst must cost a handful of index runs, not 60.
    let fixture = Fixture::new("watch-spam");
    let root = repository(&fixture);
    let head = common::git_head(&root);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    let git_dir = root.join(".git");

    for index in 0..60 {
        write_loose_ref(&git_dir, &format!("spam-{index:02}"), &head);
        drive(
            &mut watch,
            &mut job,
            &mut statuses,
            Duration::from_millis(20),
        );
    }
    let counters = watch.counters();
    assert!(
        counters.ref_changes > 16,
        "the spam should have been seen: {counters:?}"
    );
    assert!(counters.throttles >= 1, "and throttled: {counters:?}");
    assert!(
        counters.index_runs < 30,
        "60 refs must not cost 60 runs ({} runs for {} ref moves): {counters:?}",
        counters.index_runs,
        counters.ref_changes
    );
    assert!(
        has_notice(&statuses, NoticeKind::RefSpam),
        "and the watcher must say that something is scripting git"
    );
}

#[test]
fn a_run_stops_for_its_budget_and_is_asked_for_again() {
    // `complete: false` is what `fathom watch` returns when its chunk budget runs out mid-repository.
    // The watcher must not call that finished, and must not spin either.
    let fixture = Fixture::new("watch-budget-run");
    let root = repository(&fixture);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: false,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    fixture.commit(&root, "a lot", &[("j.txt", "j\n")]);
    drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() >= 2,
        Duration::from_secs(3),
    )
    .expect("an incomplete run is asked for again");
    assert!(
        statuses
            .iter()
            .any(|status| status.phase == Phase::Detecting { pending: 0 }),
        "between chunks the state is detecting, not idle"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance criterion 4: the polling fallback, and the probe `doctor` reports
// ---------------------------------------------------------------------------------------------

#[test]
fn a_notifier_that_stays_silent_is_replaced_by_polling_with_the_reason() {
    // The whole point of the fallback is that nothing is misconfigured and nothing is noticed by the user:
    // the notification subscription succeeds, events never arrive, and the watcher works it out.
    let fixture = Fixture::new("watch-fallback");
    let root = repository(&fixture);
    let mut options = options();
    options.backend = BackendChoice::Notify;
    let mut watch = Watch::open(&root, options).expect("watchable");
    watch.use_source(
        Box::new(SilentNotifier),
        BackendInfo {
            kind: BackendKind::Notification,
            name: "silent-notifier".to_owned(),
            reason: None,
        },
    );
    assert_eq!(watch.backend().kind, BackendKind::Notification);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();

    // Two changes, because one could be a race and two is a pattern — which is the actual rule the state
    // machine implements.
    fixture.commit(&root, "one", &[("k.txt", "k\n")]);
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(300),
    );
    fixture.commit(&root, "two", &[("l.txt", "l\n")]);
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(500),
    );

    let status = watch.status();
    assert_eq!(
        status.backend.kind,
        BackendKind::Polling,
        "a notifier that never reports must be replaced"
    );
    assert!(status.counters.backend_switches >= 1);
    let reason = status
        .backend
        .reason
        .clone()
        .expect("the fallback has to carry its reason");
    assert!(
        reason.contains("never reported"),
        "and the reason has to be the real one: {reason}"
    );
    assert!(
        job.count() >= 2,
        "and the changes must still have been indexed: {:?}",
        status.counters
    );
    assert!(
        has_notice(&statuses, NoticeKind::Backend),
        "the switch is a notice too"
    );
}

#[test]
fn the_probe_names_the_backend_a_repository_would_get_and_why() {
    // What `fathom doctor` prints. The filesystem classification is exercised with a synthetic mount table
    // because this container has no NFS — the same reason ADR-004 §14.2 gives for the store's network rules.
    let nfs = "sysfs /sys sysfs rw 0 0\nserver:/share /srv nfs rw 0 0\n/dev/vda1 / ext4 rw 0 0\n";
    assert_eq!(
        fathom_ingest::watch::mount_fstype(nfs, Path::new("/srv/repo/.git")).as_deref(),
        Some("nfs")
    );
    assert!(
        !fathom_ingest::watch::notifications_work("nfs"),
        "and the classification agrees that inotify does not cross it"
    );
    assert_eq!(
        fathom_ingest::watch::mount_fstype(nfs, Path::new("/srv/other")).as_deref(),
        Some("nfs"),
        "a subdirectory of the mount is still the mount"
    );

    let probe =
        fathom_ingest::watch::probe(BackendChoice::Auto, Path::new("/"), Duration::from_secs(1));
    // Whatever this machine is, the probe must be self-consistent: polling implies a stated reason.
    if probe.backend.kind == BackendKind::Polling {
        assert!(
            probe.backend.reason.is_some(),
            "a probe that chose polling has to say why: {:?}",
            probe.backend
        );
    }
    assert_eq!(probe.requested, BackendChoice::Auto);
}

// ---------------------------------------------------------------------------------------------
// The state machine's own behaviour
// ---------------------------------------------------------------------------------------------

#[test]
fn a_failing_run_retries_then_goes_stale_and_then_recovers() {
    // Failure is a state with an exit, not a log line: retries, a bounded number of them, and a way back.
    let fixture = Fixture::new("watch-error");
    let root = repository(&fixture);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        failures: 8,
        failure: Some(JobError::Transient("the store is busy".to_owned())),
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    fixture.commit(&root, "unlucky", &[("m.txt", "m\n")]);

    let stale = drive_until_phase(
        &mut watch,
        &mut job,
        &mut statuses,
        |watch| matches!(watch.phase(), Phase::Stale { .. }),
        Duration::from_secs(3),
    );
    assert!(stale, "the attempts must end in the stale state");
    let counters = watch.counters();
    assert!(
        counters.errors >= 3,
        "attempts should have been made: {counters:?}"
    );
    assert!(
        statuses
            .iter()
            .any(|status| matches!(status.phase, Phase::Error { attempt: 1, .. })),
        "the first retry is attempt one, not a silent repeat"
    );
    assert!(
        matches!(watch.phase(), Phase::Stale { .. }),
        "after the attempts run out the state is stale: {:?}",
        watch.phase()
    );
    assert!(
        has_notice(&statuses, NoticeKind::Stale),
        "and stale is said in words"
    );

    // Recovery: the same outstanding change succeeds once the cause clears, and the state leaves stale.
    job.failures = 0;
    job.failure = None;
    let recovered = drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(3),
    );
    assert!(
        recovered.is_some(),
        "the failed batch must be retried, not dropped on the floor"
    );
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(100),
    );
    assert_eq!(
        watch.phase(),
        &Phase::Idle,
        "and the state must leave stale"
    );
    assert_eq!(watch.counters().consecutive_errors, 0);
}

#[test]
fn a_fatal_run_ends_the_loop_with_its_reason() {
    let fixture = Fixture::new("watch-fatal");
    let root = repository(&fixture);
    let mut options = options();
    options.initial_refresh = true;
    let mut watch = Watch::open(&root, options).expect("watchable");
    let mut job = Recorder {
        complete: true,
        failures: 1,
        failure: Some(JobError::Fatal("the store is corrupt".to_owned())),
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    let reason = watch.run(&mut job, &|| false, &mut |status| {
        statuses.push(status.clone())
    });
    assert_eq!(
        reason,
        fathom_ingest::watch::StopReason::Fatal("the store is corrupt".to_owned())
    );
    assert!(has_notice(&statuses, NoticeKind::Stale));
}

#[test]
fn a_quiet_watcher_stays_idle_and_keeps_reading_the_repository() {
    let fixture = Fixture::new("watch-quiet");
    let root = repository(&fixture);
    let mut watch = watch(&root);
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(500),
    );
    assert_eq!(
        job.count(),
        0,
        "nothing happened, so nothing should have run"
    );
    assert_eq!(watch.phase(), &Phase::Idle);
    assert!(watch.counters().looks >= 5, "but it kept looking");
    assert!(
        statuses.iter().all(|status| status.phase == Phase::Idle),
        "{:?}",
        watch.counters()
    );
}

#[test]
fn a_quiet_watcher_keeps_emitting_so_a_reader_can_tell_alive_from_dead() {
    // The status is a heartbeat as well as a report: `.fathom/watch/status.json` and the ambient view of
    // `reco-8bx` are read by *other* processes, and a file that stopped changing when the last index run
    // finished cannot be told from one left behind by a `kill -9`. So the machine says "still here, still
    // idle" on a beat, even when it has nothing else to say.
    let fixture = Fixture::new("watch-beat");
    let root = repository(&fixture);
    let mut options = options();
    options.heartbeat = Duration::from_millis(20);
    options.poll_interval = Duration::from_secs(30); // the beat is not the poll wearing a hat
    let mut watch = Watch::open(&root, options).expect("watchable");
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    drive(
        &mut watch,
        &mut job,
        &mut statuses,
        Duration::from_millis(300),
    );
    assert!(
        statuses.len() >= 5,
        "a quiet repository produced {} statuses in 300 ms; a caller that only hears on change cannot \
         tick its own clocks, and a heartbeat that only moves on change is indistinguishable from a dead \
         watcher",
        statuses.len()
    );
    assert!(
        statuses.iter().all(|status| status.phase == Phase::Idle),
        "the beat must not invent a state: {:?}",
        statuses
            .iter()
            .map(|status| status.phase.summary())
            .collect::<Vec<_>>()
    );
    assert_eq!(job.count(), 0, "and it must not invent work either");
}

#[test]
fn the_status_says_what_is_happening_in_words_a_terminal_can_print() {
    let fixture = Fixture::new("watch-words");
    let root = repository(&fixture);
    let mut options = options();
    options.initial_refresh = true;
    let mut watch = Watch::open(&root, options).expect("watchable");
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    watch.step(&mut job, &mut |status| statuses.push(status.clone()));
    let status = watch.status();
    let line = status.line();
    assert!(line.contains("idle"), "{line}");
    assert!(line.contains('('), "{line}");
    // Every phase has a line, including the ones a user only sees when something has gone wrong.
    for phase in [
        Phase::Idle,
        Phase::Detecting { pending: 2 },
        Phase::Indexing {
            commits: 4,
            units: 9,
            chunks: 2,
        },
        Phase::Error {
            reason: "no store".to_owned(),
            attempt: 2,
        },
        Phase::Stale {
            reason: "unreadable".to_owned(),
        },
    ] {
        let text = phase.summary();
        assert!(text.starts_with(phase.as_str()), "{text}");
        assert!(
            text.len() >= phase.as_str().len(),
            "{text} is not a sentence"
        );
        if !matches!(phase, Phase::Idle) {
            assert!(
                text.len() > phase.as_str().len(),
                "only idle may be one word: {text}"
            );
        }
    }
}

#[test]
fn watching_a_link_worktree_follows_its_shared_ref_store() {
    // A linked worktree keeps its refs in the main repository's directory, so a commit in the main
    // checkout has to be noticed from the worktree — the case `common_dir` exists for.
    let fixture = Fixture::new("watch-worktree");
    let root = repository(&fixture);
    let other = fixture.path("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            other.to_str().unwrap(),
        ],
    );
    let mut watch = Watch::open(&other, options()).expect("the linked worktree is watchable");
    assert_eq!(
        watch.repository().common_dir(),
        git_dir(&root).as_path(),
        "the worktree's refs live in the main repository: {:?}",
        watch.repository().common_dir()
    );
    let mut job = Recorder {
        complete: true,
        ..Recorder::default()
    };
    let mut statuses = Vec::new();
    fixture.commit(
        &root,
        "on main, from the main checkout",
        &[("n.txt", "n\n")],
    );
    drive_until(
        &mut watch,
        &mut job,
        &mut statuses,
        |job| job.count() > 0,
        Duration::from_secs(3),
    )
    .expect("a ref move in the common dir is seen from a linked worktree");
    assert!(job.names_of_last().contains(&"refs/heads/main".to_owned()));
}

fn git_dir(root: &Path) -> PathBuf {
    let path = root.join(".git");
    if path.is_dir() {
        return path;
    }
    // A linked worktree's `.git` is a file naming the real directory.
    let text = std::fs::read_to_string(&path).expect("the gitfile names a git dir");
    let named = text
        .trim()
        .strip_prefix("gitdir: ")
        .expect("gitfile format")
        .to_owned();
    let named = PathBuf::from(named);
    if named.is_absolute() {
        named
    } else {
        root.join(named)
    }
}

/// `git` with the fixture's environment, for the verbs the helper set does not wrap.
fn git(cwd: &Path, args: &[&str]) -> String {
    common::git(cwd, args)
}
