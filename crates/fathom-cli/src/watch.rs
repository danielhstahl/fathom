//! `fathom watch` — the state machine on screen, and the index job that feeds it (`e1.9` / `reco-949`).
//!
//! The verb owns exactly the three things the composition root owns and no policy: the clock (one read per
//! chunk, at the edge — ADR-002 §4.4), the store's location (`.fathom/index.sqlite3`, the same path
//! `fathom index` writes), and what to print. The detection, the guards and the state machine are
//! [`fathom_ingest::watch`], which cannot index because it is downhill of the indexer (ADR-002 R1) — so the
//! [`Job`] implementation below is the seam, and it is the only place the two meet.
//!
//! Two things are written to disk besides the store: the state lines on stdout (or JSON, one object per
//! change, for a script), and `.fathom/watch/status.json` — the watcher's heartbeat. The status file is the
//! cheap half of the daemon contract that `reco-kvk` will formalise: a reader (`fathom doctor` today, the
//! ambient view of `reco-8bx` tomorrow) can tell whether a watcher is alive, which backend it settled on and
//! what it is doing, without a socket, a lock or a protocol. It is written by rename, so a reader never sees
//! half a file.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fathom_core::error::Diagnosable;
use fathom_core::exit::ExitCode;
use fathom_ingest::repo::Repository;
use fathom_ingest::watch::{self, Interest, Job, JobError, Progress, Status, StopReason, Watch};

use crate::output;
use crate::verbs::{Pending, WatchArgs, failed};

/// Everything the run needs that is not the state machine: the repository path, the chunk size, and where the
/// store lives.
///
/// `pub(crate)` because a second caller arrived: `fathom ambient` (`reco-8bx`) drives this same job from its
/// own poll when no watcher is running, so "index what is new, in slices the size of a budget" is one
/// implementation and not two that can drift.
pub(crate) struct IndexJob {
    root: PathBuf,
    store_path: PathBuf,
    chunk_units: usize,
    /// When set, the run stops asking for more chunks once this passes — the watcher's own slice of CPU
    /// enforced on the *expensive* side, where it matters.
    chunk_deadline: Option<Instant>,
    /// Did this build compile the index in at all?
    store_compiled: bool,
    /// Re-analyse uncommitted paths when the index moves? Off by default, and the *watcher's* option
    /// decides — not the mere fact that `git add` touched the index file, which it does on every commit.
    index_worktree: bool,
    /// Where a caller that is not a terminal wants the run's own ticks (`reco-j7t`).
    ///
    /// `fathom watch` prints its state machine to stdout; an open frame can print nothing, so
    /// `fathom tui`'s producer installs a sink here and reads the ticks back out afterwards — the same
    /// ticks that make `fathom index` say `analysing 62 % · ~14 s left · 4 102 / 6 552 commits`, handed to
    /// a shell that may show what an indexer is doing but may not run one (ADR-002 §5.2).
    ///
    /// `None` is every other caller, which was every caller until `reco-j7t`.
    ticks: Option<fathom_analyze::index::ProgressSink>,
}

impl IndexJob {
    /// The job for one repository, with the watcher's default slice and slice budget.
    ///
    /// The deadline is the same one `fathom watch` gives itself — one poll interval of indexing per ask, so a
    /// huge backlog is taken in slices and the caller keeps its idle budget.
    pub(crate) fn for_root(root: &Path, store_path: PathBuf, chunk_units: usize) -> IndexJob {
        IndexJob {
            root: root.to_path_buf(),
            store_path,
            chunk_units,
            chunk_deadline: Some(Instant::now() + Duration::from_millis(250)),
            store_compiled: cfg!(feature = "sqlite-bundled"),
            index_worktree: false,
            ticks: None,
        }
    }

    /// Install the sink that receives the run's progress ticks (`reco-j7t`).
    ///
    /// Set per ask rather than at construction, because the sink closes over whoever is asking and the job
    /// outlives the ask: the frame's producer installs one, runs a slice, and reads what it caught.
    pub(crate) fn set_ticks(&mut self, ticks: Option<fathom_analyze::index::ProgressSink>) {
        self.ticks = ticks;
    }

