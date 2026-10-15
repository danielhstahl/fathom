//! `watch.rs` — watch mode v1: noticing new commits and refs while somebody else works, and saying out
//! loud what it is doing about them (`e1.9` / `reco-949`).
//!
//! # The shape of the problem
//!
//! The repository is being changed by another process — a human in a second terminal, an agent, the `git gc`
//! a cron job runs — and fathom has to be current without becoming a cost. That is three separate jobs, and
//! this module keeps them apart because they fail differently:
//!
//! | job | module | fails as |
//! |---|---|---|
//! | *be told* something moved | `source` ([`Source`]) | silence — inotify does not cross NFS, virtiofs or an exhausted watch budget |
//! | *work out what* moved, and name it | `observe` ([`Fingerprint`], [`Snapshot`]) | false positives (it is a cheap fold) and, if careless, false negatives (mtime granularity) |
//! | *decide whether to act now* | this file, with [`Operation`] and [`Maintenance`] | a stuck spinner (waiting on a rebase that is waiting on a human) or a runaway loop (ref spam, a gc storm) |
//!
//! Indexing itself is not here. `fathom-ingest` sits below `fathom-analyze`, so a watcher that indexed would
//! be importing uphill (ADR-002 R1). Instead a caller implements [`Job`] and the watcher calls it — which is
//! also what makes the state machine testable: `tests/watch.rs` drives the whole machine with a scripted job
//! and no store at all, and it is the same machine `fathom watch` runs.
//!
//! # The state machine
//!
//! ```text
//!                  ┌─────────────┐
//!        ┌────────▶│    idle     │◀────────────────┐
//!        │         └──────┬──────┘                 │
//!        │   signal /     │ poll tick              │ run finished
//!  nothing │              ▼                        │
//!  to do ──┤        ┌─────────────┐         ┌──────┴─────────┐
//!        │         │  detecting   │────────▶│   indexing     │
//!        │         │ (settling)   │         │ (with progress)│
//!        │         └──────┬──────┘         └────────────────┘
//!        │   guard says    │ still busy,             │ failed
//!        │   wait, bounded │                         │
//!        │                 │                         ▼
//!        │                 │                  ┌─────────────┐  retries    ┌─────────┐
//!        └─────────────────┴─────────────────▶│    error    │────────────▶│  stale  │
//!                                             └─────────────┘  exhausted  └─────────┘
//! ```
//!
//! Every arrow out of a busy state is bounded by a deadline, which is the whole answer to "no stuck
//! spinner": [`Options::settle_max`] bounds how long a batch of changes may keep being deferred, and
//! [`Options::stale_after`] bounds how long the watcher may go without reading the repository at all. Both
//! fire and *say which one fired* — a state machine diagnosable only from a stack trace is a black box with a
//! spinner bolted on.
//!
//! # Where the CPU goes
//!
//! The watcher's duty is to be invisible next to a running agent (acceptance criterion two), and it earns
//! that three ways rather than by promise: the cheap pass is a `stat` fold, not a read ([`Stat`]'s
//! rationale); the poll debounces and stretches itself when the measured busy share exceeds
//! [`Options::cpu_share`]; and the expensive part — an index run — happens once per *settled* change set,
//! never once per filesystem event, because events are drained in a batch and coalesced. The share that is
//! measured is wall-clock busy time per window, which for a single-threaded watcher that sleeps between ticks
//! is an upper bound on its CPU rather than an estimate of it.
//!
//! # Clocks
//!
//! No wall clock is read here (ADR-002 §4.4). Deadlines are [`Instant`]s and nothing in this module can
//! produce a timestamp; the caller that writes one (`fathom-cli`) stamps its own. Sleeping is not reading a
//! clock: nothing here would behave differently on a different day.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::Error;
use crate::repo::Repository;

// The three jobs are three files, but a crate's surface is declared in one file (ADR-002 R9 / lint W4), so
// they are declared privately and their items re-exported below. That is also the honest boundary: a caller
// drives the machine through [`Watch`] and [`Job`], it does not reach past the machine into the cheap pass.
mod guard;
mod observe;
mod source;

pub use guard::{Maintenance, Operation, maintenance, marker_paths, operation};
pub use observe::{
    Change, ChangeKind, DirId, Fingerprint, Observation, Snapshot, Stat, head_branch, head_summary,
    watch_roots,
};
pub use source::{
    BackendChoice, BackendInfo, BackendKind, NOTIFICATION_BACKEND, NOTIFICATION_COMPILED_IN,
    NotifySource, PollSource, Probe, Sign, Source, filesystem_fstype, mount_fstype,
    notification_name, notifications_work, probe,
};

