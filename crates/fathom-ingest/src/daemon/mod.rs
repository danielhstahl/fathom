//! The watch daemon: one process per repository that watches, tails and indexes, while every other
//! process asks it instead of doing it itself (`e6.3` / `reco-kvk`, ADR-004 §7.1–§7.2).
//!
//! # The problem this is the answer to
//!
//! Before this, every process that wanted a current index did its own work: `fathom watch` indexed,
//! `fathom ambient` indexed when no watcher was alive, `fathom index` indexed, and a TUI read a store
//! that somebody had forgotten to refresh. Three terminals plus a CLI was four processes stat-ing the
//! same refs and, in the worst case, four index passes over the same commits — each one correct, and the
//! waste invisible. ADR-004 §7.1's rule is the fix: **who writes is a fact about a lock, not a fact about
//! which binary you ran.** This module is the process that wins that lock, and [`proto`] is the way
//! everybody else talks to it instead of duplicating the work.
//!
//! # The shape of one loop pass
//!
//! ```text
//!   ┌───────────────────────────── one pass of the loop ─────────────────────────────┐
//!   │ 1. clients first    accept, answer status/hello, queue index requests,         │
//!   │                     park the ones that asked to wait                           │
//!   │ 2. hook stream      at most `hook_read_bytes` from the durable cursor; the lag │
//!   │                     left behind *is* the back-pressure                         │
//!   │ 3. the budget       close the window, pay the deficit as an interruptible sleep│
//!   │ 4. the watcher      `watch::Watch::step` — detection, guards, maybe a run      │
//!   │ 5. the queue        if clients asked and the watcher did not run, run once     │
//!   │ 6. the beat         lease heartbeat, parked-client replies, status file        │
//!   └─────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! One thread, on purpose. A second indexing thread would double the peak the acceptance criterion is
//! about, and every step above is written as a *bound*: the client budget per pass, the hook byte cap,
//! the work slice and the budget sleep. Those four numbers are why a TUI attached to this daemon stays
//! responsive while an agent commits underneath it.
//!
//! # What is delegated, and to whom
//!
//! | job | owner | why it is not here |
//! |---|---|---|
//! | noticing repository change | [`fathom_ingest::watch`] | it is a tested state machine with its own guards; a daemon that re-implemented it would drift from `fathom watch` |
//! | who may write the store | [`fathom_store::lease`] | the lease is a storage-concurrency object (ADR-004 §7) and it is useful to verbs that never become daemons |
//! | the index run itself | the caller's [`Work`] | indexing needs `fathom-analyze` and the store, both of which are uphill of this crate (ADR-002 R1) — so the daemon is handed a worker, and the tests drive the real loop with a scripted one |
//! | the wall clock | the caller's [`Clock`] | ADR-002 §4.4: whoever stamps time is the crate that decides what time means |
//!
//! # Shutdown, and what "clean" buys
//!
//! Three ways the loop ends — a client's `stop`, the caller's stop flag (Ctrl-C, SIGTERM, SIGHUP), and a
//! fatal store error — and all three take one path: finish the pass in flight, answer anybody still
//! waiting, [`Work::settle`] (checkpoint, close), release the lease.
//!
//! Consistency does **not** depend on that path running. Every write is a committed transaction, so
//! `kill -9` leaves the same store an orderly exit does, and the next process's `Store::recover_runs`
//! marks the abandoned run `crashed` and continues from the last durable commit (ADR-004 §9.2). The
//! orderly path buys a *checkpoint and a released lock*; it is not load-bearing for correctness, and a
//! design whose crash-recovery required a signal handler to run would be a design with a bug instead of a
//! daemon with a crash.
//!
//! # Notices
//!
//! Anything the daemon wants a human to see is a notice — the lease it took over, the socket path it fell
//! back to, an events file that rotated, a queue that failed. Notices are replaced per kind, for the
//! reason the watcher already learned: a notice that outlives its cause is a lie about the present.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use fathom_core::error::{Diagnosable, ErrorCode};
use fathom_core::time::{Clock, Timestamp};
use fathom_store::lease::{self, Lease, Record, StaleAfter};

use crate::hooks::Reader;
use crate::watch::{
    self, Change, Interest, InterestReason, JobError, Options as WatchOptions, Progress, Status,
    Watch,
};

pub mod budget;
pub mod proto;
pub mod queue;
pub mod socket;

pub use budget::{Budget, Source as MeterSource};
pub use proto::{PROTOCOL, Reply, Request};
pub use queue::{Admission, Lag, Queue};
pub use socket::{Endpoint, Env, Kind as EndpointKind, Reason as EndpointReason};

/// How many client connections one loop pass will service. A bound, not a tuning knob: the loop that
/// answers clients is the loop that indexes, so an unbounded accept is a client flood that starves the
/// repository. A connection left over waits one tick, which is what a tick is for.
pub const CLIENTS_PER_CYCLE: usize = 8;

/// Why the daemon could not start.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Another process holds the writer lease. The message names it and points at `fathom status`.
    #[error("{0}")]
    Busy(String),
    /// The endpoint could not be bound, or this platform cannot bind one and `--require-socket` was given.
    #[error("{0}")]
    Socket(String),
    /// The repository could not be opened, so there is nothing to watch.
    #[error("{0}")]
    Repository(String),
}