    /// Re-arm the slice budget for one more ask.
    ///
    /// The deadline is absolute, so a caller that drives the job from a poll has to move it each time or the
    /// first quarter-second of the process is the only indexing it ever does. That is a bug with the shape of
    /// a silent hang, which is why it is a method rather than something each caller remembers.
    pub(crate) fn start_slice(&mut self, budget: Duration) {
        self.chunk_deadline = Some(Instant::now() + budget);
    }
}

impl Job for IndexJob {
    fn refresh(
        &mut self,
        interest: &Interest,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        if !self.store_compiled {
            // Detection works without SQLite; indexing cannot. Saying so beats pretending to have indexed.
            return Ok(Progress {
                commits: 0,
                units: 0,
                chunks: 0,
                complete: true,
            });
        }
        self.index(interest, progress)
    }
}

#[cfg(feature = "sqlite-bundled")]
impl IndexJob {
    /// Run the index in chunks, reporting after each one.
    ///
    /// The chunking is not decoration: `fathom_analyze::index::run` takes a unit budget and stops there with
    /// `interrupted: Budget`, which is a *stated partial* (ADR-004 §7.6) rather than a failure — so a watch
    /// cycle can hand the CPU back between slices, show progress while it goes, and still finish the work
    /// because the next chunk asks for the same pending rows. A run that is cancelled or that fails is a
    /// different story and is reported as such.
    fn index(
        &mut self,
        interest: &Interest,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        use fathom_analyze::index::{self, Interrupted, Scope, Worktree};
        use fathom_ingest::record::Unclassified;
        use fathom_store::spine::Store;

        // Re-discover rather than reuse: the repository handle the watcher holds may have been reopened after
        // the directory was replaced, and a job holding its own handle would be indexing a dead path.
        let repository = Repository::discover(&self.root).map_err(|err| {
            classify(&err, |code| {
                matches!(code, fathom_core::error::ErrorCode::NotARepository)
            })
        })?;
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                JobError::Fatal(format!(
                    "cannot create {} for the store: {err}",
                    parent.display()
                ))
            })?;
        }
        let mut store = Store::open(&self.store_path).map_err(|err| classify(&err, |_| false))?;

        let mut done = Progress::default();
        loop {
            let options = index::Options {
                now: Some(crate::clock::now_millis()),
                scope: Scope::NewCommits,
                worktree: if self.index_worktree && interest.index_touched {
                    Worktree::Analyse
                } else {
                    Worktree::Ignore
                },
                unit_budget: Some(self.chunk_units),
                // The ticks are the run's own counters, already phase-labelled and already measured
                // against the one denominator a run can state up front — which is why the frame's producer
                // reads these rather than counting commits from the outside (`reco-j7t`).
                progress: self.ticks.clone(),
                // The watcher writes the graph too (`e3.11`, `reco-dlp`): a daemon that kept everything else
                // current and let the module graph rot would leave `fathom architecture` and `fathom check`
                // answering from a stale lane without ever saying it. Same knobs as the CLI's own run, read
                // from the repository's `fathom.toml` each tick — which is what lets a `[layers]` edit work
                // by re-ticking rather than by restarting the daemon.
                graph: crate::verbs::graph_pass(&self.root, false, |line| {
                    tracing::debug!(target: "fathom::watch", "{line}");
                }),
                ..index::Options::head()
            };
            let report = index::run(&repository, &mut store, &Unclassified, &options)
                .map_err(|err| classify(&err, |_| false))?;
            done.commits += report.commits_new as u64;
            done.units += report.computed() as u64;
            done.chunks += 1;
            // Progress between chunks is the whole point of chunking: a watch cycle that only spoke at the
            // end would be a spinner, which is the thing this ticket has to not be.
            progress(Progress {
                complete: !matches!(report.reason, Some(Interrupted::Budget)),
                ..done
            });
            if !matches!(report.reason, Some(Interrupted::Budget)) {
                done.complete = true;
                return Ok(done);
            }
            if let Some(deadline) = self.chunk_deadline
                && Instant::now() >= deadline
            {
                // Out of slice, not out of work: the watcher will ask again, and the rows are already stored.
                done.complete = false;
                return Ok(done);
            }
            if report.computed() == 0 && report.commits_new == 0 {
                // The budget is generous enough that a chunk computed nothing yet still reports itself
                // interrupted: nothing is left, so stop rather than spin.
                done.complete = true;
                return Ok(done);
            }
            if done.chunks >= 1_000 {
                // A stated limit, not a hang. Reaching it means a repository producing work faster than the
                // watcher can consume it, which is the ref-spam case one layer down.
                tracing::warn!(
                    reason = ?interest.reason,
                    "the watch run reached its chunk limit without finishing"
                );
                done.complete = false;
                return Ok(done);
            }
        }
    }
}