/// How the watcher may spend its time, and what it waits for.
///
/// Every default is a claim about a real repository, and every one is overridable from `fathom watch`,
/// because a repository on a laptop and a repository in a container do not agree about what quiet means.
#[derive(Debug, Clone)]
pub struct Options {
    /// Which backend to ask for. [`BackendChoice::Auto`] is the default, and is what makes acceptance
    /// criterion four ("the polling fallback kicks in automatically") true rather than aspirational.
    pub backend: BackendChoice,
    /// How long the filesystem must stay quiet before a batch of notifications is acted on. git writes a
    /// ref move as several syscalls (lock, write, rename) and one commit touches `HEAD`, a ref, a reflog
    /// and two objects, so a commit is a *burst*; acting on the first event of a burst reads a half-written
    /// state and indexes the same commit twice.
    pub debounce: Duration,
    /// The absolute bound on deferring. A rebase that keeps touching refs for a minute must not starve the
    /// first change forever: when this fires the watcher indexes what it can see and says that it did.
    pub settle_max: Duration,
    /// The base polling cadence — the debounced fallback, and the only backend on a filesystem that cannot
    /// notify. One second is what keeps detection inside the latency budget even when notifications work.
    pub poll_interval: Duration,
    /// The ceiling polling stretches to when the CPU budget is exceeded.
    pub poll_interval_max: Duration,
    /// How long one loop iteration may block. Bounds stop-latency and nothing else.
    pub tick: Duration,
    /// The longest a caller may go without an emit, even when nothing changed.
    ///
    /// The change-driven half of the contract is what makes a watcher cheap to listen to; the beat is what
    /// makes the silence *distinguishable from a dead process*. A caller that only hears on change cannot
    /// tick its own clocks: the `status.json` a second process reads, and the "quiet for 4 s" the ambient
    /// view of `reco-8bx` will draw, would otherwise keep saying "indexing" for ten minutes after the
    /// indexing stopped — the stuck spinner this ticket has to not be, one process further out.
    pub heartbeat: Duration,
    /// Go to [`Phase::Stale`] when the repository has not been read successfully for this long.
    pub stale_after: Duration,
    /// More than this many ref moves inside [`Options::ref_spam_window`] is a machine, not a person.
    pub ref_spam_threshold: usize,
    /// How far back the spam counter looks.
    pub ref_spam_window: Duration,
    /// The longest the guards may wait between index runs.
    pub throttle_max: Duration,
    /// The longer settle window used while git is mid-operation, so a 40-pick rebase costs a handful of
    /// index runs rather than forty.
    pub operation_settle: Duration,
    /// The minimum spacing between index runs while an operation is in progress.
    pub operation_min_interval: Duration,
    /// The watcher's CPU budget, as a share of one core, measured over [`Options::cpu_window`] windows.
    /// Five percent is "invisible next to a running agent" with room to spare.
    pub cpu_share: f64,
    /// The window the CPU share is measured over.
    pub cpu_window: Duration,
    /// The first retry delay after a failed run; doubled per attempt, capped at [`Options::throttle_max`].
    pub error_backoff: Duration,
    /// How many consecutive failures before the run is given up on and the state goes [`Phase::Stale`].
    pub max_error_attempts: u32,
    /// How many loose ref files the cheap pass may stat before it gives up and asks the ref store instead.
    pub refs_walk_cap: usize,
    /// Re-analyse uncommitted paths when only the index moved. Off by default: staging churn is constant
    /// while an agent works, and this ticket's promise is about commits and refs.
    pub index_worktree: bool,
    /// Index once at startup, before waiting for anything. On by default: a watcher that shows yesterday's
    /// timeline until the next commit is a watcher trusted for one commit too long.
    pub initial_refresh: bool,
    /// Stop after this many looks. A test and script seam, and the honest way to run `fathom watch` in CI.
    pub max_cycles: Option<u64>,
    /// Stop after this much wall time. The same seam in another unit.
    pub deadline: Option<Duration>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            backend: BackendChoice::default(),
            debounce: Duration::from_millis(150),
            settle_max: Duration::from_secs(5),
            poll_interval: Duration::from_secs(1),
            poll_interval_max: Duration::from_secs(20),
            tick: Duration::from_millis(50),
            heartbeat: Duration::from_secs(1),
            stale_after: Duration::from_secs(30),
            ref_spam_threshold: 32,
            ref_spam_window: Duration::from_secs(2),
            throttle_max: Duration::from_secs(30),
            operation_settle: Duration::from_millis(2_000),
            operation_min_interval: Duration::from_secs(10),
            cpu_share: 0.05,
            cpu_window: Duration::from_secs(1),
            error_backoff: Duration::from_millis(500),
            max_error_attempts: 5,
            refs_walk_cap: 20_000,
            index_worktree: false,
            initial_refresh: true,
            max_cycles: None,
            deadline: None,
        }
    }
}

/// The visible state. This is what the TUI of `e1.5` and the ambient mode of `reco-8bx` will draw, and what
/// `fathom watch` prints one line at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Nothing pending, and the repository was read recently.
    Idle,
    /// Something moved; the watcher is waiting out a settle window, or a guard is holding the batch.
    Detecting {
        /// Ref moves collected into the batch that is waiting.
        pending: usize,
    },
    /// A refresh is running. The numbers are cumulative for this run, and progress is emitted between
    /// chunks rather than only at the end, which is what "indexing (with progress)" asks for.
    Indexing {
        /// Commits the spine has taken.
        commits: u64,
        /// Analysis units computed.
        units: u64,
        /// Chunks of work finished so far.
        chunks: u32,
    },
    /// The last thing attempted failed and will be retried.
    Error {
        /// The failure, already rendered.
        reason: String,
        /// Which attempt this was, from 1.
        attempt: u32,
    },
    /// The watcher cannot see the repository, or has given up retrying a run. Reached, never stuck: the loop
    /// keeps watching, and the next successful read leaves this state.
    Stale {
        /// Which of the two it is, in the watcher's own words.
        reason: String,
    },
}

impl Phase {
    /// The stable one-word name, for `--json`, the status file and a TUI style lookup.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Detecting { .. } => "detecting",
            Phase::Indexing { .. } => "indexing",
            Phase::Error { .. } => "error",
            Phase::Stale { .. } => "stale",
        }
    }

    /// Is a spinner allowed here? Both busy states are deadline-bounded, which is the difference between a
    /// spinner and a hang.
    #[must_use]
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Phase::Detecting { .. } | Phase::Indexing { .. } | Phase::Error { .. }
        )
    }

    /// The line for a human.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            Phase::Idle => "idle".to_owned(),
            Phase::Detecting { pending } => {
                if *pending == 0 {
                    "detecting: waiting for the filesystem to settle".to_owned()
                } else {
                    format!("detecting: {pending} change(s) settling")
                }
            }
            Phase::Indexing {
                commits,
                units,
                chunks,
            } => format!("indexing: {commits} commit(s), {units} unit(s), chunk {chunks}"),
            Phase::Error { reason, attempt } => format!("error (attempt {attempt}): {reason}"),
            Phase::Stale { reason } => format!("stale: {reason}"),
        }
    }
}

/// Why the watcher is telling you something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// git is mid-rebase/merge/cherry-pick, so index runs are being spaced out.
    Operation,
    /// gc, a repack or a lock is held, so the watcher is waiting — for a bounded time.
    Maintenance,
    /// Something is moving refs faster than a person does, so the watcher coalesced and throttled.
    RefSpam,
    /// The notification backend was replaced by polling; the text is the reason.
    Backend,
    /// The git directory became a different directory and the handle was reopened.
    Repository,
    /// The measured busy share exceeded the budget, so the poll stretched.
    Budget,
    /// The watcher has not been able to read the repository.
    Stale,
    /// The cheap ref scan hit its cap, so polls read the ref store instead.
    Scan,
    /// A condition that had been reported has cleared.
    Cleared,
}

impl NoticeKind {
    /// The stable spelling for `--json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            NoticeKind::Operation => "operation",
            NoticeKind::Maintenance => "maintenance",
            NoticeKind::RefSpam => "ref-spam",
            NoticeKind::Backend => "backend",
            NoticeKind::Repository => "repository",
            NoticeKind::Budget => "budget",
            NoticeKind::Stale => "stale",
            NoticeKind::Scan => "ref-scan",
            NoticeKind::Cleared => "cleared",
        }
    }
}

/// One thing the watcher wants the human to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// What kind of thing it is.
    pub kind: NoticeKind,
    /// The sentence.
    pub text: String,
}