impl Diagnosable for Error {
    fn code(&self) -> ErrorCode {
        match self {
            // The lease is held by a live writer, which is the cancellation-shaped failure ADR-002 §7.3
            // already has a code and an exit for.
            Error::Busy(_) => ErrorCode::Cancelled,
            Error::Socket(_) => ErrorCode::UnsupportedPlatform,
            Error::Repository(_) => ErrorCode::NotARepository,
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            Error::Busy(_) => Some("ask the daemon that is running: `fathom status`"),
            Error::Socket(why) => Some(why.as_str()),
            Error::Repository(_) => None,
        }
    }
}

/// What the daemon needs from the outside world to do work, and what it promises on the way out.
///
/// The daemon deliberately does not know what "index" means: it knows how big a slice to ask for, how
/// long to allow, and when to stop asking. That is the whole interface between a budget and an index, and
/// it is what lets `tests/daemon.rs` drive the real loop with a scripted worker.
pub trait Work {
    /// Bring the index up to date, spending at most `slice` analysis units and `budget` of wall time.
    ///
    /// A run that stops early for its slice returns [`Progress::complete`] `false` and the daemon asks
    /// again — the same contract [`watch::Job`] already has, because the same rule produced it (ADR-001
    /// §7.6: a stated partial, never a silent one).
    fn pass(
        &mut self,
        interest: &Interest,
        slice: usize,
        budget: Duration,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError>;

    /// What this worker will actually do, in words and numbers a `hello` can carry. The default says the
    /// honest minimum; a real worker says whether a store backend is compiled in, which is the first
    /// thing a client needs to know before it waits for a pass that would do nothing.
    fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({"worker": "scripted"})
    }

    /// The daemon is leaving: checkpoint and close, so the next process finds a whole store (ADR-004
    /// §7.4 — `wal_checkpoint(TRUNCATE)` at daemon idle and on clean shutdown).
    ///
    /// The error is a sentence, not a taxonomy: whatever it is, the daemon is leaving and the store is
    /// still consistent. It is reported and counted, never retried.
    fn settle(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Everything about a daemon that is a choice rather than a measurement.
#[derive(Debug, Clone)]
pub struct Options {
    /// The watch options: backends, debounce, the state machine's own budgets.
    pub watch: WatchOptions,
    /// An explicit socket path (`--socket`, or the config key). `None` derives one — see [`socket`].
    pub socket: Option<PathBuf>,
    /// The environment the endpoint resolver reads.
    pub env: Env,
    /// Where the writer lease lives: ADR-004 §3's `.fathom/daemon.lock` by default.
    pub lease_path: PathBuf,
    /// How long a claim may go without a beat before it counts as stale.
    pub lease_stale: Duration,
    /// The CPU ceiling, as a share of one core.
    pub cpu_share: f64,
    /// The window that share is measured over.
    pub cpu_window: Duration,
    /// Analysis units per pass while the foreground is quiet.
    pub slice: usize,
    /// Analysis units per pass while a foreground client has just been heard.
    pub yield_slice: usize,
    /// How many distinct index requests may wait.
    pub queue_depth: usize,
    /// The hook stream's per-pass byte cap — the back-pressure knob (see [`Lag`]).
    pub hook_read_bytes: u64,
    /// The events file to tail. `None` is `<repo>/.fathom/events.jsonl`.
    pub events: Option<PathBuf>,
    /// The loop tick: how long a wait may block, and the chunk size of an enforced budget sleep.
    pub tick: Duration,
    /// Stop after this much wall time — a supervisor seam and the verb's `--deadline`.
    pub deadline: Option<Duration>,
    /// Stop after this many loop passes — the test seam and the verb's `--max-cycles`.
    pub max_cycles: Option<u64>,
    /// Refuse to start rather than run without an IPC endpoint. Off by default: the lease and the
    /// heartbeat already give `fathom status` something to read where no socket transport exists, so a
    /// daemon that refused to run there would be an unusable daemon, not a safer one.
    pub require_socket: bool,
    /// The build string stamped into the lease and the health document, e.g. `fathom 0.1.0-alpha.1`.
    pub build: String,
    /// Who is supervising this daemon (`--foreground`'s argument), printed by `fathom status` so a
    /// container's log can say which process was supposed to be keeping it up.
    pub supervisor: Option<String>,
}

impl Options {
    /// The defaults for one repository: everything derived, nothing explicit.
    #[must_use]
    pub fn for_root(root: &Path) -> Options {
        Options {
            watch: WatchOptions::default(),
            socket: None,
            env: Env::default(),
            lease_path: lease::default_path(root),
            lease_stale: lease::DEFAULT_STALE_AFTER,
            cpu_share: 0.05,
            cpu_window: Duration::from_secs(1),
            slice: 2_000,
            yield_slice: 50,
            queue_depth: 64,
            hook_read_bytes: 1024 * 1024,
            events: None,
            tick: Duration::from_millis(50),
            deadline: None,
            max_cycles: None,
            require_socket: false,
            build: "fathom".to_owned(),
            supervisor: None,
        }
    }
}

/// Why the loop ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The caller's stop flag, or a client's `stop` request.
    Requested(String),
    /// [`Options::deadline`] passed.
    Deadline,
    /// [`Options::max_cycles`] ran out — the test and `--once` seam.
    CycleBudget,
    /// A [`Work`] said the store is corrupt. The one reason that ends the loop rather than the pass.
    Fatal(String),
}

impl Stop {
    /// The last line the daemon prints.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            Stop::Requested(why) | Stop::Fatal(why) => format!("stopped: {why}"),
            Stop::Deadline => "stopped at its deadline".to_owned(),
            Stop::CycleBudget => "stopped after its cycle budget".to_owned(),
        }
    }
}

/// One thing the daemon wants a human to know: a kind (stable, because a script greps for it) and a
/// sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// What kind of thing it is.
    pub kind: String,
    /// The sentence.
    pub text: String,
}