/// Builds that were compiled without a SQLite backend have nothing to index into; see [`Job`] above.
#[cfg(not(feature = "sqlite-bundled"))]
impl IndexJob {
    fn index(
        &mut self,
        _interest: &Interest,
        _progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        Ok(Progress {
            complete: true,
            ..Progress::default()
        })
    }
}

/// Turn a crate error into the kind of job failure the watcher can act on: a missing repository is a reopen,
/// a corrupt store is fatal, anything else is worth a retry.
#[cfg(feature = "sqlite-bundled")]
fn classify<E: Diagnosable + std::fmt::Display, F: Fn(fathom_core::error::ErrorCode) -> bool>(
    err: &E,
    is_repository: F,
) -> JobError {
    let code = err.code();
    let text = err.to_string();
    if is_repository(code) {
        return JobError::Repository(text);
    }
    if code == fathom_core::error::ErrorCode::StoreCorrupt {
        return JobError::Fatal(format!(
            "{text} (rebuild is safe: `fathom index` rebuilds the store)"
        ));
    }
    JobError::Transient(text)
}

/// The watcher's job, which is either this process's own index or a daemon's pass asked over a socket.
///
/// An enum rather than a `Box<dyn Job>`: [`Job`] is `fathom-ingest`'s trait, and `impl Job for
/// Box<dyn Job>` would be an impl of a foreign trait for a foreign type — the orphan rule says no, and
/// the alternative (making the daemon's client live in `fathom-ingest`) is the layering violation this
/// split exists to avoid (`e6.3`). Two variants is the honest shape anyway: there are exactly two ways
/// for a `fathom watch` to have its work done.
enum JobKind {
    /// This process indexes, through [`IndexJob`]. The behaviour of every previous release.
    Direct(IndexJob),
    /// A daemon indexes, and this process asks (`fathom_ingest::daemon`).
    Asked(crate::daemon::Asked),
}

impl Job for JobKind {
    fn refresh(
        &mut self,
        interest: &Interest,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        match self {
            JobKind::Direct(job) => job.refresh(interest, progress),
            JobKind::Asked(asked) => asked.refresh(interest, progress),
        }
    }
}
/// What arrives at stdout, once per state change rather than once per tick.
struct Printer {
    json: bool,
    quiet: bool,
    /// Who is doing the indexing this watcher is reporting on — this process, or a daemon asked over its
    /// socket (`e6.3`). Printed, because the difference is a fact about the repository.
    indexer: &'static str,
    /// The last thing printed, so a state that is merely still true is not printed forty times a second.
    last: Cell<Option<(String, String, usize)>>,
}

impl Printer {
    fn emit(&self, status: &Status) {
        // The watcher emits on a beat as well as on a change — that is what keeps `status.json` and a TUI's
        // "quiet for 4 s" live while nothing happens — and this is the layer that decides a repeated truth is
        // not news. `--json`'s promise is one object per *change*, so the beat is filtered here rather than
        // arriving as a stream of identical objects for a script to throw away.
        let notices = status
            .notices
            .iter()
            .map(|notice| notice.text.clone())
            .collect::<Vec<_>>()
            .join("; ");
        let key = (
            status.phase.as_str().to_owned(),
            status
                .last_change
                .as_ref()
                .map(|change| change.summary())
                .unwrap_or_default(),
            status.notices.len(),
        );
        if self.last.replace(Some(key.clone())) == Some(key) {
            return;
        }
        if self.json {
            output::line(format_args!(
                "{}",
                crate::watch::status_json_indexed_by(status, None, self.indexer)
            ));
            return;
        }
        if self.quiet {
            return;
        }
        let mut line = status.line();
        if !notices.is_empty() {
            line.push_str(&format!(" · {notices}"));
        }
        output::line(format_args!("{line}"));
    }
}