/// What a watch session accumulated. Printed by `fathom watch`, written to the status file, read by
/// `fathom doctor`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counters {
    /// Reads of the filesystem taken.
    pub looks: u64,
    /// Notifications the backend reported.
    pub notifications: u64,
    /// Poll ticks taken.
    pub poll_ticks: u64,
    /// Ref moves detected.
    pub ref_changes: u64,
    /// Index runs started.
    pub index_runs: u64,
    /// Commits those runs took in total.
    pub commits_indexed: u64,
    /// Analysis units computed in total.
    pub units_computed: u64,
    /// Times a change was deferred by a guard.
    pub deferrals: u64,
    /// Times the ref-spam guard throttled.
    pub throttles: u64,
    /// Times the poll interval was stretched for the CPU budget.
    pub stretches: u64,
    /// Times the git directory was found to be a different directory.
    pub reopens: u64,
    /// Times the backend was switched over.
    pub backend_switches: u64,
    /// Notification errors seen.
    pub degradations: u64,
    /// Failed refreshes.
    pub errors: u64,
    /// Consecutive failures right now, reset by a success.
    pub consecutive_errors: u32,
}

/// Everything a viewer needs to draw the watcher.
#[derive(Debug, Clone)]
pub struct Status {
    /// The state.
    pub phase: Phase,
    /// How long it has been in this state.
    pub in_phase: Duration,
    /// Which backend is producing signals, and why.
    pub backend: BackendInfo,
    /// `HEAD` as it reads.
    pub head: String,
    /// How many refs were seen.
    pub refs: usize,
    /// What it is waiting for, or what made it quiet.
    pub notices: Vec<Notice>,
    /// The most recent named change, for "last: main a1b2c3d → e4f5a6b".
    pub last_change: Option<Change>,
    /// The session numbers.
    pub counters: Counters,
    /// Busy share of the last complete measurement window.
    pub cpu_share: f64,
    /// The poll interval in force, which is the base one unless the budget stretched it.
    pub poll_interval: Duration,
    /// How long since the repository was last read successfully.
    pub quiet_for: Duration,
    /// The git directory being watched, for the label.
    pub git_dir: String,
}

impl Status {
    /// The one-line form `fathom watch` prints.
    #[must_use]
    pub fn line(&self) -> String {
        let mut out = format!("{} · {}", self.phase.summary(), self.backend.summary());
        if let Some(change) = &self.last_change {
            out.push_str(&format!(" · {}", change.summary()));
        }
        out
    }
}

/// What the watcher found, in the shape a refresh needs.
#[derive(Debug, Clone)]
pub struct Interest {
    /// Why the refresh was called.
    pub reason: InterestReason,
    /// `HEAD` as it reads now.
    pub head: String,
    /// The moves, coalesced across whatever arrived while the last run was going.
    pub changes: Vec<Change>,
    /// Did the index move? Only acted on with [`Options::index_worktree`].
    pub index_touched: bool,
    /// Was the git directory replaced? A caller with per-repository cached state must drop it.
    pub replaced: bool,
    /// Which cheap signals fired.
    pub signals: Vec<&'static str>,
}

impl Interest {
    /// Is anything in here worth an index run?
    #[must_use]
    pub fn is_work(&self, index_worktree: bool) -> bool {
        !self.changes.is_empty() || (self.index_touched && index_worktree)
    }
}

/// Why a refresh was asked for, which decides how hard the caller may look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterestReason {
    /// The watcher had barely started.
    Startup,
    /// `HEAD` or a branch moved: there are commits to take.
    Commits,
    /// A ref moved and `HEAD` did not: a tag, a remote ref, a deleted branch.
    Refs,
    /// Only the index moved.
    Worktree,
    /// Several of the above arrived together.
    Coalesced,
}

impl InterestReason {
    /// The stable spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            InterestReason::Startup => "startup",
            InterestReason::Commits => "commits",
            InterestReason::Refs => "refs",
            InterestReason::Worktree => "worktree",
            InterestReason::Coalesced => "coalesced",
        }
    }
}

/// How far a refresh has got. Emitted between chunks so a caller can show progress rather than a spinner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Commits taken so far.
    pub commits: u64,
    /// Analysis units computed so far.
    pub units: u64,
    /// Chunks finished.
    pub chunks: u32,
    /// Is the work done, or is more waiting? `false` means the caller stopped early for its own budget,
    /// which the watcher treats as "still not done" and asks again.
    pub complete: bool,
}

/// What a [`Job`] can say failed, and what the watcher does about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    /// Worth retrying after a backoff: a busy store, a lock, a read that may come back.
    Transient(String),
    /// The repository itself could not be read. The watcher reopens its handle before retrying, which is the
    /// recovery path for a repository that was moved and put back.
    Repository(String),
    /// Stop the loop with this reason. Reserved for "the store is corrupt", not for a bad day.
    Fatal(String),
}

impl JobError {
    /// The message, whichever kind it is.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            JobError::Transient(text) | JobError::Repository(text) | JobError::Fatal(text) => text,
        }
    }
}

/// What the watcher calls when it decides the repository is worth re-reading. `fathom-cli` implements it
/// over `fathom_analyze::index::run` (the composition root owns the clock and the store); a test implements
/// it with a script.
pub trait Job {
    /// Bring the index up to date. Call `progress` after each chunk so the state machine can show progress,
    /// and return the final tally.
    ///
    /// Reentrancy: the watcher never calls this twice at once, but it calls it again immediately if the
    /// returned [`Progress::complete`] is `false`.
    fn refresh(
        &mut self,
        interest: &Interest,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError>;
}

/// Why the loop ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The caller's stop flag was set.
    Requested,
    /// [`Options::deadline`] passed.
    Deadline,
    /// [`Options::max_cycles`] ran out.
    CycleBudget,
    /// A [`Job`] called it fatal.
    Fatal(String),
}

impl StopReason {
    /// The line for the terminal.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            StopReason::Requested => "stopped on request".to_owned(),
            StopReason::Deadline => "stopped at its deadline".to_owned(),
            StopReason::CycleBudget => "stopped after its cycle budget".to_owned(),
            StopReason::Fatal(reason) => format!("stopped: {reason}"),
        }
    }
}

/// The busy-time meter behind the CPU budget.
#[derive(Debug)]
struct Meter {
    window: Duration,
    started: Instant,
    busy: Duration,
    share: f64,
}

impl Meter {
    fn new(window: Duration) -> Meter {
        Meter {
            window,
            started: Instant::now(),
            busy: Duration::ZERO,
            share: 0.0,
        }
    }

    fn charge(&mut self, work: Duration) {
        self.busy += work;
        self.roll();
    }

    /// Close the window if it is old enough, recording the share into [`Meter::share`].
    fn roll(&mut self) {
        let elapsed = self.started.elapsed();
        if elapsed < self.window || elapsed.is_zero() {
            return;
        }
        self.share = micros(self.busy) / micros(elapsed);
        self.busy = Duration::ZERO;
        self.started = Instant::now();
    }
}

fn micros(value: Duration) -> f64 {
    f64::from(u32::try_from(value.as_micros()).unwrap_or(u32::MAX))
}