/// What a pass of work produced, kept so "what did the last pass do" is answerable without a log.
#[derive(Debug, Clone)]
pub struct Pass {
    /// Why the pass ran: `watcher` or `request`.
    pub reason: &'static str,
    /// What the worker reported.
    pub progress: Progress,
    /// Wall time it took.
    pub wall: Duration,
    /// Whether the run finished or stopped for its slice.
    pub complete: bool,
    /// The failure, if the worker failed rather than stopped.
    pub error: Option<String>,
    /// Whether the failure was the fatal kind.
    pub fatal: bool,
}

impl Pass {
    /// The status-document form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "reason": self.reason,
            "commits": self.progress.commits,
            "units": self.progress.units,
            "chunks": self.progress.chunks,
            "complete": self.complete,
            "wall_ms": self.wall.as_millis(),
            "error": self.error,
            "fatal": self.fatal,
        })
    }
}

/// A parked client: it asked to be told when a pass covered its request, so the daemon holds the
/// connection open instead of the client polling.
#[cfg(unix)]
struct Waiter {
    stream: std::os::unix::net::UnixStream,
    deadline: Instant,
}

/// The daemon. One per repository, one per process — the second one's lease attempt is what says so.
pub struct Daemon {
    root: PathBuf,
    options: Options,
    lease: Lease,
    /// `None` only where no socket transport exists, or binding failed with `require_socket` off — a
    /// state the health document prints rather than hides.
    listener: Option<socket::Listener>,
    endpoint: Endpoint,
    watch: Watch,
    budget: Budget,
    queue: Queue,
    lag: Lag,
    reader: Reader,
    cursor: crate::hooks::Cursor,
    clock: Box<dyn Clock>,
    started: Instant,
    started_utc_ms: i64,
    last_beat: Instant,
    notices: Vec<Notice>,
    errors: Vec<String>,
    /// Passes started, including failed ones: the headline number behind "exactly one indexing pass".
    passes: u64,
    /// Requests served, counted by origin — the counters that make "three TUIs plus a CLI made one pass"
    /// assertable rather than plausible.
    by_origin: BTreeMap<String, u64>,
    last_pass: Option<Pass>,
    clients: u64,
    refused: u64,
    cycles: u64,
    /// Cursor saves, which is not the same as passes: a pass that read nothing saves nothing.
    cursor_saves: u64,
    shutdown: Option<String>,
    fatal: Option<String>,
    /// Clients parked on a reply.
    #[cfg(unix)]
    waiters: Vec<Waiter>,
}

impl Daemon {
    /// Take the lease, resolve and bind the endpoint, open the watcher.
    ///
    /// The order is the single-instance guarantee: the endpoint is *resolved* first (a pure function), the
    /// lease is taken second, and the socket is bound third — so a second daemon never removes the first
    /// one's socket file on its way in. Binding then asks the socket itself whether something is already
    /// listening, which catches the case a lease cannot: two repositories configured onto one endpoint.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] when a live process holds the lease, [`Error::Repository`] when there is nothing to
    /// watch, [`Error::Socket`] when the endpoint cannot be bound and `require_socket` is set.
    pub fn open(root: &Path, options: Options, clock: Box<dyn Clock>) -> Result<Daemon, Error> {
        let endpoint = socket::resolve(root, options.socket.as_deref(), &options.env);
        let stamp = clock.now();
        let record = Record {
            version: lease::RECORD_VERSION,
            pid: std::process::id(),
            boot_id: lease::boot_id().unwrap_or_else(|| lease::UNKNOWN.to_owned()),
            pid_start: lease::self_start_token(),
            started_utc_ms: unix_ms(stamp),
            // Stamped with the endpoint the resolver chose, so a client that can only read the lease
            // (`fathom status` with no daemon to ask) still finds where to knock.
            socket: if endpoint.bindable() {
                endpoint.path.display().to_string()
            } else {
                String::new()
            },
            writer_epoch: 0,
            build: options.build.clone(),
            root: root.display().to_string(),
        };
        let lease = Lease::acquire(
            &options.lease_path,
            record,
            SystemTime::now(),
            StaleAfter(options.lease_stale),
        )
        .map_err(|taken| Error::Busy(taken.message()))?;

        let mut notices = Vec::new();
        if let Some(why) = lease.broken() {
            notices.push(Notice {
                kind: "lease".to_owned(),
                text: format!("the writer lease was taken over: {why}"),
            });
        }
        if let Some(from) = endpoint.fallback_from.clone() {
            notices.push(Notice {
                kind: "socket".to_owned(),
                text: format!(
                    "{} is {} bytes, over the {}-byte unix socket name limit, so the endpoint is {}",
                    from.display(),
                    from.as_os_str().len(),
                    socket::SUN_PATH_LIMIT,
                    endpoint.path.display()
                ),
            });
        }

        let listener = match socket::bind(&endpoint) {
            Ok(listener) => Some(listener),
            Err(err) if options.require_socket => {
                return Err(Error::Socket(format!(
                    "{}: {err} — and --require-socket said that is fatal",
                    endpoint.path.display()
                )));
            }
            Err(err) => {
                notices.push(Notice {
                    kind: "socket".to_owned(),
                    text: format!(
                        "no IPC endpoint is published ({err}); `fathom status` reads the lease and the \
                         heartbeat file instead"
                    ),
                });
                None
            }
        };

        let watch = Watch::open(root, options.watch.clone())
            .map_err(|err| Error::Repository(err.to_string()))?;
        let events = options
            .events
            .clone()
            .unwrap_or_else(|| root.join(".fathom").join("events.jsonl"));
        let reader = Reader::new(&events);
        let (cursor, notice) = reader.open_cursor(unix_ms(stamp));
        if let Some(notice) = notice {
            notices.push(Notice {
                kind: notice.kind,
                text: notice.detail,
            });
        }

        let queue_depth = options.queue_depth;
        let mut budget = Budget::new(options.cpu_share, options.cpu_window);
        budget.slice = options.slice;
        budget.yield_slice = options.yield_slice;
        let lag = Lag {
            cap_bytes: options.hook_read_bytes,
            ..Lag::default()
        };
        Ok(Daemon {
            root: root.to_path_buf(),
            options,
            lease,
            listener,
            endpoint,
            watch,
            budget,
            queue: Queue::new(queue_depth),
            lag,
            reader,
            cursor,
            clock,
            started: Instant::now(),
            started_utc_ms: unix_ms(stamp),
            last_beat: Instant::now(),
            notices,
            errors: Vec::new(),
            passes: 0,
            by_origin: BTreeMap::new(),
            last_pass: None,
            clients: 0,
            refused: 0,
            cycles: 0,
            cursor_saves: 0,
            shutdown: None,
            fatal: None,
            #[cfg(unix)]
            waiters: Vec::new(),
        })
    }