/// Run `fathom watch`.
pub fn run(args: &WatchArgs) -> eyre::Result<ExitCode> {
    let backend = match fathom_ingest::watch::BackendChoice::parse(&args.backend) {
        Some(choice) => choice,
        None => {
            return Err(Pending::usage(
                "`--backend` takes `auto`, `notify` or `poll`",
                format!("`{}` is none of those", args.backend),
            )
            .into());
        }
    };
    let root = Path::new(&args.root);
    let repository = Repository::discover(root).map_err(failed)?;
    let store_path = store_path(root);

    let mut options = watch::Options {
        backend,
        index_worktree: args.worktree,
        initial_refresh: true,
        max_cycles: args.max_cycles,
        deadline: args.deadline.map(Duration::from_secs),
        ..watch::Options::default()
    };
    if let Some(millis) = args.interval_ms {
        options.poll_interval = Duration::from_millis(millis);
        options.poll_interval_max = options.poll_interval.max(options.poll_interval_max);
    }
    if let Some(millis) = args.debounce_ms {
        options.debounce = Duration::from_millis(millis);
    }
    if let Some(millis) = args.heartbeat_ms {
        options.heartbeat = Duration::from_millis(millis);
    }

    let mut watch = Watch::with_repository(root, repository, options.clone()).map_err(failed)?;
    if !args.json && !args.quiet {
        output::line(format_args!(
            "watching {} · {}",
            watch.repository().git_dir().display(),
            watch.backend().summary()
        ));
        if let Some(reason) = watch.backend().reason.clone() {
            output::line(format_args!("  {reason}"));
        }
        if !options.index_worktree {
            output::line(format_args!(
                "  uncommitted work is not re-indexed while watching; pass --worktree for it"
            ));
        }
    }

    // One process per repository does the indexing (`e6.3`): when a daemon is serving this
    // repository, this watcher asks it for each pass instead of doing a second copy of the same
    // work in a second process. Everything above is unchanged either way, which is the point of the
    // [`Job`] seam — the debounce, the guards, the printed states and the status file are the same,
    // and only the thing that reads commits differs. A daemon that answers and then refuses is a
    // `Transient` job failure, so the watcher keeps running and asks again rather than giving up.
    let daemon = crate::daemon::health(root, None);
    if let Some(health) = &daemon
        && !args.json
        && !args.quiet
    {
        output::line(format_args!(
            "  {}",
            crate::daemon::delegation_note(health, &crate::daemon::endpoint(root, None))
        ));
    }
    let mut job = if daemon.is_some() {
        JobKind::Asked(crate::daemon::Asked::new(
            root,
            "fathom watch",
            crate::daemon::ASK_WAIT,
        ))
    } else {
        JobKind::Direct(IndexJob {
            root: root.to_path_buf(),
            store_path: store_path.clone(),
            chunk_units: args.chunk_units,
            // One cycle of watch work gets at most one poll interval of indexing, so a huge backlog
            // is taken in slices the size of the watcher's own budget rather than in one long block.
            chunk_deadline: Some(
                Instant::now() + options.poll_interval.max(Duration::from_millis(250)),
            ),
            store_compiled: cfg!(feature = "sqlite-bundled"),
            index_worktree: options.index_worktree,
            // `fathom watch` prints its own state machine to stdout and wants no sink: the ticks are for a
            // caller that cannot print anything, which is the frame, not the watcher (`reco-j7t`).
            ticks: None,
        })
    };
    let printer = Printer {
        json: args.json,
        quiet: args.quiet,
        indexer: if daemon.is_some() {
            crate::watch::DAEMON_INDEXER
        } else {
            crate::watch::LOCAL_INDEXER
        },
        last: Cell::new(None),
    };
    // `--once` stops the moment the first run reports itself done: it is `fathom index` with the watcher's
    // detection budget, which is what a pre-commit hook or a CI check wants.
    let finished = Cell::new(false);
    let status_file = if args.no_status_file {
        None
    } else {
        Some(status_path(&store_path))
    };
    let reason = watch.run(
        &mut job,
        &|| {
            if args.once { finished.get() } else { false }
        },
        &mut |status| {
            printer.emit(status);
            if let Some(path) = &status_file {
                write_status(path, status);
            }
            if args.once && status.phase == watch::Phase::Idle {
                finished.set(true);
            }
        },
    );
    if !args.json && !args.quiet {
        output::line(format_args!("{}", reason.summary()));
    }
    if args.json {
        output::line(format_args!(
            "{}",
            status_json_indexed_by(&watch.status(), Some(reason.summary()), printer.indexer,)
        ));
    }
    match reason {
        StopReason::Fatal(reason) => Err(Pending::new(
            ExitCode::StoreCorrupt,
            reason,
            Some("`fathom index` rebuilds the store; it is derived data".to_owned()),
        )
        .into()),
        _ => Ok(ExitCode::Ok),
    }
}