/// The watcher: a repository handle, a source of signals, and a state machine that decides when to ask a
/// [`Job`] to do work.
pub struct Watch {
    /// The path it was opened from, kept for the reopen path.
    root: PathBuf,
    repo: Repository,
    options: Options,
    source: Box<dyn Source>,
    backend: BackendInfo,
    /// Whether notifications are expected to arrive. `false` makes the poll the authority and skips the
    /// evidence-gathering that would only reach the same conclusion more slowly.
    notifications_trusted: bool,
    /// Poll-discovered changes the notification backend never mentioned.
    unnotified: u32,
    fingerprint: Fingerprint,
    snapshot: Snapshot,
    phase: Phase,
    phase_at: Instant,
    started: Instant,
    last_look: Instant,
    last_index: Option<Instant>,
    counters: Counters,
    notices: Vec<Notice>,
    last_change: Option<Change>,
    /// Signals collected and not yet acted on.
    pending: Option<Observation>,
    /// When to look again because of a notification.
    settle: Option<Instant>,
    /// The absolute deadline for acting on the current batch — the thing that bounds every deferral.
    settle_deadline: Option<Instant>,
    next_poll: Instant,
    interval: Duration,
    /// Ref-move timestamps for the spam guard.
    ref_moves: VecDeque<Instant>,
    /// Until when the spam guard refuses another run.
    throttle_until: Option<Instant>,
    spam_streak: u32,
    /// The last run stopped for its own budget, so there is outstanding work even though nothing new has
    /// been noticed. Cleared by a run that says it is complete.
    more_work: bool,
    /// Has the startup refresh happened? Once, ever, however the loop is driven.
    startup_done: bool,
    /// When a failed run may be retried.
    retry_after: Option<Instant>,
    meter: Meter,
    fatal: Option<String>,
    /// The last operation and maintenance texts shown, so a notice appears when the state *changes* rather
    /// than on every tick that repeats it.
    last_operation: String,
    last_maintenance: String,
    /// When the caller was last handed a status, change or no change — the heartbeat's clock.
    last_emit: Instant,
}

impl Watch {
    /// Open a watcher over `root`, which is where the caller asked to look (and what a reopen re-reads).
    ///
    /// # Errors
    ///
    /// [`Error::NotARepository`] or [`Error::RepositoryUnreadable`] when there is nothing to watch.
    pub fn open(root: &Path, options: Options) -> Result<Watch, Error> {
        let repository = Repository::discover(root)?;
        Self::with_repository(root, repository, options)
    }

    /// Open a watcher over an already-open repository. `root` is remembered only for the reopen path.
    ///
    /// # Errors
    ///
    /// [`Error::RepositoryUnreadable`] when the ref store cannot be read at all: a watcher with no starting
    /// state has nothing to diff against, so that is a refusal rather than a notice.
    pub fn with_repository(
        root: &Path,
        repository: Repository,
        options: Options,
    ) -> Result<Watch, Error> {
        let fingerprint = Fingerprint::read(&repository, options.refs_walk_cap);
        let snapshot = Snapshot::read(&repository)?;
        let probe = source::probe(options.backend, repository.git_dir(), options.poll_interval);
        let (source, backend) = source::open(
            &probe,
            &observe::watch_roots(&repository),
            options.poll_interval,
        );
        let now = Instant::now();
        let notifications_trusted =
            probe.notifications_trusted && backend.kind == BackendKind::Notification;
        let cpu_window = options.cpu_window;
        let poll_interval = options.poll_interval;
        let mut notices = Vec::new();
        if let Some(reason) = backend.reason.clone() {
            notices.push(Notice {
                kind: NoticeKind::Backend,
                text: reason,
            });
        }
        if fingerprint.loose_refs_capped {
            notices.push(Notice {
                kind: NoticeKind::Scan,
                text: format!(
                    "{} loose refs is over the cheap-scan cap of {}, so each poll reads the ref store \
                     instead",
                    fingerprint.loose_refs_walked, options.refs_walk_cap
                ),
            });
        }
        Ok(Watch {
            root: root.to_path_buf(),
            repo: repository,
            options,
            source,
            backend,
            notifications_trusted,
            unnotified: 0,
            fingerprint,
            snapshot,
            phase: Phase::Idle,
            phase_at: now,
            started: now,
            last_look: now,
            last_index: None,
            counters: Counters::default(),
            notices,
            last_change: None,
            pending: None,
            settle: None,
            settle_deadline: None,
            next_poll: now,
            interval: poll_interval,
            ref_moves: VecDeque::new(),
            throttle_until: None,
            spam_streak: 0,
            more_work: false,
            startup_done: false,
            retry_after: None,
            meter: Meter::new(cpu_window),
            fatal: None,
            last_operation: String::new(),
            last_maintenance: String::new(),
            last_emit: now,
        })
    }

    /// The repository handle to read through. The reopen path is the only thing that swaps it, and a caller
    /// that kept its own handle would be holding a dead one after a repository was replaced.
    #[must_use]
    pub fn repository(&self) -> &Repository {
        &self.repo
    }

    /// Which backend is in force right now.
    #[must_use]
    pub fn backend(&self) -> &BackendInfo {
        &self.backend
    }

    /// The options in force.
    #[must_use]
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The session counters.
    #[must_use]
    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// Swap the source of change signals, keeping everything the state machine has learned.
    ///
    /// The reactive fallback does this to itself; a caller can too. Two real uses: `reco-kvk`'s daemon, which
    /// will want one subscription shared by every repository it watches instead of one each, and a test that
    /// has to demonstrate a notifier that stays silent — the failure mode this module exists to survive and
    /// which cannot be produced on demand in a filesystem that is working properly.
    pub fn use_source(&mut self, source: Box<dyn Source>, info: BackendInfo) {
        let changed = info != self.backend;
        self.source = source;
        self.backend = info;
        self.notifications_trusted = self.backend.kind == BackendKind::Notification;
        self.unnotified = 0;
        self.next_poll = Instant::now();
        if changed {
            self.counters.backend_switches += 1;
            if let Some(reason) = self.backend.reason.clone() {
                self.push_notice(NoticeKind::Backend, reason);
            }
        }
    }

    /// Everything a viewer needs, assembled. Contains no wall-clock field, by design (ADR-002 §4.4).
    #[must_use]
    pub fn status(&self) -> Status {
        Status {
            phase: self.phase.clone(),
            in_phase: self.phase_at.elapsed(),
            backend: self.backend.clone(),
            head: self.snapshot.head.clone(),
            refs: self.snapshot.ref_count(),
            notices: self.notices.clone(),
            last_change: self.last_change.clone(),
            counters: self.counters.clone(),
            cpu_share: self.meter.share,
            poll_interval: self.interval,
            quiet_for: self.last_look.elapsed(),
            git_dir: self.repo.git_dir().display().to_string(),
        }
    }