    /// The lease this daemon holds.
    #[must_use]
    pub fn lease(&self) -> &Lease {
        &self.lease
    }

    /// The endpoint, bound or not, and why it is where it is.
    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The watcher's own status — the same shape `fathom watch` prints, which is what lets `fathom
    /// doctor` and the ambient pane read a daemon without knowing it is one.
    #[must_use]
    pub fn watch_status(&self) -> Status {
        self.watch.status()
    }

    /// The notices a reader should see.
    #[must_use]
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// Loop passes so far.
    #[must_use]
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Passes of work taken.
    #[must_use]
    pub fn passes(&self) -> u64 {
        self.passes
    }

    /// Requests counted by origin, which is the evidence behind "one pass, four askers".
    #[must_use]
    pub fn requests_by_origin(&self) -> &BTreeMap<String, u64> {
        &self.by_origin
    }

    /// The queue, for a caller that wants counters rather than a JSON document.
    #[must_use]
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// How long this daemon has been up.
    #[must_use]
    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    /// The last pass, or `None` before the first.
    #[must_use]
    pub fn last_pass(&self) -> Option<&Pass> {
        self.last_pass.as_ref()
    }

    /// The measured CPU share of the last closed window.
    #[must_use]
    pub fn cpu_share(&self) -> f64 {
        self.budget.measured()
    }

    /// Run until the stop flag says so, the deadline passes, the cycle budget runs out, or a [`Work`]
    /// calls the store fatal.
    ///
    /// `beat` is called once per loop pass with a borrow of the daemon: that is where the caller writes
    /// the heartbeat file, prints a foreground line, or asserts on a fixture. [`Daemon::beat_due`] tells
    /// it whether this is the pass where the heartbeat should actually be rewritten, so a 50 ms tick does
    /// not become 20 status-file writes a second.
    pub fn run(
        &mut self,
        work: &mut dyn Work,
        stop: &dyn Fn() -> bool,
        beat: &mut dyn FnMut(&Daemon),
    ) -> Stop {
        loop {
            if let Some(why) = self.shutdown.take() {
                self.leave(work, &why);
                return Stop::Requested(why);
            }
            if stop() {
                self.leave(work, "the process was asked to stop");
                return Stop::Requested("the process was asked to stop".to_owned());
            }
            if let Some(deadline) = self.options.deadline
                && self.started.elapsed() >= deadline
            {
                self.leave(work, "its deadline passed");
                return Stop::Deadline;
            }
            if let Some(budget) = self.options.max_cycles
                && self.cycles >= budget
            {
                self.leave(work, "its cycle budget ran out");
                return Stop::CycleBudget;
            }
            if let Some(why) = self.fatal.take() {
                self.leave(work, &why);
                return Stop::Fatal(why);
            }
            self.step(work);
            beat(self);
        }
    }

    /// One loop pass, in the six steps the module header draws.
    ///
    /// Public because a caller that owns the thread — a test, a supervisor that interleaves its own work
    /// — may want to drive it one pass at a time, which is the same reason [`Watch::step`] is public.
    pub fn step(&mut self, work: &mut dyn Work) {
        self.cycles += 1;

        // 1. Clients first, always. Every request answered also marks the foreground present, which is
        //    what shrinks the slice in step 4 and 5.
        self.poll_clients(work);
        self.answer_waiters();

        // 2. The hook stream, capped. This is the only place the cursor advances.
        self.tail_hooks();

        // 3. The budget: close the window if it is due, and pay any deficit as an interruptible sleep
        //    *before* taking more work, so the sleep bounds the loop's latency instead of adding to it.
        self.budget.roll();
        if self.budget.in_debt() {
            let nap = self.budget.sleep_now(self.options.tick);
            if !nap.is_zero() {
                std::thread::sleep(nap);
                return;
            }
        }

        // 4. The watcher. It may decide to run the job itself, which is the case `fathom watch` already
        //    covers; the queue is the addition, not a replacement.
        let before = self.passes;
        self.drive_watch(work);
        let watch_ran = self.passes > before;

        // 5. What the clients asked for, when the watcher's own guards did not already produce a run.
        if !self.queue.is_empty() && !watch_ran {
            self.run_pass(work);
        }

        // 6. The beat: the lease heartbeat. The status file is the caller's, in its `beat` callback.
        if let Err(err) = self.lease.heartbeat() {
            self.push_notice(
                "lease",
                format!("the lease heartbeat could not be refreshed: {err}"),
            );
        }
    }