/// Where the index lives: `.fathom/index.sqlite3` under the repository, the same path `fathom index` uses.
#[must_use]
pub fn store_path(root: &Path) -> PathBuf {
    #[cfg(feature = "sqlite-bundled")]
    {
        fathom_analyze::index::default_store_path(root)
    }
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        root.join(".fathom").join("index.sqlite3")
    }
}

/// Where the heartbeat goes, next to the store and in fathom's own directory (ADR-004 §7.7: `.fathom/` is
/// fathom's, which is why writing there needs no new permission).
#[must_use]
pub fn status_path(store_path: &Path) -> PathBuf {
    store_path.parent().map_or_else(
        || PathBuf::from("watch-status.json"),
        |dir| dir.join("watch").join("status.json"),
    )
}

/// The heartbeat, as data. The field names are the CLI's promise, assembled here rather than derived from
/// [`Status`]'s serde shape, for the same reason `fathom index --json`'s shape is assembled in `verbs.rs`:
/// a renamed internal field must not silently rename what `reco-8bx` reads.
pub fn status_json(status: &Status, stop: Option<String>) -> serde_json::Value {
    status_json_indexed_by(status, stop, LOCAL_INDEXER)
}

/// This process did the work (`e6.3`): the watcher's own index run, the store opened here, the cache filled
/// here. The other value is [`DAEMON_INDEXER`].
pub const LOCAL_INDEXER: &str = "this process";
/// The work was asked of the daemon over its socket, and this process only waited for the answer (`e6.3`).
/// A machine reading `fathom watch --json` cannot otherwise tell the two apart: the counters look identical,
/// and which process held the writer lease is exactly the fact ADR-004 §7.1 cares about.
pub const DAEMON_INDEXER: &str = "daemon";

/// The watcher's state as one JSON document, with the one fact only the verb knows stamped on it: who
/// indexed. Additive on purpose — everything above [`status_json`]'s warning is unchanged for `reco-8bx`.
#[must_use]
pub fn status_json_indexed_by(
    status: &Status,
    stop: Option<String>,
    indexer: &str,
) -> serde_json::Value {
    let notices: Vec<serde_json::Value> = status
        .notices
        .iter()
        .map(|notice| {
            serde_json::json!({
                "kind": notice.kind.as_str(),
                "text": notice.text,
            })
        })
        .collect();
    let counters = &status.counters;
    let mut value = serde_json::json!({
        "pid": std::process::id(),
        "watcher": format!("fathom {}", crate::features::VERSION),
        "indexer": indexer,
        "git_dir": status.git_dir,
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
        "notices": notices,
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
    });
    if let Some(object) = value.as_object_mut() {
        // The wall clock belongs to the edge, and the edge may not be compiled in (a binary without a store
        // has no clock module); an absent stamp is stated, never faked.
        #[cfg(feature = "sqlite-bundled")]
        object.insert(
            "stamp_ms".to_owned(),
            serde_json::json!(crate::clock::now_millis()),
        );
        if let Some(stop) = stop {
            object.insert("stopped".to_owned(), serde_json::json!(stop));
        }
    }
    value
}

/// Write the heartbeat by rename, so a reader never observes a half-written file.
///
/// `pub(crate)` because the daemon has a heartbeat to write too (`e6.3`): a reader that can ask a daemon
/// over a socket can also read this file when there is no socket, and both writers must leave the same shape
/// or `fathom doctor` is comparing two different documents.
pub(crate) fn write_status(path: &Path, status: &Status) {
    let value = status_json(status, None);
    let rendered = match serde_json::to_string(&value) {
        Ok(text) => text,
        Err(_) => return,
    };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, rendered).is_ok() {
        // A failed rename leaves the previous heartbeat in place, which is better than no heartbeat: the
        // reader will see a stale one and say so, rather than seeing nothing and guessing.
        let _ = std::fs::rename(&temp, path);
    }
}