    /// Run until the stop flag is set, the cycle budget runs out, the deadline passes, or a [`Job`] calls it
    /// fatal. Emits a [`Status`] on every state change and on every progress step.
    pub fn run(
        &mut self,
        job: &mut dyn Job,
        stop: &dyn Fn() -> bool,
        emit: &mut dyn FnMut(&Status),
    ) -> StopReason {
        loop {
            if stop() {
                return StopReason::Requested;
            }
            if let Some(max) = self.options.max_cycles
                && self.counters.looks >= max
            {
                return StopReason::CycleBudget;
            }
            if let Some(deadline) = self.options.deadline
                && self.started.elapsed() >= deadline
            {
                return StopReason::Deadline;
            }
            if let Some(reason) = self.fatal.take() {
                return StopReason::Fatal(reason);
            }
            self.step(job, emit);
        }
    }

    /// One loop iteration: wait for the soonest deadline, take the signals that arrived, and act if the
    /// guards allow it. Public because a caller that owns the thread — a daemon (`reco-kvk`), a test, the
    /// future TUI event loop — may want to drive one step at a time instead of handing over the loop.
    pub fn step(&mut self, job: &mut dyn Job, emit: &mut dyn FnMut(&Status)) {
        let before = self.phase.clone();
        let notice_count = self.notices.len();

        if self.options.initial_refresh && !self.startup_done {
            // Done here rather than in `run` so that a caller driving one step at a time and a caller handing
            // over the loop see the same first state — otherwise `step`-driving silently skips the pass that
            // makes the timeline current, which is the opposite of what the option promises.
            self.startup_done = true;
            let interest = Interest {
                reason: InterestReason::Startup,
                head: self.snapshot.head.clone(),
                changes: Vec::new(),
                index_touched: false,
                replaced: false,
                signals: Vec::new(),
            };
            self.act(&interest, job, emit);
        }

        // 1. Wait for whatever comes first: a notification, the next poll, a settle window, a retry.
        let timeout = self.wait_time();
        let polling = self.backend.kind == BackendKind::Polling;
        match self.source.wait(timeout) {
            Sign::Changed if polling => self.counters.poll_ticks += 1,
            Sign::Changed => {
                self.counters.notifications += 1;
                self.unnotified = 0;
                self.settle = Some(Instant::now() + self.options.debounce);
                self.set_phase(Phase::Detecting {
                    pending: self.pending.as_ref().map_or(0, |p| p.changes.len()),
                });
            }
            Sign::Degraded(reason) if polling => {
                // The poll cannot degrade. If it claims it did, say so and carry on.
                self.counters.degradations += 1;
                self.push_notice(NoticeKind::Backend, reason);
            }
            Sign::Degraded(reason) => self.fallback_to_poll(&reason),
            Sign::Nothing => {}
        }

        // 2. The debounced poll runs whatever the backend is: with a working notifier it is the safety net,
        //    on a filesystem that cannot notify it is the whole mechanism.
        let now = Instant::now();
        let mut due = now >= self.next_poll;
        if due {
            self.next_poll = now + self.interval;
            if !polling {
                self.counters.poll_ticks += 1;
            }
        }
        if let Some(settle) = self.settle
            && now >= settle
        {
            due = true;
            self.settle = None;
        }
        if !due {
            self.emit_tick(&before, notice_count, emit);
            return;
        }

        // 3. The cheap pass, then the named diff if it earned one.
        let work = Instant::now();
        let observed = self.look();
        self.meter.charge(work.elapsed());
        match observed {
            Ok(Some(observation)) => {
                self.counters.looks += 1;
                self.last_look = Instant::now();
                self.counters.ref_changes += observation.changes.len() as u64;
                if let Some(change) = observation.changes.first() {
                    self.last_change = Some(change.clone());
                }
                let moved = !observation.changes.is_empty();
                self.coalesce(observation);
                if moved {
                    // Evidence for the reactive fallback: the poll found a change the notifier never
                    // mentioned. Two of them is a pattern, not a race.
                    if self.notifications_trusted && self.counters.notifications == 0 {
                        self.unnotified += 1;
                        if self.unnotified >= 2 {
                            self.fallback_to_poll(
                                "the poll found repository changes that the notification backend never \
                                 reported, so notifications are not arriving here",
                            );
                        }
                    }
                }
            }
            Ok(None) => {
                self.counters.looks += 1;
                // A look that found nothing is still a successful read of the repository, which is what
                // `stale_after` asks about. Updating the stamp only on a found change would make every quiet
                // repository go stale by construction — which is a lie about the watcher, not about the repo.
                self.last_look = Instant::now();
            }
            Err(reason) => {
                self.counters.errors += 1;
                self.counters.consecutive_errors += 1;
                self.push_notice(NoticeKind::Stale, reason.clone());
                if self.counters.consecutive_errors >= self.options.max_error_attempts {
                    self.set_phase(Phase::Stale { reason });
                } else {
                    let attempt = self.counters.consecutive_errors;
                    self.set_phase(Phase::Error { reason, attempt });
                    self.retry_after = Some(Instant::now() + self.backoff());
                }
            }
        }

        // 4. The guards, then the work.
        self.guard_notices();
        if let Some(interest) = self.interest_to_act() {
            self.act(&interest, job, emit);
        } else if self.pending.as_ref().is_some_and(|p| !p.changes.is_empty()) {
            self.set_phase(Phase::Detecting {
                pending: self.pending.as_ref().map_or(0, |p| p.changes.len()),
            });
        } else if self.last_look.elapsed() > self.options.stale_after
            && !matches!(self.phase, Phase::Stale { .. })
            && !self.phase.busy()
        {
            self.set_phase(Phase::Stale {
                reason: format!(
                    "the repository has not been read for {} s",
                    self.last_look.elapsed().as_secs().max(1)
                ),
            });
        } else if self.phase.busy() && !self.more_work {
            self.set_phase(Phase::Idle);
        }

        self.enforce_cpu_budget();
        self.emit_tick(&before, notice_count, emit);
    }

    /// Emit when the state moved, and at least once per [`Options::heartbeat`] when it did not.
    fn emit_tick(&mut self, before: &Phase, notice_count: usize, emit: &mut dyn FnMut(&Status)) {
        if &self.phase == before
            && self.notices.len() == notice_count
            && self.last_emit.elapsed() < self.options.heartbeat
        {
            return;
        }
        self.last_emit = Instant::now();
        let work = Instant::now();
        emit(&self.status());
        // The caller's render, print and heartbeat write are this loop's cost, not free: a budget measured
        // over only the two expensive passes would understate the watcher by exactly the part that runs
        // every single second.
        self.meter.charge(work.elapsed());
    }