    /// Is this the pass where the caller should rewrite its status file? The watcher's heartbeat interval
    /// is the answer, and the daemon owns the clock so the caller does not have to.
    #[must_use]
    pub fn beat_due(&mut self) -> bool {
        if self.last_beat.elapsed() < self.options.watch.heartbeat {
            return false;
        }
        self.last_beat = Instant::now();
        true
    }

    /// The health document — everything `fathom status --json` prints that the daemon itself knows.
    ///
    /// Cache statistics and the store's epoch are deliberately **not** in here: this crate cannot open the
    /// store (ADR-002 §3), and the CLI merges them from the store it opens itself. The split is named in
    /// the document, so a reader knows which half came from where.
    #[must_use]
    pub fn health(&self) -> serde_json::Value {
        let status = self.watch.status();
        // The lease as the kernel sees it, read here rather than remembered from what this process wrote: a
        // daemon that lost its flock should say so in its own report.
        let lease_state = Lease::state(
            self.lease.path(),
            SystemTime::now(),
            StaleAfter(self.options.lease_stale),
        );
        serde_json::json!({
            "protocol": PROTOCOL,
            "protocol_version": proto::VERSION,
            "pid": std::process::id(),
            "build": self.options.build,
            "supervisor": self.options.supervisor,
            "repository": self.root.display().to_string(),
            "started_utc_ms": self.started_utc_ms,
            "uptime_ms": self.uptime().as_millis(),
            "stamp_utc_ms": unix_ms(self.clock.now()),
            "cycles": self.cycles,
            "lease": {
                "path": self.lease.path().display().to_string(),
                // The same two words the client prints when it has to read the lease file instead of asking:
                // a reader should not have to know which half of the report it is looking at to ask "is the
                // writer lease held". This is the daemon *asking the kernel*, not remembering what it wrote.
                "state": lease_state.as_str(),
                "held": lease_state.held(),
                "epoch": self.lease.epoch(),
                "platform": self.lease.platform().as_str(),
                "platform_note": self.lease.platform().note(),
                "boot_id": self.lease.record().boot_id,
                "pid_start": self.lease.record().pid_start,
                "broken": self.lease.broken(),
                "heartbeat_age_ms": Lease::heartbeat_age(
                    self.lease.path(),
                    SystemTime::now()
                ).map(|age| age.as_millis()),
            },
            "socket": {
                "path": self.endpoint.path.display().to_string(),
                "kind": self.endpoint.kind.as_str(),
                "reason": self.endpoint.reason.as_str(),
                "fallback_from": self
                    .endpoint
                    .fallback_from
                    .as_ref()
                    .map(|path| path.display().to_string()),
                "listening": self.listener.is_some(),
                "sun_path_limit": socket::SUN_PATH_LIMIT,
                "fits": self.endpoint.fits(),
            },
            "watch": watch_json(&status),
            "cpu": self.budget.stats(),
            "queue": self.queue.stats(),
            "hooks": self.hooks_json(),
            "passes": {
                "count": self.passes,
                "requests_by_origin": self.by_origin,
                "clients_served": self.clients,
                "clients_refused": self.refused,
                "accept_errors": self.accept_errors(),
                "last": self.last_pass.as_ref().map(Pass::to_json),
            },
            "notices": self
                .notices
                .iter()
                .map(|notice| serde_json::json!({"kind": &notice.kind, "text": &notice.text}))
                .collect::<Vec<_>>(),
            "errors": self.errors,
            "shutdown": self.shutdown,
        })
    }

    /// The hook offsets, as the status document prints them: the durable cursor plus the lag behind it.
    #[must_use]
    pub fn hooks_json(&self) -> serde_json::Value {
        let size = self.reader.size();
        serde_json::json!({
            "events": self.reader.events.display().to_string(),
            "cursor": self.reader.cursor_path().display().to_string(),
            "offset": self.cursor.offset,
            "file_bytes": size,
            "lag_bytes": size.saturating_sub(self.cursor.offset),
            "last_seq": self.cursor.last_seq,
            "reads": self.cursor.reads,
            "rotations": self.cursor.rotations,
            "cursor_saves": self.cursor_saves,
            "turns": self.cursor.ledger.turns.len(),
            "open_turns": self.cursor.ledger.open_turns(),
            "counters": self.cursor.counters.to_json(),
            "stream": self.lag.stats(),
        })
    }

    /// Record a notice, replacing any of the same kind, so a condition that stays true is said once.
    fn push_notice(&mut self, kind: &str, text: String) {
        self.notices.retain(|notice| notice.kind != kind);
        self.notices.push(Notice {
            kind: kind.to_owned(),
            text,
        });
        while self.notices.len() > 6 {
            self.notices.remove(0);
        }
    }

    /// Drop a notice whose cause is gone — the other half of "a notice is a statement about the present".
    fn clear_notice(&mut self, kind: &str) {
        self.notices.retain(|notice| notice.kind != kind);
    }

    /// Failed accepts, for the status page. Zero where there is no listener.
    #[must_use]
    fn accept_errors(&self) -> u64 {
        self.listener
            .as_ref()
            .map_or(0, socket::Listener::accept_errors)
    }

    /// Accept up to [`CLIENTS_PER_CYCLE`] connections and answer them.
    #[cfg(unix)]
    fn poll_clients(&mut self, work: &mut dyn Work) {
        let mut served = 0;
        while served < CLIENTS_PER_CYCLE {
            let Some(connection) = self.listener.as_ref().and_then(socket::Listener::accept) else {
                return;
            };
            served += 1;
            self.serve(connection, work);
        }
    }