/// Read a watcher's heartbeat, for `fathom doctor`. `None` means no file, which is a different sentence from
/// "a watcher that died an hour ago" and is reported as one.
#[must_use]
pub fn read_status(store_path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(status_path(store_path)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The line `fathom doctor` prints about the watcher, including the backend a repository *would* get, which
/// is the part a user can act on before starting a watcher.
#[must_use]
pub fn doctor_lines(root: &Path, interval: Duration) -> Vec<String> {
    let mut out = Vec::new();
    if Repository::discover(root).is_err() {
        // Doctor answers about the whole build even outside a repository, so the watch section says what it
        // cannot answer instead of inventing a backend for a directory that holds no refs.
        out.push(format!(
            "watch: {} is not a git repository, so there is nothing to watch here",
            root.display()
        ));
        return out;
    }
    let probe =
        fathom_ingest::watch::probe(fathom_ingest::watch::BackendChoice::Auto, root, interval);
    out.push(format!(
        "watch: backend here — {} (requested {}, filesystem {})",
        probe.backend.summary(),
        probe.requested.as_str(),
        probe
            .filesystem
            .clone()
            .unwrap_or_else(|| "uncharacterised".to_owned())
    ));
    match read_status(&store_path(root)) {
        Some(status) => {
            let pid = status.get("pid").and_then(serde_json::Value::as_u64);
            let alive = process_alive(pid);
            let phase = status
                .get("phase_summary")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown phase");
            // The *watcher's* backend, not the probe's prediction: this is the line that shows a fallback
            // that already happened, reason included, rather than one that merely might.
            let running = status
                .pointer("/backend/name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let why = status
                .pointer("/backend/reason")
                .and_then(serde_json::Value::as_str)
                .map(|reason| format!(" — {reason}"))
                .unwrap_or_default();
            out.push(format!(
                "watch: {} (pid {}, {phase}) · backend {running}{why}",
                if alive {
                    "a watcher is running"
                } else {
                    "a watcher stopped"
                },
                pid.unwrap_or(0),
            ));
        }
        None => out.push(format!(
            "watch: no watcher running (the heartbeat would be {})",
            status_path(&store_path(root)).display()
        )),
    }
    out
}

/// The same, as data, for `fathom doctor --json`.
#[must_use]
pub fn doctor_json(root: &Path, interval: Duration) -> serde_json::Value {
    if Repository::discover(root).is_err() {
        return serde_json::json!({ "repository": false });
    }
    let probe =
        fathom_ingest::watch::probe(fathom_ingest::watch::BackendChoice::Auto, root, interval);
    serde_json::json!({
        "repository": true,
        "compiled_in": true,
        "backend": {
            "kind": probe.backend.kind.as_str(),
            "name": probe.backend.name,
            "reason": probe.backend.reason,
            "requested": probe.requested.as_str(),
            "notifications_trusted": probe.notifications_trusted,
        },
        "filesystem": probe.filesystem,
        "running": read_status(&store_path(root)).map(|status| {
            serde_json::json!({
                "pid": status.get("pid"),
                "phase": status.get("phase"),
                "alive": process_alive(status.get("pid").and_then(serde_json::Value::as_u64)),
                "stamp_ms": status.get("stamp_ms"),
                "backend": status.get("backend"),
            })
        }),
    })
}

/// Is the watcher's process still there?
///
/// `getppid`-style liveness without a new dependency: on Linux the procfs entry is the answer, and elsewhere
/// an honest `false` when no heartbeat was refreshed recently is better than a guess. A watcher that was
/// `kill -9`d leaves its heartbeat behind, which is exactly the case this has to catch: a status file that
/// promises a live watcher and no process is the "stale" state, said out loud.
#[must_use]
fn process_alive(pid: Option<u64>) -> bool {
    let Some(pid) = pid else {
        return false;
    };
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        false
    }
}