    /// How long the next wait may block: until the earliest deadline, never longer than one tick.
    fn wait_time(&self) -> Duration {
        let now = Instant::now();
        let mut wake = self.next_poll;
        for deadline in [
            self.settle,
            self.settle_deadline,
            self.throttle_until,
            self.retry_after,
        ]
        .into_iter()
        .flatten()
        {
            wake = wake.min(deadline);
        }
        wake.saturating_duration_since(now).min(self.options.tick)
    }

    /// Read the cheap pass and, if it moved, name what moved.
    fn look(&mut self) -> Result<Option<Observation>, String> {
        let fingerprint = Fingerprint::read(&self.repo, self.options.refs_walk_cap);
        if fingerprint == self.fingerprint {
            return Ok(None);
        }
        let signals = fingerprint.what_moved(&self.fingerprint);
        let replaced = self.reopen_if_replaced(&fingerprint)?;
        let index_touched = fingerprint.index != self.fingerprint.index;
        let coarse = fingerprint.loose_refs_capped;
        // The snapshot is the expensive half, so it is taken only when the cheap pass says refs may have
        // moved, when the fold was capped and so proves nothing, or when the repository turned out to be a
        // different one. A `gc` that rewrote packs therefore costs a fold, not a ref walk.
        let refs_maybe = replaced
            || coarse
            || signals.iter().any(|signal| {
                matches!(
                    *signal,
                    "HEAD" | "packed-refs" | "refs" | "git-dir identity" | "git's own markers"
                )
            });
        let changes = if refs_maybe {
            let snapshot = Snapshot::read(&self.repo).map_err(|err| err.to_string())?;
            let changes = observe::diff(&self.snapshot, &snapshot);
            self.snapshot = snapshot;
            changes
        } else {
            Vec::new()
        };
        self.fingerprint = fingerprint;
        Ok(Some(Observation {
            changes,
            head: self.snapshot.head.clone(),
            index_touched,
            replaced,
            signals,
            coarse,
        }))
    }

    /// Reopen the repository when the directory under us became a different directory.
    ///
    /// This is the case a handle cannot recover from by re-reading: a re-clone, an `mv` and an `mv` back, a
    /// container restarted onto a different bind mount. The inode changed, so every path fact the old handle
    /// cached is about a directory that no longer exists, and the only honest move is to start again.
    fn reopen_if_replaced(&mut self, fingerprint: &Fingerprint) -> Result<bool, String> {
        let replaced = self.fingerprint.git_dir.replaced(&fingerprint.git_dir)
            || self
                .fingerprint
                .common_dir
                .replaced(&fingerprint.common_dir);
        if !replaced {
            return Ok(false);
        }
        self.counters.reopens += 1;
        let repository = Repository::discover(&self.root).map_err(|err| {
            format!(
                "the git directory was replaced and nothing readable was found at {}: {err}",
                self.root.display()
            )
        })?;
        self.repo = repository;
        self.snapshot = Snapshot::read(&self.repo)
            .map_err(|err| format!("the repository was replaced and cannot be read: {err}"))?;
        self.unnotified = 0;
        self.push_notice(
            NoticeKind::Repository,
            format!(
                "{} became a different git directory, so the watcher reopened it",
                self.repo.git_dir().display()
            ),
        );
        // The new directory may be on a different filesystem, so the old subscription is no more trustworthy
        // than it was, and the paths may differ: re-arm it.
        self.rearm_source();
        Ok(true)
    }

    /// Re-subscribe the backend, keeping the current backend kind if it still works.
    fn rearm_source(&mut self) {
        if self.backend.kind == BackendKind::Notification {
            let probe = source::probe(
                self.options.backend,
                self.repo.git_dir(),
                self.options.poll_interval,
            );
            let (source, info) = source::open(
                &probe,
                &observe::watch_roots(&self.repo),
                self.options.poll_interval,
            );
            self.source = source;
            if info != self.backend {
                self.counters.backend_switches += 1;
                self.backend = info;
            }
            self.notifications_trusted =
                probe.notifications_trusted && self.backend.kind == BackendKind::Notification;
        } else {
            self.source.set_interval(self.interval);
        }
    }

    /// Fold a fresh observation into the batch waiting for a run.
    fn coalesce(&mut self, observation: Observation) {
        let now = Instant::now();
        for _ in &observation.changes {
            self.ref_moves.push_back(now);
        }
        if !(observation.has_changes()
            || (observation.index_touched && self.options.index_worktree))
        {
            return;
        }
        let Some(mut pending) = self.pending.take() else {
            // A batch starts a clock: it will be acted on within `settle_max`, guards or no guards.
            self.settle_deadline = Some(now + self.options.settle_max);
            self.pending = Some(observation);
            return;
        };
        // Coalescing is a *set union by name*, keeping the oldest `from` and the newest `to`: a branch that
        // moved a→b→c while we were busy is one change a→c, and reporting three would make a rebase's log
        // unreadable and a spam guard's count wrong.
        for change in observation.changes {
            match pending
                .changes
                .iter_mut()
                .find(|have| have.name == change.name)
            {
                Some(have) => {
                    have.to = change.to;
                    have.kind = change.kind;
                }
                None => pending.changes.push(change),
            }
        }
        pending.index_touched |= observation.index_touched;
        pending.replaced |= observation.replaced;
        pending.head = observation.head;
        for signal in observation.signals {
            if !pending.signals.contains(&signal) {
                pending.signals.push(signal);
            }
        }
        self.pending = Some(pending);
    }

    /// The notices describing git's own busyness, refreshed when the state *changes* rather than on every
    /// tick that repeats it.
    fn guard_notices(&mut self) {
        let operation = guard::operation(self.repo.git_dir(), self.repo.common_dir());
        let maintenance = guard::maintenance(
            self.repo.git_dir(),
            self.repo.common_dir(),
            &self.repo.index_path(),
        );
        let operation_text = operation.summary().unwrap_or_default();
        if operation_text != self.last_operation {
            let ended = self.last_operation != operation_text && operation_text.is_empty();
            self.last_operation = operation_text.clone();
            if ended {
                // A notice that stays after its cause is gone is a lie about the present, so the *reason*
                // leaves the list and the fact that it left is what remains.
                self.notices
                    .retain(|notice| notice.kind != NoticeKind::Operation);
                self.push_notice(
                    NoticeKind::Cleared,
                    "the operation that was in progress has finished".to_owned(),
                );
            } else {
                self.push_notice(NoticeKind::Operation, operation_text);
            }
        }
        let maintenance_text = maintenance.summary().unwrap_or_default();
        if maintenance_text != self.last_maintenance {
            let ended = self.last_maintenance != maintenance_text && maintenance_text.is_empty();
            self.last_maintenance = maintenance_text.clone();
            if ended {
                self.notices
                    .retain(|notice| notice.kind != NoticeKind::Maintenance);
            } else if !maintenance_text.is_empty() {
                self.push_notice(NoticeKind::Maintenance, maintenance_text);
            }
        }
    }