    /// No socket transport on this platform: there is nobody to answer. Stated in the health document's
    /// `socket.listening`, not hidden here.
    #[cfg(not(unix))]
    fn poll_clients(&mut self, _work: &mut dyn Work) {}

    /// Answer one client.
    #[cfg(unix)]
    fn serve(&mut self, mut connection: socket::Connection, work: &mut dyn Work) {
        self.budget.heard();
        let line = match connection.request() {
            Ok(Some(line)) => line,
            // EOF or an empty line: a liveness probe, or a client that gave up. Nothing to answer, and
            // nothing counted as a refusal — it never asked anything.
            Ok(None) => return,
            Err(err) => {
                self.refused += 1;
                let _ = connection.reply(&proto::err(
                    "",
                    proto::code::FAILED,
                    &format!("the request could not be read: {err}"),
                ));
                return;
            }
        };
        self.clients += 1;
        let request = match Request::parse(&line) {
            Ok(request) => request,
            Err((message, why)) => {
                self.refused += 1;
                let _ = connection.reply(&proto::err(
                    "",
                    why.unwrap_or(proto::code::MALFORMED),
                    &message,
                ));
                return;
            }
        };
        match request {
            Request::Hello => {
                let data = serde_json::json!({
                    "protocol": PROTOCOL,
                    "version": proto::VERSION,
                    "methods": ["hello", "status", "index", "stop"],
                    "pid": std::process::id(),
                    "build": self.options.build,
                    "uptime_ms": self.uptime().as_millis(),
                    "socket": self.endpoint.path.display().to_string(),
                    "worker": work.capabilities(),
                });
                let _ = connection.reply(&proto::ok("hello", data));
            }
            Request::Status => {
                let _ = connection.reply(&proto::ok("status", self.health()));
            }
            Request::Index { origin, wait_ms } => {
                *self.by_origin.entry(origin.clone()).or_insert(0) += 1;
                // One queue entry per *asker*, not one per method: three TUIs asking for the same
                // thing is one unit of work, and a drain covers every entry waiting in one pass. Keying
                // on the origin is what makes the depth bound mean "distinct askers" — keying on the
                // method would cap the queue at one entry always, and then the depth, the high-water mark
                // and the shed counter would all be decoration (`e6.3`).
                match self.queue.push(&origin, &origin, wait_ms > 0) {
                    Admission::Shed => {
                        self.refused += 1;
                        let _ = connection.reply(&proto::err(
                            "index",
                            proto::code::REFUSED,
                            &format!(
                                "the queue is full ({} waiting of {}) — a pass is running or about to, \
                                 and this request would be redundant; `fathom status` shows the queue",
                                self.queue.len(),
                                self.queue.depth()
                            ),
                        ));
                        return;
                    }
                    Admission::Coalesced if wait_ms == 0 => {
                        let _ = connection.reply(&proto::ok(
                            "index",
                            serde_json::json!({
                                "queued": false,
                                "coalesced": true,
                                "passes": self.passes,
                                "reason": "an identical request is already waiting",
                            }),
                        ));
                        return;
                    }
                    Admission::Queued | Admission::Coalesced => {}
                }
                if wait_ms == 0 {
                    let _ = connection.reply(&proto::ok(
                        "index",
                        serde_json::json!({"queued": true, "coalesced": false}),
                    ));
                } else {
                    // Parked. The reply goes out from `cover_request` when a pass finishes, or from
                    // `answer_waiters` when the wait runs out — which is what lets `fathom index` defer to
                    // the daemon and still print a report of what the pass did. Parking has to take the
                    // connection out of `Drop`'s reach: a connection that closes on the way out hands the
                    // client an EOF instead of an answer, which is the bug this branch exists to not have.
                    //
                    // The clone is taken *before* the park, because the park consumes the connection and the
                    // failure path still owes this client a sentence.
                    let held = connection.try_clone();
                    match connection.park() {
                        Ok(stream) => self.waiters.push(Waiter {
                            stream,
                            deadline: Instant::now() + Duration::from_millis(wait_ms.max(1)),
                        }),
                        Err(err) => {
                            // The one case where the daemon cannot keep the promise: say so in the reply it
                            // *can* send, rather than letting the client wonder why it timed out.
                            if let Some(stream) = held {
                                let _ = reply(
                                    &stream,
                                    &proto::ok(
                                        "index",
                                        serde_json::json!({
                                            "queued": true,
                                            "coalesced": false,
                                            "waited": false,
                                            "reason": format!(
                                                "the connection could not be held open ({err}); \
                                                 `fathom status` shows the queue",
                                            ),
                                        }),
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            Request::Stop { reason } => {
                let why = format!("a client asked it to stop ({reason})");
                let _ = connection.reply(&proto::ok(
                    "stop",
                    serde_json::json!({"stopping": true, "reason": why}),
                ));
                self.shutdown = Some(why);
            }
        }
    }

    /// Reply to everybody whose wait ran out. Answered, never silently dropped: a parked client is a
    /// process sitting in `read`.
    #[cfg(unix)]
    fn answer_waiters(&mut self) {
        if self.waiters.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut keep = Vec::new();
        for waiter in std::mem::take(&mut self.waiters) {
            if waiter.deadline > now {
                keep.push(waiter);
                continue;
            }
            let body = serde_json::json!({
                "settled": false,
                "passes": self.passes,
                "reason": "the wait ran out before a pass covered this request; it is either still \
                          queued or already served, and `fathom status` says which",
            });
            let _ = reply(&waiter.stream, &proto::ok("index", body));
        }
        self.waiters = keep;
    }

    /// Nothing can be parked without a socket transport.
    #[cfg(not(unix))]
    fn answer_waiters(&mut self) {}

    /// Tell every parked client that a pass has covered what it was waiting for.
    #[cfg(unix)]
    fn cover_request(&mut self) {
        let waiters = std::mem::take(&mut self.waiters);
        for waiter in waiters {
            let body = serde_json::json!({
                "settled": true,
                "passes": self.passes,
                "commits": self.last_pass.as_ref().map_or(0, |pass| pass.progress.commits),
                "units": self.last_pass.as_ref().map_or(0, |pass| pass.progress.units),
            });
            let _ = reply(&waiter.stream, &proto::ok("index", body));
        }
    }

    /// Nothing to cover without a socket transport.
    #[cfg(not(unix))]
    fn cover_request(&mut self) {}

    /// Read at most [`Options::hook_read_bytes`] from the events file, advancing the durable cursor.
    ///
    /// The cap *is* the back-pressure: a harness writing faster than the daemon may read makes the lag
    /// grow, and the lag is reported rather than hidden. Nothing is shed, because ADR-005 §2 counts a
    /// dropped event as a coverage gap and this ticket is not allowed to invent one.
    fn tail_hooks(&mut self) {
        let now = unix_ms(self.clock.now());
        let before = self.cursor.offset;
        let within = Some(self.options.hook_read_bytes);
        match self.reader.read_after_within(&mut self.cursor, now, within) {
            Ok(batch) => {
                if batch.lines > 0 {
                    self.cursor_saves += 1;
                    if let Err(err) = self.reader.save_cursor(&self.cursor) {
                        self.errors
                            .push(format!("the hook cursor could not be saved: {err}"));
                    }
                }
                let behind = self.reader.size().saturating_sub(self.cursor.offset);
                self.lag.observe(
                    self.cursor.offset.saturating_sub(before),
                    behind,
                    batch.lines,
                );
                for notice in batch.notices {
                    self.push_notice(&notice.kind, notice.detail);
                }
                if self.lag.backpressured {
                    self.push_notice(
                        "hooks",
                        format!(
                            "{} bytes of the harness stream are unread: the per-pass cap is {} and the \
                             cursor is at {}",
                            behind, self.options.hook_read_bytes, self.cursor.offset
                        ),
                    );
                } else {
                    self.clear_notice("hooks");
                }
                if !batch.rejections.is_empty() {
                    // Counted, not quarantined-and-forgotten: ADR-005 §2's argument is that a number on
                    // a status page is the difference between a coverage gap you know about and one you
                    // discover in somebody's incident.
                    self.push_notice(
                        "hooks",
                        format!(
                            "{} record(s) were refused and quarantined in this pass; `fathom hooks` \
                             has the reasons",
                            batch.rejections.len()
                        ),
                    );
                }
            }
            Err(err) => {
                self.push_notice(
                    "hooks",
                    format!("the harness stream could not be read: {err}"),
                );
            }
        }
    }

    /// Drive the state machine one step, with the slice and the wall-time budget the governor allows.
    fn drive_watch(&mut self, work: &mut dyn Work) {
        let slice = self.budget.slice();
        let budget = self
            .options
            .watch
            .poll_interval
            .min(self.options.cpu_window.max(Duration::from_millis(1)));
        let before = self.cycles;
        let mut bridge = Bridge {
            work,
            slice,
            budget,
            passes: 0,
            elapsed: Duration::ZERO,
            last: None,
            fatal: None,
        };
        self.watch.step(&mut bridge, &mut |_| {});
        let _ = before;
        self.budget.charge(bridge.elapsed);
        self.passes += bridge.passes;
        if let Some(pass) = bridge.last {
            if let Some(why) = pass.error.clone() {
                self.errors.push(why);
            }
            self.last_pass = Some(pass);
        }
        // A fatal from inside the watcher's own run is the watcher's decision to stop, and it is the
        // loop's decision to stop too: the reason is stored where `run` will find it rather than being
        // swallowed by a state machine that has no way to end a process.
        if let Some(why) = bridge.fatal {
            self.fatal = Some(why);
        }
    }

    /// Run one pass because a client asked for it.
    fn run_pass(&mut self, work: &mut dyn Work) {
        let slice = self.budget.slice();
        let budget = self
            .options
            .watch
            .poll_interval
            .min(self.options.cpu_window.max(Duration::from_millis(1)));
        let batch = self.queue.drain();
        let interest = self.interest_from_queue();
        let started = Instant::now();
        self.passes += 1;
        let outcome = work.pass(&interest, slice, budget, &mut |_| {});
        let wall = started.elapsed();
        self.budget.charge(wall);
        let complete = match &outcome {
            Ok(progress) => progress.complete,
            Err(_) => false,
        };
        match outcome {
            Ok(progress) => {
                // The pass is recorded *before* anybody is answered, because the answer a parked client
                // gets is read out of `last_pass`: answer first and the reply says the pass it was waiting
                // for did nothing, which is the kind of near-miss number that wastes a morning.
                self.last_pass = Some(Pass {
                    reason: "request",
                    progress,
                    wall,
                    complete,
                    error: None,
                    fatal: false,
                });
                if complete {
                    self.cover_request();
                    self.clear_notice("queue");
                } else {
                    // Out of slice, not out of work: the requests go back with their original arrival
                    // stamps, so a queue that has been waiting two minutes still says so.
                    self.queue.requeue(batch);
                }
            }
            Err(JobError::Fatal(reason)) => {
                self.fatal = Some(reason.clone());
                self.last_pass = Some(Pass {
                    reason: "request",
                    progress: Progress::default(),
                    wall,
                    complete: false,
                    error: Some(reason),
                    fatal: true,
                });
            }
            Err(other) => {
                self.queue.requeue(batch);
                self.push_notice(
                    "queue",
                    format!(
                        "an index request failed ({}); it is still queued and will be retried on the \
                         next pass",
                        other.message()
                    ),
                );
                self.last_pass = Some(Pass {
                    reason: "request",
                    progress: Progress::default(),
                    wall,
                    complete: false,
                    error: Some(other.message().to_owned()),
                    fatal: false,
                });
            }
        }
    }

    /// The interest to hand a queue-driven pass. The watcher's own pending batch belongs to the watcher;
    /// a client request is its own reason, and says so — which is why `signals` names the queue rather
    /// than borrowing a detection the client did not make.
    fn interest_from_queue(&self) -> Interest {
        let status = self.watch.status();
        Interest {
            reason: InterestReason::Commits,
            head: status.head.clone(),
            changes: status
                .last_change
                .clone()
                .into_iter()
                .collect::<Vec<Change>>(),
            index_touched: false,
            replaced: false,
            signals: vec!["queue"],
        }
    }

    /// The orderly exit: settle the worker, answer the parked clients, then release the lease.
    ///
    /// That order is deliberate. Releasing the lock first would let a waiting client start a second
    /// daemon while the first was still checkpointing, which is the one interleaving the lease exists to
    /// prevent.
    fn leave(&mut self, work: &mut dyn Work, why: &str) {
        if let Err(err) = work.settle() {
            self.errors.push(format!(
                "the store could not be settled on the way out: {err}"
            ));
        }
        self.cover_request();
        if let Err(err) = self.lease.release() {
            self.errors.push(format!(
                "the writer lease could not be released cleanly: {err}"
            ));
        }
        self.shutdown = Some(why.to_owned());
    }
}

/// The adapter that lets the watcher's [`watch::Job`] seam call a daemon's [`Work`].
///
/// The two traits exist for two different callers: `Job` is what a *watcher* asks for when its guards say
/// a run is due, `Work` is what a *daemon* asks for when a client did. Both end in the same index run, so
/// the adapter passes the slice through, times it, and remembers the outcome — which is the only way the
/// loop finds out that the watcher's step actually did something.
struct Bridge<'a> {
    work: &'a mut dyn Work,
    slice: usize,
    budget: Duration,
    passes: u64,
    elapsed: Duration,
    last: Option<Pass>,
    fatal: Option<String>,
}

impl watch::Job for Bridge<'_> {
    fn refresh(
        &mut self,
        interest: &Interest,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        self.passes += 1;
        let started = Instant::now();
        let outcome = self.work.pass(interest, self.slice, self.budget, progress);
        self.elapsed += started.elapsed();
        self.last = Some(match &outcome {
            Ok(report) => Pass {
                reason: "watcher",
                progress: *report,
                wall: started.elapsed(),
                complete: report.complete,
                error: None,
                fatal: false,
            },
            Err(err) => Pass {
                reason: "watcher",
                progress: Progress::default(),
                wall: started.elapsed(),
                complete: false,
                error: Some(err.message().to_owned()),
                fatal: matches!(err, JobError::Fatal(_)),
            },
        });
        if let Err(JobError::Fatal(reason)) = &outcome {
            self.fatal = Some(reason.clone());
        }
        outcome
    }
}

/// Write a parked client its answer.
#[cfg(unix)]
fn reply(stream: &std::os::unix::net::UnixStream, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut stream = stream.try_clone()?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

/// The watcher's status, as JSON — the same field names `fathom watch --json` prints, because one status
/// shape that works for both is the point of having one.
fn watch_json(status: &Status) -> serde_json::Value {
    let counters = &status.counters;
    serde_json::json!({
        "phase": status.phase.as_str(),
        "phase_summary": status.phase.summary(),
        "in_phase_ms": status.in_phase.as_millis(),
        "backend": {
            "kind": status.backend.kind.as_str(),
            "name": status.backend.name,
            "reason": status.backend.reason,
        },
        "head": status.head,
        "refs": status.refs,
        "poll_interval_ms": status.poll_interval.as_millis(),
        "cpu_share": status.cpu_share,
        "quiet_for_ms": status.quiet_for.as_millis(),
        "git_dir": status.git_dir,
        "last_change": status.last_change,
        "counters": {
            "looks": counters.looks,
            "notifications": counters.notifications,
            "poll_ticks": counters.poll_ticks,
            "ref_changes": counters.ref_changes,
            "index_runs": counters.index_runs,
            "commits_indexed": counters.commits_indexed,
            "units_computed": counters.units_computed,
            "deferrals": counters.deferrals,
            "throttles": counters.throttles,
            "stretches": counters.stretches,
            "reopens": counters.reopens,
            "backend_switches": counters.backend_switches,
            "degradations": counters.degradations,
            "errors": counters.errors,
            "consecutive_errors": counters.consecutive_errors,
        },
        "notices": status
            .notices
            .iter()
            .map(|notice| serde_json::json!({"kind": notice.kind.as_str(), "text": notice.text}))
            .collect::<Vec<_>>(),
    })
}

/// A [`Timestamp`] as unix milliseconds, the unit every stamp in the health document uses.
#[must_use]
pub fn unix_ms(stamp: Timestamp) -> i64 {
    stamp.as_unix_micros() / 1_000
}