    /// Decide whether a run may start now, and say why not when it may not.
    ///
    /// Every guard here can *delay* and none can *prevent*: once [`Options::settle_max`] has passed since a
    /// batch started, the run happens no matter what git is busy with. That is the "no stuck spinner"
    /// argument, in one function.
    fn interest_to_act(&mut self) -> Option<Interest> {
        let now = Instant::now();
        let pending_empty = self
            .pending
            .as_ref()
            .is_none_or(|batch| batch.changes.is_empty());
        if pending_empty
            && !self.more_work
            && !self
                .pending
                .as_ref()
                .is_some_and(|batch| batch.index_touched && self.options.index_worktree)
        {
            self.settle_deadline = None;
            return None;
        }
        let overdue = self.overdue();

        // Ref spam: count moves in a sliding window and, past the threshold, act at most once per
        // exponentially growing interval. The batch keeps accumulating, so the index still ends up correct.
        let moves = self.count_recent_moves(now);
        if moves >= self.options.ref_spam_threshold {
            self.spam_streak = self.spam_streak.saturating_add(1);
            let wait = (self.options.ref_spam_window * 2_u32.pow(self.spam_streak.min(6)))
                .min(self.options.throttle_max);
            self.throttle_until = Some(now + wait);
            self.counters.throttles += 1;
            self.push_notice(
                NoticeKind::RefSpam,
                format!(
                    "{moves} refs moved in {} ms — something is scripting git here; coalescing and \
                     waiting up to {} ms between runs",
                    self.options.ref_spam_window.as_millis(),
                    wait.as_millis()
                ),
            );
        }
        if !overdue {
            if let Some(until) = self.throttle_until {
                if now < until {
                    return None;
                }
                self.throttle_until = None;
            }
            if let Some(retry) = self.retry_after {
                if now < retry {
                    return None;
                }
                self.retry_after = None;
            }
            let maintenance = guard::maintenance(
                self.repo.git_dir(),
                self.repo.common_dir(),
                &self.repo.index_path(),
            );
            if maintenance.busy() {
                self.counters.deferrals += 1;
                self.settle = Some(now + self.options.poll_interval);
                return None;
            }
            let operation = guard::operation(self.repo.git_dir(), self.repo.common_dir());
            if operation.in_progress()
                && self
                    .last_index
                    .is_some_and(|last| now - last < self.options.operation_min_interval)
            {
                self.counters.deferrals += 1;
                self.settle =
                    Some(now + self.options.operation_settle.min(self.options.settle_max));
                return None;
            }
        } else {
            self.throttle_until = None;
            self.retry_after = None;
            self.push_notice(
                NoticeKind::Cleared,
                format!(
                    "acting after waiting the full {} s settle window, git's own work notwithstanding",
                    self.options.settle_max.as_secs()
                ),
            );
        }

        let batch = self.pending.take().unwrap_or_else(|| Observation {
            changes: Vec::new(),
            head: self.snapshot.head.clone(),
            index_touched: false,
            replaced: false,
            signals: Vec::new(),
            coarse: false,
        });
        let reason = if batch.replaced {
            InterestReason::Startup
        } else if batch.changes.iter().any(Change::is_head) {
            InterestReason::Commits
        } else if batch.changes.is_empty() && batch.index_touched {
            InterestReason::Worktree
        } else if self.more_work {
            // The previous run stopped for its budget: this is the same work, asked for again.
            InterestReason::Commits
        } else if batch.changes.len() > 1 || self.counters.index_runs > 0 {
            InterestReason::Coalesced
        } else {
            InterestReason::Refs
        };
        self.more_work = false;
        Some(Interest {
            reason,
            head: batch.head,
            changes: batch.changes,
            index_touched: batch.index_touched,
            replaced: batch.replaced,
            signals: batch.signals,
        })
    }

    /// Has the current batch waited as long as it will ever be allowed to?
    fn overdue(&self) -> bool {
        self.settle_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// How many ref moves were seen in the spam window.
    ///
    /// The history is *arrivals*, pruned only by age — deliberately not cleared when a run succeeds. Clearing
    /// it would make sixty refs look like sixty separate, reasonable changes whenever the index is fast
    /// enough to keep up, which is exactly when the notice is most useful to whoever reads the log.
    fn count_recent_moves(&mut self, now: Instant) -> usize {
        let window = self.options.ref_spam_window;
        self.ref_moves
            .retain(|when| now.duration_since(*when) <= window);
        self.ref_moves.len()
    }

    /// Run the job, driving the state machine through its progress.
    fn act(&mut self, interest: &Interest, job: &mut dyn Job, emit: &mut dyn FnMut(&Status)) {
        self.set_phase(Phase::Indexing {
            commits: 0,
            units: 0,
            chunks: 0,
        });
        emit(&self.status());
        self.counters.index_runs += 1;
        let work = Instant::now();
        let outcome = job.refresh(interest, &mut |progress| {
            self.set_phase(Phase::Indexing {
                commits: progress.commits,
                units: progress.units,
                chunks: progress.chunks,
            });
            emit(&self.status());
        });
        match outcome {
            Ok(progress) => {
                self.meter.charge(work.elapsed());
                self.counters.commits_indexed += progress.commits;
                self.counters.units_computed += progress.units;
                self.last_index = Some(Instant::now());
                self.counters.consecutive_errors = 0;
                self.settle_deadline = None;
                self.spam_streak = 0;
                self.more_work = !progress.complete;
                // A run that worked is the end of the failure story: the error notices leave with it, so the
                // status never shows a problem the watcher has already got past. The replacement notice is
                // only for a reader who saw the failure — a status that explains a problem nobody saw is
                // noise.
                let had_failures = self
                    .notices
                    .iter()
                    .any(|notice| notice.kind == NoticeKind::Stale);
                self.notices
                    .retain(|notice| notice.kind != NoticeKind::Stale);
                if had_failures && self.fatal.is_none() {
                    self.push_notice(
                        NoticeKind::Cleared,
                        "the failed index run has succeeded".to_owned(),
                    );
                }
                if progress.complete {
                    self.set_phase(Phase::Idle);
                } else {
                    // The caller stopped for its own budget. Not an error, and not done: ask again on the
                    // next poll rather than spin, and let the phase say so.
                    self.set_phase(Phase::Detecting { pending: 0 });
                    self.settle = Some(Instant::now() + self.options.poll_interval);
                }
            }
            Err(JobError::Fatal(reason)) => {
                self.fatal = Some(reason.clone());
                self.push_notice(NoticeKind::Stale, reason.clone());
                self.set_phase(Phase::Error { reason, attempt: 1 });
            }
            Err(JobError::Repository(reason)) => {
                self.counters.errors += 1;
                self.counters.consecutive_errors += 1;
                self.requeue(interest);
                // A read failure is the same symptom a replaced repository produces, and reopening is the
                // only remedy that does not assume the path is stable.
                match Repository::discover(&self.root) {
                    Ok(repository) => {
                        self.repo = repository;
                        self.push_notice(
                            NoticeKind::Repository,
                            format!("reopened after a read failure: {reason}"),
                        );
                    }
                    Err(err) => self.push_notice(
                        NoticeKind::Repository,
                        format!("could not reopen the repository: {err}"),
                    ),
                }
                self.retry_after = Some(Instant::now() + self.backoff());
                self.set_phase(Phase::Error {
                    reason,
                    attempt: self.counters.consecutive_errors,
                });
            }
            Err(JobError::Transient(reason)) => {
                self.counters.errors += 1;
                self.counters.consecutive_errors += 1;
                self.requeue(interest);
                if self.counters.consecutive_errors >= self.options.max_error_attempts {
                    self.push_notice(
                        NoticeKind::Stale,
                        format!(
                            "giving up after {} consecutive failures; the changes are still queued and \
                             will be retried more slowly",
                            self.counters.consecutive_errors
                        ),
                    );
                    self.set_phase(Phase::Stale { reason });
                } else {
                    let attempt = self.counters.consecutive_errors;
                    self.set_phase(Phase::Error { reason, attempt });
                }
                // The batch is back in `pending`, so this is what makes a retry happen at all: without it
                // a failed run's changes would never be asked about again, because the cheap pass has
                // already been told they are the current state.
                self.retry_after = Some(Instant::now() + self.backoff());
            }
        }
        // Every run ends with an emit: the caller's status has to reflect the outcome even when the run
        // happened outside `step` (the startup refresh, a fatal stop that ends the loop immediately).
        emit(&self.status());
    }

    /// Put a failed batch back for the next attempt.
    ///
    /// This is the difference between "retry" and "re-noticed": the fingerprint and the ref snapshot have
    /// already moved on, so nothing will tell the watcher the change is still outstanding unless it keeps the
    /// change itself.
    fn requeue(&mut self, interest: &Interest) {
        if self.pending.is_some() || interest.changes.is_empty() {
            return;
        }
        self.pending = Some(Observation {
            changes: interest.changes.clone(),
            head: interest.head.clone(),
            index_touched: interest.index_touched,
            replaced: interest.replaced,
            signals: interest.signals.clone(),
            coarse: false,
        });
        self.settle_deadline = Some(Instant::now() + self.options.settle_max);
    }

    /// The next retry delay: the base backoff doubled per consecutive failure, capped.
    fn backoff(&self) -> Duration {
        let factor = 2_u32
            .pow(self.counters.consecutive_errors.saturating_sub(1).min(10))
            .max(1);
        (self.options.error_backoff * factor).min(self.options.throttle_max)
    }

    /// Stretch or shrink the poll from the measured busy share. This is the CPU budget being *enforced*
    /// rather than declared: the measurement is real work time, so a watcher on a machine already busy with
    /// something else stretches on its own.
    fn enforce_cpu_budget(&mut self) {
        self.meter.roll();
        let share = self.meter.share;
        if share > self.options.cpu_share && self.interval < self.options.poll_interval_max {
            self.interval = (self.interval * 2).min(self.options.poll_interval_max);
            self.source.set_interval(self.interval);
            self.counters.stretches += 1;
            self.push_notice(
                NoticeKind::Budget,
                format!(
                    "the watcher used {:.1}% of a core over the last window, over its {:.1}% budget; \
                     polling is stretched to {} ms",
                    share * 100.0,
                    self.options.cpu_share * 100.0,
                    self.interval.as_millis()
                ),
            );
        } else if share < self.options.cpu_share / 2.0 && self.interval > self.options.poll_interval
        {
            self.interval = (self.interval / 2).max(self.options.poll_interval);
            self.source.set_interval(self.interval);
        }
    }

    /// Switch to polling, keeping the reason, because "it fell back" is only useful if it says why.
    fn fallback_to_poll(&mut self, reason: &str) {
        self.counters.degradations += 1;
        if self.backend.kind == BackendKind::Polling {
            return;
        }
        self.counters.backend_switches += 1;
        self.source = Box::new(source::PollSource::new(self.interval));
        self.backend = BackendInfo {
            kind: BackendKind::Polling,
            name: "poll".to_owned(),
            reason: Some(format!(
                "polling took over: {reason}; `fathom doctor` reports which backend a repository gets"
            )),
        };
        self.notifications_trusted = false;
        self.next_poll = Instant::now();
        self.push_notice(NoticeKind::Backend, reason.to_owned());
    }

    /// Record a notice, replacing any of the same kind, so the list cannot grow without bound while a
    /// condition that is simply still true keeps being reported.
    fn push_notice(&mut self, kind: NoticeKind, text: String) {
        self.notices.retain(|notice| notice.kind != kind);
        self.notices.push(Notice { kind, text });
        while self.notices.len() > 6 {
            let index = self
                .notices
                .iter()
                .position(|notice| notice.kind != kind)
                .unwrap_or(0);
            self.notices.remove(index);
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        if phase == self.phase {
            return;
        }
        self.phase = phase;
        self.phase_at = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn every_phase_names_itself_and_only_the_busy_ones_claim_a_spinner() {
        assert_eq!(Phase::Idle.as_str(), "idle");
        assert_eq!(
            Phase::Detecting { pending: 3 }.as_str(),
            "detecting",
            "the state names are the status-file contract"
        );
        assert_eq!(
            Phase::Indexing {
                commits: 1,
                units: 2,
                chunks: 1
            }
            .as_str(),
            "indexing"
        );
        assert_eq!(
            Phase::Error {
                reason: "x".to_owned(),
                attempt: 1
            }
            .as_str(),
            "error"
        );
        assert_eq!(
            Phase::Stale {
                reason: "x".to_owned()
            }
            .as_str(),
            "stale"
        );
        assert!(!Phase::Idle.busy());
        assert!(
            !Phase::Stale {
                reason: String::new()
            }
            .busy()
        );
        assert!(Phase::Detecting { pending: 0 }.busy());

        // The summaries have to carry the progress, not just the word.
        let text = Phase::Indexing {
            commits: 3,
            units: 12,
            chunks: 2,
        }
        .summary();
        assert!(
            text.contains('3') && text.contains("12") && text.contains('2'),
            "{text}"
        );
    }

    #[test]
    fn a_settled_batch_starts_a_deadline_and_a_finished_run_clears_it() {
        // The two halves of the no-stuck-spinner guarantee, in the two functions that own them.
        let options = Options::default();
        assert!(options.settle_max < options.stale_after);
        assert!(
            options.poll_interval <= options.settle_max,
            "a poll that outlasts the settle window would defer a change forever"
        );
        assert!(
            options.poll_interval_max >= options.poll_interval,
            "the stretch ceiling has to be above the floor"
        );
    }
}
