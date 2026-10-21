//! `fathom daemon` and `fathom status` — the one process per repository, and the way to ask it what it
//! knows (`e6.3` / `reco-kvk`).
//!
//! # Why the loop is not in this file
//!
//! The loop — lease, endpoint, queue, budget, the six steps of one pass — lives in
//! [`fathom_ingest::daemon`]. What lives here is the half that loop cannot contain: the [`Worker`] that
//! indexes (which needs `fathom-analyze` and the store, both uphill of `fathom-ingest` — ADR-002 R1), the
//! process management around `--foreground`, the signals, and the printing. The same split as
//! [`crate::watch`], for the same reason, with the same payoff: the loop is tested in `fathom-ingest`
//! against a scripted worker, and the real one is assembled here, where reading the clock and the config is
//! allowed (ADR-002 §4.4, §4.5).
//!
//! # The three ways a `fathom` process meets the daemon
//!
//! * it **is** one: `fathom daemon start`, which takes the writer lease and loops;
//! * it **asks** one: `fathom status`, and — when a daemon is answering — `fathom index`, `fathom watch`
//!   and `fathom ambient`, which hand their indexing over instead of doing a second copy of it. That is
//!   §1's acceptance criterion read literally: three shells and a CLI, one pass, counted in the daemon's
//!   own counters rather than assumed;
//! * it **never met one**: no daemon, no socket, nothing changed anywhere. Every verb behaves exactly as it
//!   did before this ticket, which is the property that makes the feature safe to turn on by default.
//!
//! # Foreground, and why it is a flag rather than a binary
//!
//! A supervisor — systemd, a container's `init`, a CI step — wants the daemon in its *foreground* so it can
//! supervise it at all, and wants its stdout. That is `--foreground`, and it is what ADR-004 §7.1 means by
//! "the daemon is not a separate installation unit: `fathom daemon --foreground` is what a supervisor runs".
//! Plain `fathom daemon start` is the human path: the process re-executes itself detached (new session, so a
//! terminal's Ctrl-C cannot reach it later), and the parent waits until the child's *socket answers* before
//! saying anything is up. Waiting for a reply rather than for a `waitpid` is the difference between "the
//! child was spawned" and "the daemon is serving", and only the second is worth printing.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fathom_core::exit::ExitCode;
use fathom_ingest::daemon::{self, Daemon, Options as DaemonOptions, proto, socket};
use fathom_ingest::repo::Repository;
use fathom_ingest::watch::{Interest, Job, JobError, Progress};
use fathom_store::lease::{self, Lease, StaleAfter, State};
use fathom_store::spine::Store;
use serde_json::{Value, json};

use crate::clock::now_millis;
use crate::output;
use crate::state::Finding;
use crate::verbs::{DaemonStartArgs, DaemonStopArgs, Pending, StatusArgs, failed};

/// How long a client asks to be held when it wants the pass to have *happened* before it prints.
///
/// Long enough that a normal pass is waited for — a second terminal asking for a pass should not be answered
/// with a stale view — and short enough that a daemon wedged in a slow pass produces a report with a note
/// instead of a hung command. The daemon caps any wait at [`proto::MAX_WAIT_MS`] whatever a client asks;
/// this is the smaller, politer number.
const ASK_MS: u64 = 20_000;

/// How long a liveness probe waits. A daemon that takes longer than this to answer `status` is not a daemon
/// that should keep a human waiting for a status line; the answer becomes "not serving", which is honest
/// about the window even when the daemon comes back a moment later.
const PROBE: Duration = Duration::from_millis(1_000);

/// How long a *handoff* probe waits, which is a different question from [`PROBE`] and deserves its own
/// number.
///
/// `fathom status` is answering a human: a daemon busy in a long pass should not make a status line slow, so
/// it gives up after a second and prints what it could read instead. `fathom index`, `fathom watch` and
/// `fathom ambient` are asking something else — *should this process do the work, or is somebody already
/// doing it* — and that branch is expensive in the wrong direction: at a one-second deadline a daemon that is
/// merely busy and a daemon that is absent get the same answer, and the wrong answer is a second process
/// indexing the same repository for nobody's benefit. So the handoff waits longer, and still far short of the
/// [`ASK_MS`] it is about to spend if the daemon really is there.
const DEFER_PROBE: Duration = Duration::from_secs(5);

/// How long `fathom daemon stop` waits for the process to actually release the lease.
const STOP_WAIT: Duration = Duration::from_secs(10);

/// How long `fathom daemon start` waits for a detached child to become ready.
const READY_WAIT: Duration = Duration::from_secs(20);

/// How often the parent re-probes a starting daemon, and how often `stop` re-reads the lease. Fast enough to
/// be under a human's blink, slow enough that the poll is not the thing showing up in a profile.
const POLL: Duration = Duration::from_millis(50);

// ── the client half ─────────────────────────────────────────────────────────────────────────────

/// The endpoint a client would use, resolved from the repository and the environment.
///
/// One function, so `fathom status`, `fathom index`, `fathom doctor` and the ambient pane cannot disagree
/// about where the daemon is. Two of them saying "running" and "no daemon" about the same repository is the
/// kind of inconsistency this repository treats as a bug rather than a quirk.
#[must_use]
pub fn endpoint(root: &Path, explicit: Option<&Path>) -> socket::Endpoint {
    socket::resolve(root, explicit, &socket::Env::from_process())
}

/// What asking the daemon produced — four outcomes, because "it refused", "nobody was there" and "something
/// answered in a language I do not speak" need different sentences and different remedies.
enum Answer {
    /// A success, with the reply's `data`.
    Done(Value),
    /// The daemon answered and said no. It knows something the client does not, so its sentence is printed
    /// verbatim rather than paraphrased.
    Refused { why: String, message: String },
    /// Nothing was listening, or this platform has no transport. The normal state in a repository with no
    /// daemon, and never a failure by itself.
    Absent,
    /// Something answered that this build cannot read: a newer daemon, an older client, or a path somebody
    /// else is using. Stated loudly, because a silently-ignored protocol mismatch is a support week.
    Broken(String),
}

impl Answer {
    /// The payload, if there was one. Callers that only care "is a daemon serving?" use this.
    fn ok(self) -> Option<Value> {
        match self {
            Answer::Done(data) => Some(data),
            Answer::Refused { .. } | Answer::Absent | Answer::Broken(_) => None,
        }
    }
}

/// Send one request over one connection and classify what came back.
fn ask(root: &Path, socket: Option<&Path>, request: &proto::Request, timeout: Duration) -> Answer {
    let endpoint = endpoint(root, socket);
    match proto::dial(&endpoint.path, request, timeout) {
        Ok(proto::Reply::Ok(data)) => Answer::Done(data),
        Ok(proto::Reply::Refused { why, message }) => Answer::Refused { why, message },
        Ok(proto::Reply::Malformed(why)) => Answer::Broken(why),
        // `Unavailable` is the interesting one of the four dial failures: it is the answer "there is no
        // daemon", which is a fact about the repository rather than about the transport.
        Err(proto::Dial::Unavailable(_)) => Answer::Absent,
        Err(err) => Answer::Broken(err.to_string()),
    }
}

/// The daemon's health document, if a daemon is serving this repository.
///
/// `None` means "no daemon", and callers fall back to what they did before this verb existed. A daemon that
/// refuses or talks nonsense is also `None` — a broken daemon must never be able to *stop* a repository from
/// being indexed — but it is [`status`]'s job to say so, and `status` is a diagnostic.
#[must_use]
pub fn health(root: &Path, socket: Option<&Path>) -> Option<Value> {
    ask(root, socket, &proto::Request::Status, DEFER_PROBE).ok()
}

/// `fathom index`'s deferral: ask the daemon for the pass, and report what its pass did.
///
/// `None` means "no daemon — index it yourself", which is the answer every caller was already
/// handling. A *refusal* comes back folded into the reply as an unsettled pass rather than as an
/// error: a full queue means a pass is running or about to be, which is the same news the caller
/// would have got by doing the work itself, and cheaper to say.
pub fn ask_for_pass(root: &Path, origin: &str) -> Option<Value> {
    match request_pass(root, None, origin, ASK_MS) {
        Ok(Some(reply)) => Some(reply),
        Ok(None) => None,
        Err(why) => Some(json!({
            "settled": false,
            "refused": true,
            "reason": why,
        })),
    }
}

/// The same wait, spelled for a caller outside this module: a watcher asking on someone's behalf
/// holds for the same time the CLI's own verbs hold for.
pub(crate) const ASK_WAIT: Duration = Duration::from_millis(20_000);
/// Is a daemon serving this repository? The whole deferral test, in one call a caller can put at the top of
/// its own function.
#[must_use]
pub fn serving(root: &Path) -> bool {
    health(root, None).is_some()
}

/// Ask the daemon to do a pass and wait for it, as a client.
///
/// `Ok(Some(reply))` is the daemon's answer (a pass, or a coalescing note), `Ok(None)` is "no daemon, do it
/// yourself", and `Err` is a daemon that said no — a full queue, most often, which is the back-pressure of
/// §4 working rather than failing.
fn request_pass(
    root: &Path,
    socket: Option<&Path>,
    origin: &str,
    wait_ms: u64,
) -> Result<Option<Value>, String> {
    match ask(
        root,
        socket,
        &proto::Request::Index {
            origin: origin.to_owned(),
            wait_ms,
        },
        Duration::from_millis(wait_ms + 2_000),
    ) {
        Answer::Done(data) => Ok(Some(data)),
        Answer::Refused { why, message } => Err(format!("{why}: {message}")),
        Answer::Absent => Ok(None),
        Answer::Broken(why) => Err(why),
    }
}

/// The one-sentence version of a daemon's `index` reply, for a line a human reads.
#[must_use]
pub fn describe_pass(reply: &Value) -> String {
    let settled = reply.get("settled").and_then(Value::as_bool);
    let commits = reply.get("commits").and_then(Value::as_u64).unwrap_or(0);
    let units = reply.get("units").and_then(Value::as_u64).unwrap_or(0);
    match settled {
        Some(true) => format!("the daemon's pass read {commits} commit(s), {units} unit(s)"),
        Some(false) => reply
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("the daemon did not cover this request before the wait ran out")
            .to_owned(),
        None => if reply
            .get("coalesced")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "an identical request was already waiting, and its pass will cover this one"
        } else {
            "the daemon queued the request without waiting"
        }
        .to_owned(),
    }
}

// ── `fathom daemon start` ───────────────────────────────────────────────────────────────────────

/// `fathom daemon start`.
///
/// Two processes are involved in the background case and one in the foreground case, and the split happens
/// before anything is opened: a lease taken by the parent and inherited by the child would be a lease the
/// parent still holds while it prints "started", which is a sentence about a lock rather than a fact.
pub fn start(args: &DaemonStartArgs) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    let repository = Repository::discover(root).map_err(failed)?;
    if args.foreground {
        return foreground(args, root, repository);
    }
    respawn(args, root)
}

/// The options the daemon runs on: the flags given, over the library's defaults.
///
/// The watcher's own knobs are passed through unchanged, which is the whole point of the daemon *being* a
/// watcher: `fathom daemon --debounce-ms 800` and `fathom watch --debounce-ms 800` must mean one thing.
///
/// Worth saying plainly: these flags are *not* layered over `fathom.toml`, and neither are `fathom watch`'s.
/// The schema documents `watch.cpu_share_pct`, and nothing reads it yet — the same gap the watcher has, noted
/// here rather than left for the next person to discover by being surprised about which of the two files they
/// were supposed to edit.
fn options(args: &DaemonStartArgs) -> DaemonOptions {
    let mut options = DaemonOptions::for_root(Path::new(&args.root));
    options.socket = args.socket.as_ref().map(PathBuf::from);
    if let Some(choice) = fathom_ingest::watch::BackendChoice::parse(&args.backend) {
        options.watch.backend = choice;
    }
    if let Some(millis) = args.interval_ms {
        options.watch.poll_interval = Duration::from_millis(millis);
        options.watch.poll_interval_max = options
            .watch
            .poll_interval_max
            .max(options.watch.poll_interval);
    }
    if let Some(millis) = args.debounce_ms {
        options.watch.debounce = Duration::from_millis(millis);
    }
    if let Some(millis) = args.heartbeat_ms {
        options.watch.heartbeat = Duration::from_millis(millis);
    }
    options.watch.index_worktree = args.worktree;
    // `--cpu-share-pct` is a percentage because that is the unit the acceptance criterion is written in
    // ("under 25 % of one core"), and the conversion happens once, here, rather than in the budget, which
    // would then have to know which unit a caller meant.
    options.cpu_share = f64::from(args.cpu_share_pct.max(1)) / 100.0;
    options.slice = args.slice.max(1);
    options.yield_slice = args.yield_slice.max(1);
    options.queue_depth = args.queue_depth.max(1);
    options.hook_read_bytes = args.hook_bytes.max(1);
    options.require_socket = args.require_socket;
    options.deadline = args.deadline.map(Duration::from_secs);
    options.max_cycles = args.max_cycles;
    options.build = format!("fathom {}", crate::features::VERSION);
    options.supervisor = args.supervisor.clone().filter(|text| !text.is_empty());
    options
}

/// The foreground daemon: this process, this thread, this lease.
///
/// `Repository::discover` was already run by the caller for its error, and the result is dropped on purpose:
/// the daemon's watcher re-opens and re-opens-again on its own (a repository that is moved and put back is a
/// state the watcher recovers from by design), so a handle held here would only be a handle that goes stale.
fn foreground(
    args: &DaemonStartArgs,
    root: &Path,
    repository: Repository,
) -> eyre::Result<ExitCode> {
    drop(repository);
    crate::sandbox::ensure_state_dir(root)?;
    if args.detach {
        detach_session();
    }
    let signals = Signals::install()?;
    let options = options(args);
    // Read before the options move into the daemon, which takes them by value.
    let beat_every = options.watch.heartbeat;
    let mut worker = Worker::new(root);
    let mut daemon =
        Daemon::open(root, options, Box::new(crate::clock::SystemClock)).map_err(failed)?;
    let status_file = if args.no_status_file {
        None
    } else {
        Some(crate::watch::status_path(&crate::watch::store_path(root)))
    };
    let say = |text: String| {
        if !args.json && !args.quiet {
            output::line(format_args!("{text}"));
        }
    };

    if daemon
        .health()
        .pointer("/socket/listening")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        say(format!(
            "fathom daemon: pid {}, writer epoch {} ({}), endpoint {}, budget {}% of one core",
            std::process::id(),
            daemon.lease().epoch(),
            daemon
                .health()
                .pointer("/lease/platform")
                .and_then(Value::as_str)
                .unwrap_or("unknown lock"),
            daemon.endpoint().path.display(),
            (options_cpu(&daemon) * 100.0).round(),
        ));
    } else {
        say(format!(
            "fathom daemon: pid {}, writer epoch {} — no IPC endpoint, so `fathom status` will read the \
             lease and the heartbeat file instead",
            std::process::id(),
            daemon.lease().epoch()
        ));
    }
    for notice in daemon.notices() {
        say(format!("  {}: {}", notice.kind, notice.text));
    }
    // A writer that came back from a crash says so here, because the alternative is a reviewer concluding
    // the index silently lied. This is ADR-004 §9.2's resumption, and it belongs to the daemon because the
    // daemon is the process that has a *predecessor*: a one-shot `fathom index` has nothing to resume from.
    if let Some(reason) = worker.recover() {
        say(format!("resumed: {reason}"));
    }

    let mut printed = false;
    // The beat's own clock, kept here rather than asked of the daemon: `run` hands the beat callback a shared
    // borrow (the health document is read, not written), so the cadence is the caller's to remember. The
    // interval is the watcher's heartbeat, which is the same number `fathom watch` uses for the same file.
    let mut last_beat = Instant::now();
    let stop = daemon.run(&mut worker, &|| signals.stop(), &mut |daemon| {
        if args.json && !printed && daemon.last_pass().is_some() {
            // One object, as soon as there is a pass to describe: a script that asked for JSON wants
            // the document, not a stream of heartbeats for it to discard.
            output::line(format_args!("{}", compact(&daemon.health())));
            printed = true;
        }
        if last_beat.elapsed() >= beat_every {
            last_beat = Instant::now();
            let status = daemon.watch_status();
            if let Some(path) = &status_file {
                crate::watch::write_status(path, &status);
            }
            if !args.json && !args.quiet && daemon.last_pass().is_some() {
                // The watcher's own line, with the rule its `Printer` learned for the same reason: a
                // truth that is merely still true is not news, and a daemon that prints is unreadable.
                output::line(format_args!("daemon: {}", status.line()));
            }
        }
    });
    if args.json && !printed {
        output::line(format_args!("{}", compact(&daemon.health())));
    }
    say(format!("daemon: {}", stop.summary()));
    Ok(match stop {
        daemon::Stop::Fatal(_) => ExitCode::StoreCorrupt,
        // Ctrl-C, SIGTERM and SIGHUP are all requests to stop, and ADR-002 §7.3 already has the code for
        // "the user, not a bug, ended this": 6, never 1, so a script can tell the two apart.
        _ if signals.hangup() => ExitCode::Cancelled,
        _ => ExitCode::Ok,
    })
}

/// The daemon's configured share, for the start-up line, read back from its own health document so the
/// number printed is the number the budget was built with rather than a re-derivation of the flag.
fn options_cpu(daemon: &Daemon) -> f64 {
    daemon
        .health()
        .pointer("/cpu/budget_share")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

/// Render a health document compactly. `--json` streams one line per object, and the daemon's document has
/// nested objects that stay nested but must not add newlines.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned())
}

/// Start the daemon in the background by re-executing this command with `--foreground`.
///
/// Re-exec rather than `fork`: a forked child would inherit this process's descriptors, its clock reading
/// and whatever else `main` happens to have done, and would then be a daemon whose behaviour depends on the
/// accident of what its parent did first. A fresh process running `fathom daemon start --foreground` is the
/// same code path a supervisor would use — which is the point of having the flag at all.
fn respawn(args: &DaemonStartArgs, root: &Path) -> eyre::Result<ExitCode> {
    let log = log_path(root);
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!(
                    "cannot create {} for the daemon's log: {err}",
                    dir.display()
                ),
                None,
            )
        })?;
    }
    let open = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("cannot open {}: {err}", log.display()),
                Some(
                    "`fathom daemon start --foreground` keeps the log on this terminal".to_owned(),
                ),
            )
        })?;
    let err = open.try_clone().map_err(|err| {
        Pending::new(
            ExitCode::Error,
            format!("cannot open {}: {err}", log.display()),
            None,
        )
    })?;
    let mut command = std::process::Command::new(std::env::current_exe().map_err(|err| {
        Pending::new(
            ExitCode::Error,
            format!("this executable could not be found to re-execute itself: {err}"),
            None,
        )
    })?);
    command
        .arg("daemon")
        .arg("start")
        .arg("--foreground")
        .arg("--detach")
        .arg("--root")
        .arg(root)
        .arg("--cpu-share-pct")
        .arg(args.cpu_share_pct.to_string())
        .arg("--slice")
        .arg(args.slice.to_string())
        .arg("--yield-slice")
        .arg(args.yield_slice.to_string())
        .arg("--queue-depth")
        .arg(args.queue_depth.to_string())
        .arg("--hook-bytes")
        .arg(args.hook_bytes.to_string())
        .arg("--backend")
        .arg(&args.backend)
        .current_dir(root);
    if let Some(value) = args.socket.as_deref().map(Path::new) {
        command.arg("--socket").arg(value);
    }
    if let Some(value) = args.supervisor.as_deref() {
        command.arg("--supervisor").arg(value);
    }
    for (flag, value) in [
        ("--interval-ms", args.interval_ms),
        ("--debounce-ms", args.debounce_ms),
        ("--heartbeat-ms", args.heartbeat_ms),
        ("--deadline", args.deadline),
        ("--max-cycles", args.max_cycles),
    ] {
        if let Some(value) = value {
            command.arg(flag).arg(value.to_string());
        }
    }
    for (flag, on) in [
        ("--require-socket", args.require_socket),
        ("--worktree", args.worktree),
        ("--no-status-file", args.no_status_file),
        ("--json", args.json),
        ("--quiet", args.quiet),
    ] {
        if on {
            command.arg(flag);
        }
    }
    let child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(open))
        .stderr(std::process::Stdio::from(err))
        .spawn()
        .map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("could not start the daemon: {err}"),
                Some("`fathom daemon start --foreground` runs it in this process".to_owned()),
            )
        })?;
    let pid = child.id();
    // Released, not waited for: supervising a daemon from the command that was supposed to detach it would
    // be a daemon that dies with its own `start`.
    drop(child);

    let socket = endpoint(root, args.socket.as_deref().map(Path::new));
    let started = Instant::now();
    let health = loop {
        if let Some(health) = health(root, args.socket.as_deref().map(Path::new)) {
            break Some(health);
        }
        if started.elapsed() >= READY_WAIT {
            break None;
        }
        std::thread::sleep(POLL);
    };
    let Some(health) = health else {
        return Err(Pending::new(
            ExitCode::Error,
            format!(
                "the daemon started at pid {pid} did not answer on {} within {} s",
                socket.path.display(),
                READY_WAIT.as_secs()
            ),
            Some(format!(
                "read {}; `fathom daemon start --foreground` prints the failure here instead",
                log.display()
            )),
        )
        .into());
    };
    if args.json {
        let mut report = json!({
            "started": true,
            "spawned_pid": pid,
            "log": log.display().to_string(),
        });
        if let (Some(object), Some(data)) = (report.as_object_mut(), health.as_object()) {
            for (key, value) in data {
                object.insert(key.clone(), value.clone());
            }
        }
        print(args.json, &report, "");
    } else {
        output::line(format_args!(
            "daemon: ready — pid {}, endpoint {}, log {}",
            health
                .get("pid")
                .and_then(Value::as_u64)
                .unwrap_or(u64::from(pid)),
            health
                .pointer("/socket/path")
                .and_then(Value::as_str)
                .unwrap_or(&socket.path.display().to_string()),
            log.display()
        ));
    }
    Ok(ExitCode::Ok)
}

/// Where a background daemon's stdout and stderr go: ADR-004 §3's `.fathom/logs/`, which is fathom's own
/// directory and so needs no new permission (§7.7).
#[must_use]
pub fn log_path(root: &Path) -> PathBuf {
    root.join(".fathom").join("logs").join("daemon.log")
}

/// Make the process a session leader, so the terminal that spawned it cannot signal it later.
///
/// `setsid(2)` is `unsafe` in `rustix` for the reason every `fork`-then-`exec` helper gives: a child must not
/// touch the parent's state before it execs. This is not that child — it is a process that re-executed
/// itself, has one thread, and does nothing else first — and the call's only effect here is which session
/// and process group this process belongs to.
fn detach_session() {
    // `rustix::process::setsid` is a safe wrapper over `setsid(2)` — the `unsafe` in the syscall is about a
    // `fork`ed child touching the parent's state, and this is a process that re-executed itself. The crate
    // forbids `unsafe_code`, so a wrapper that needed it would have to be a reason in ADR-002 §9, not a
    // quiet exception here.
    #[cfg(unix)]
    let _ = rustix::process::setsid();
}

/// The stop flags: written by a signal handler, read by the loop.
///
/// `signal-hook`'s `flag` API is the entire async-signal-safe subset — the handler writes one `AtomicBool`
/// and nothing else — which is why no `unsafe` is needed at the call site, and why the crate being already in
/// the binary through `crossterm` (ADR-002 §6.2) makes this a new *use* rather than a new dependency. The
/// daemon cannot borrow `fathom-tui`'s signal setup: `watch` does not imply `tui`, and a headless
/// `--features minimal` binary that supervises a daemon still has to notice Ctrl-C.
struct Signals {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    hangup: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Signals {
    fn install() -> Result<Signals, eyre::Report> {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hangup = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        for (signal, flag) in [
            (signal_hook::consts::SIGINT, std::sync::Arc::clone(&stop)),
            (signal_hook::consts::SIGTERM, std::sync::Arc::clone(&stop)),
            (signal_hook::consts::SIGHUP, std::sync::Arc::clone(&stop)),
            (signal_hook::consts::SIGHUP, std::sync::Arc::clone(&hangup)),
        ] {
            signal_hook::flag::register(signal, flag).map_err(|err| {
                Pending::new(
                    ExitCode::Error,
                    format!("the handler for signal {signal} could not be installed: {err}"),
                    None,
                )
            })?;
        }
        Ok(Signals { stop, hangup })
    }

    fn stop(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether the reason was `SIGHUP` — ADR-004 §7.1's "the controlling terminal closed", which leaves with
    /// the cancellation code rather than looking like a crash.
    fn hangup(&self) -> bool {
        self.hangup.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ── `fathom daemon stop` ────────────────────────────────────────────────────────────────────────

/// `fathom daemon stop`.
pub fn stop(args: &DaemonStopArgs) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    if !root.is_dir() {
        return Err(not_a_repository(root));
    }
    let Some(health) = health(root, args.socket.as_deref().map(Path::new)) else {
        // "Not running" is the state the caller wanted, so this is success. `stop` is a command people run to
        // *make sure*, and failing it when the answer was already the desired one is how scripts learn to
        // write `|| true` and lose the cases that matter.
        print(
            args.json,
            &json!({"stopped": false, "reason": "no daemon is serving this repository"}),
            "daemon: not running",
        );
        return Ok(ExitCode::Ok);
    };
    let pid = health.get("pid").and_then(Value::as_u64).unwrap_or(0);
    let reason = if args.reason.is_empty() {
        "fathom daemon stop".to_owned()
    } else {
        args.reason.clone()
    };
    let reply = ask(
        root,
        args.socket.as_deref().map(Path::new),
        &proto::Request::Stop { reason },
        STOP_WAIT,
    );
    // A refusal is printed — the daemon knows why — but the *lease* is the final word, not the reply: a
    // daemon that died after answering and before releasing is a stop that did not happen, and a daemon that
    // released without answering is one that did.
    if let Answer::Refused { why, message } = &reply {
        output::problem(format_args!("the daemon refused: {why}: {message}"));
    }
    let gone = wait_for_release(root, pid, STOP_WAIT);
    print(
        args.json,
        &json!({
            "stopped": gone,
            "pid": pid,
            "answered": matches!(reply, Answer::Done(_)),
            "socket": endpoint(root, args.socket.as_deref().map(Path::new)).path.display().to_string(),
        }),
        &if gone {
            format!("daemon: stopped (pid {pid})")
        } else {
            format!(
                "daemon: pid {pid} was asked to stop and still holds its lease after {} s — it is \
                 finishing a checkpoint, or it is wedged; `kill {pid}` interrupts the checkpoint and the \
                 WAL recovers either way",
                STOP_WAIT.as_secs()
            )
        },
    );
    Ok(if gone { ExitCode::Ok } else { ExitCode::Error })
}

/// Wait until the process at `pid` no longer claims the repository.
fn wait_for_release(root: &Path, pid: u64, total: Duration) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if !lease_held_by(root, pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// Does the writer lease name this process, and is that claim live?
#[must_use]
fn lease_held_by(root: &Path, pid: u64) -> bool {
    let path = lease::default_path(root);
    matches!(
        Lease::state(&path, std::time::SystemTime::now(), StaleAfter::default()),
        State::Live(record) if u64::from(record.pid) == pid
    )
}

// ── `fathom status` ─────────────────────────────────────────────────────────────────────────────

/// The `fathom status --json` contract id, mirrored by `schemas/status.schema.json` and checked by
/// `tests/schemas.rs` like every other verb's payload.
pub const STATUS_CONTRACT: &str = "fathom.cli.status/1";

/// `fathom status`: who is keeping this index current, what it costs, and what is left to do.
///
/// The daemon answers its own health document ([`Daemon::health`]) and this verb adds what a daemon cannot see
/// — the store's index state and cache statistics — because `fathom-ingest` sits below the half of
/// `fathom-store` that opens a store. The split is printed, not hidden: the health document is the `daemon`
/// section, `daemon.source` says whether it came from a live process, a dead one's lease, a watcher's
/// heartbeat file or nothing at all, and every demotion between those is a finding with a remedy.
///
/// The exit code is the *repository's*, from the same gatherer `fathom stats` uses, plus the daemon's own
/// findings: a stale lease is a degraded index (ADR-004 §6.3), an absent daemon is not. A script that cares
/// whether the daemon is up reads `daemon.running` rather than a code that would have to mean two things.
pub fn status(args: &StatusArgs) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    if !root.is_dir() {
        return Err(not_a_repository(root));
    }
    let socket = args.socket.as_deref().map(Path::new);
    // The store half first, from the same gatherer `fathom stats` and `fathom doctor` use: one reader, three
    // verbs, and no chance that `status` calls the index fine while `stats` calls it stale. The daemon's
    // findings join these, so one exit code covers both halves of the answer.
    let mut snapshot = crate::state::gather(root, false);
    let endpoint = endpoint(root, socket);
    let mut daemon = serde_json::Map::new();
    let mut source = "none";

    if args.index {
        // Ask first, report second, so the health document below describes the state *after* the pass. A
        // client that only wants today's answer pays nothing for this, which is why it is a flag.
        match request_pass(root, socket, "fathom status --index", ASK_MS) {
            Ok(Some(pass)) => {
                daemon.insert("pass".to_owned(), pass);
            }
            Ok(None) => snapshot.findings.push(Finding::new(
                "daemon_absent",
                "daemon",
                format!(
                    "--index asked for a pass and there was no daemon to ask; the endpoint it would have \
                     used is {}",
                    endpoint.path.display()
                ),
                "fathom daemon start, or `fathom index` without --index to do the pass here",
                ExitCode::Ok,
            )),
            Err(why) => snapshot.findings.push(Finding::new(
                "daemon_refused",
                "daemon",
                format!("the daemon refused the pass --index asked for: {why}"),
                "`fathom status` prints the queue; `fathom index` does the pass in this process instead",
                ExitCode::Ok,
            )),
        }
    }

    match ask(root, socket, &proto::Request::Status, PROBE) {
        Answer::Done(health) => {
            source = "daemon";
            if let Some(object) = health.as_object() {
                daemon.extend(object.clone());
            }
        }
        Answer::Refused { why, message } => snapshot.findings.push(Finding::new(
            "daemon_refused",
            "daemon",
            format!(
                "something answered on {} and refused the status request: {why}: {message}",
                endpoint.path.display()
            ),
            "`fathom daemon stop` then `fathom daemon start`; the daemon this binary ships speaks this \
             protocol",
            ExitCode::Ok,
        )),
        Answer::Absent => snapshot.findings.push(Finding::new(
            "daemon_absent",
            "daemon",
            format!(
                "no daemon is serving this repository; the endpoint it would listen on is {}",
                endpoint.path.display()
            ),
            "fathom daemon start, or `fathom watch --once` for a single pass",
            ExitCode::Ok,
        )),
        Answer::Broken(why) => snapshot.findings.push(Finding::new(
            "daemon_unreadable",
            "daemon",
            format!(
                "something answered on {} in a shape this build cannot read: {why}",
                endpoint.path.display()
            ),
            "`fathom daemon stop` and start the daemon this binary ships; the protocol id is the first \
             field of any reply",
            ExitCode::Ok,
        )),
    }

    if source != "daemon" {
        // ADR-004 §7.1's client fallback chain, in order of trustworthiness, with each demotion recorded as a
        // finding rather than silently taken. The lease section stays in the payload even when it says
        // "free", because "I looked and nothing holds it" is evidence and not a blank.
        let path = lease::default_path(root);
        let state = Lease::state(&path, std::time::SystemTime::now(), StaleAfter::default());
        daemon.insert(
            "lease".to_owned(),
            json!({
                "path": path.display().to_string(),
                "state": state.as_str(),
                "held": state.held(),
                "holder": state.record().map(|record| json!({
                    "pid": record.pid,
                    "boot_id": record.boot_id,
                    "writer_epoch": record.writer_epoch,
                    "socket": record.socket,
                    "build": record.build,
                    "started_utc_ms": record.started_utc_ms,
                })),
                "heartbeat_age_ms": Lease::heartbeat_age(&path, std::time::SystemTime::now())
                    .map(|age| u64::try_from(age.as_millis()).unwrap_or(u64::MAX)),
            }),
        );
        match &state {
            State::Live(record) => {
                source = "lease";
                snapshot.findings.push(Finding::new(
                    "lease_live_no_ipc",
                    "daemon",
                    format!(
                        "the writer lease is live (pid {}, epoch {}) and nothing answered on its socket, so \
                         a writer is running without IPC — a `fathom watch` that took the lease without an \
                         endpoint, or a daemon started before this one's socket path",
                        record.pid, record.writer_epoch
                    ),
                    "stop it by pid, or `fathom daemon stop --pid`, which sends SIGTERM and waits",
                    ExitCode::Ok,
                ));
            }
            State::Stale { why, .. } => {
                source = "lease";
                snapshot.findings.push(Finding::new(
                    "lease_stale",
                    "daemon",
                    format!("the writer lease on disk belongs to a writer that is gone: {why}"),
                    "the next `fathom daemon start` or `fathom index` breaks the claim and says so; the \
                     store is consistent either way (ADR-004 §7.1)",
                    ExitCode::IndexDegraded,
                ));
            }
            State::Unreadable(why) => {
                source = "lease";
                snapshot.findings.push(Finding::new(
                    "lease_unreadable",
                    "daemon",
                    format!("the writer lease cannot be read: {why}"),
                    "`fathom doctor` checks `.fathom/`; the file can be removed while no process holds \
                     the flock, which `flock(2)` will tell you",
                    ExitCode::IndexDegraded,
                ));
            }
            State::Free => {}
        }
        if let Some(heartbeat) = crate::watch::read_status(&crate::watch::store_path(root)) {
            // A watcher's heartbeat is the last thing a *dead* process wrote down, which is more than nothing
            // and less than a live one. `source` is where that distinction has to be visible.
            daemon.insert("heartbeat".to_owned(), heartbeat);
            if source == "none" {
                source = "heartbeat";
            }
        }
    }
    daemon.insert("source".to_owned(), json!(source));
    daemon.insert("running".to_owned(), json!(source == "daemon"));

    let code = snapshot.worst_code();
    let mut report = serde_json::Map::new();
    report.insert("contract".to_owned(), json!(STATUS_CONTRACT));
    report.insert("root".to_owned(), json!(snapshot.root_display));
    report.insert("daemon".to_owned(), Value::Object(daemon));
    report.insert("repository".to_owned(), snapshot.repository.clone());
    report.insert("index".to_owned(), snapshot.index.clone());
    report.insert("cache".to_owned(), snapshot.cache.clone());
    report.insert("degraded".to_owned(), json!(code != ExitCode::Ok));
    report.insert("exit_code".to_owned(), json!(code.as_i32()));
    report.insert("exit_name".to_owned(), json!(code.name()));
    report.insert("diagnostics".to_owned(), json!(snapshot.findings_json()));
    let document = Value::Object(report);

    if args.json {
        print(true, &document, "");
        return Ok(code);
    }

    let health = &document["daemon"];
    if source == "daemon" {
        output::line(format_args!(
            "daemon: running — pid {}, writer epoch {}, up {}, protocol {}",
            health.get("pid").and_then(Value::as_u64).unwrap_or(0),
            epoch_of(&document),
            human_millis(health.get("uptime_ms").and_then(Value::as_u64).unwrap_or(0)),
            health
                .get("protocol")
                .and_then(Value::as_str)
                .unwrap_or("?"),
        ));
        if let Some(cpu) = health.get("cpu") {
            output::line(format_args!(
                "cpu: {:.1}% of one core measured over {} s (budget {:.1}%, throttled {} window(s), \
                 slept {}){}",
                cpu.get("measured_share")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    * 100.0,
                cpu.get("window_ms")
                    .and_then(Value::as_u64)
                    .map_or(1, |ms| (ms / 1_000).max(1)),
                cpu.get("budget_share")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    * 100.0,
                cpu.get("over_windows").and_then(Value::as_u64).unwrap_or(0),
                human_millis(cpu.get("slept_ms").and_then(Value::as_u64).unwrap_or(0)),
                if cpu.get("source").and_then(Value::as_str) == Some("estimate") {
                    " — measured from busy wall time, an upper bound: this platform has no process CPU clock"
                } else {
                    ""
                },
            ));
        }
        if let Some(queue) = health.get("queue") {
            output::line(format_args!(
                "requests: {} asked, {} coalesced, {} served, {} shed (depth {} of {}, high water {})",
                queue.get("pushed").and_then(Value::as_u64).unwrap_or(0),
                queue.get("coalesced").and_then(Value::as_u64).unwrap_or(0),
                queue.get("served").and_then(Value::as_u64).unwrap_or(0),
                queue.get("shed").and_then(Value::as_u64).unwrap_or(0),
                queue.get("depth").and_then(Value::as_u64).unwrap_or(0),
                queue.get("bound").and_then(Value::as_u64).unwrap_or(0),
                queue.get("high_water").and_then(Value::as_u64).unwrap_or(0)
            ));
        }
        if let Some(hooks) = health.get("hooks") {
            output::line(format_args!(
                "harness stream: offset {} of {}, {} bytes unread, {} read(s), {} rotation(s)",
                hooks.get("offset").and_then(Value::as_u64).unwrap_or(0),
                hooks.get("file_bytes").and_then(Value::as_u64).unwrap_or(0),
                hooks.get("lag_bytes").and_then(Value::as_u64).unwrap_or(0),
                hooks.get("reads").and_then(Value::as_u64).unwrap_or(0),
                hooks.get("rotations").and_then(Value::as_u64).unwrap_or(0)
            ));
            if let Some(stream) = hooks.get("stream") {
                output::line(format_args!(
                    "back-pressure: cap {} bytes/pass, {} saturated pass(es){}",
                    stream.get("cap_bytes").and_then(Value::as_u64).unwrap_or(0),
                    stream
                        .get("saturated_cycles")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    if stream
                        .get("backpressure")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        " — back-pressured now"
                    } else {
                        ""
                    },
                ));
            }
        }
        if let Some(passes) = health.get("passes") {
            output::line(format_args!(
                "index passes: {} run, {} client(s) served, {} refused; by origin {}",
                passes.get("count").and_then(Value::as_u64).unwrap_or(0),
                passes
                    .get("clients_served")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                passes
                    .get("clients_refused")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                passes
                    .get("requests_by_origin")
                    .and_then(Value::as_object)
                    .filter(|origins| !origins.is_empty())
                    .map(|origins| origins
                        .iter()
                        .map(|(origin, count)| format!("{origin}={count}"))
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_else(|| "none yet".to_owned())
            ));
        }
        if let Some(pass) = health.get("pass") {
            output::line(format_args!("this request: {}", describe_pass(pass)));
        }
        for notice in health
            .get("notices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            if let (Some(kind), Some(text)) = (
                notice.get("kind").and_then(Value::as_str),
                notice.get("text").and_then(Value::as_str),
            ) {
                output::line(format_args!("  {kind}: {text}"));
            }
        }
    } else {
        output::line(format_args!(
            "daemon: not running (source: {source}) — nothing is watching this repository; `fathom daemon \
             start` starts one (ADR-004 §7.1) and `fathom watch --once` is the one-shot form"
        ));
        if let Some(heartbeat) = health.get("heartbeat") {
            // The watcher's own status file, flat and written by a process that is now gone: whose it was,
            // what it was doing, and how long ago that was — the age is the file's mtime, which is the only
            // clock a dead process leaves behind.
            let age = Lease::heartbeat_age(
                &crate::watch::status_path(&crate::watch::store_path(root)),
                std::time::SystemTime::now(),
            );
            output::line(format_args!(
                "last watcher: pid {} — {} ({}), {} commit(s) indexed in {} run(s), heartbeat {} ago",
                heartbeat.get("pid").and_then(Value::as_u64).unwrap_or(0),
                heartbeat
                    .get("phase")
                    .and_then(Value::as_str)
                    .unwrap_or("?"),
                heartbeat
                    .get("phase_summary")
                    .and_then(Value::as_str)
                    .unwrap_or("?"),
                heartbeat
                    .pointer("/counters/commits_indexed")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                heartbeat
                    .pointer("/counters/index_runs")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                age.map_or_else(
                    || "an unknown time".to_owned(),
                    |age| format!("{} s", age.as_secs())
                ),
            ));
        }
    }
    if let Some(cache) = document.get("cache").filter(|cache| !cache.is_null()) {
        {
            output::line(format_args!(
                "cache: hit rate {} over {} lookup(s)",
                cache
                    .get("hit_rate")
                    .and_then(Value::as_f64)
                    .map_or_else(|| "never asked".to_owned(), |rate| format!("{rate:.0}%"),),
                cache.get("lookups").and_then(Value::as_u64).unwrap_or(0)
            ));
        }
    }
    if let Some(index) = document.get("index").filter(|index| !index.is_null()) {
        {
            output::line(format_args!(
                "index: {} commit(s) indexed, epoch {}, HEAD {}indexed",
                index.get("commits").and_then(Value::as_u64).unwrap_or(0),
                index.get("epoch").and_then(Value::as_u64).unwrap_or(0),
                if index
                    .get("head_indexed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    ""
                } else {
                    "not "
                }
            ));
        }
    }
    for line in snapshot.finding_lines() {
        output::line(format_args!("{line}"));
    }
    Ok(code)
}

/// The writer epoch, wherever the report keeps it: a live daemon's self-report and the lease fallback put it
/// in different places, because one is a process describing itself and the other is a file a dead process
/// left behind.
fn epoch_of(report: &Value) -> u64 {
    [
        "/daemon/lease/epoch",
        "/daemon/lease/writer_epoch",
        "/daemon/lease/holder/writer_epoch",
    ]
    .iter()
    .filter_map(|pointer| report.pointer(pointer).and_then(Value::as_u64))
    .next()
    .unwrap_or(0)
}

/// The repository is not a repository, in the code ADR-002 §7.3 assigns to it.
fn not_a_repository(root: &Path) -> eyre::Report {
    Pending::new(
        ExitCode::NotARepository,
        format!("{} is not a git repository", root.display()),
        None,
    )
    .into()
}

/// Print a report, human or JSON, in the one place that knows both shapes.
fn print(json: bool, value: &Value, line: &str) {
    if json {
        // A document that cannot render is a bug in this file rather than a condition a caller can handle,
        // and the alternative — printing nothing and exiting 0 — is a lie in a script's pipeline.
        match serde_json::to_string(value) {
            Ok(text) => output::line(format_args!("{text}")),
            Err(err) => {
                output::problem(format_args!("the report could not be rendered: {err}"));
            }
        }
    } else {
        output::line(format_args!("{line}"));
    }
}

/// `1234 ms`, `3.4 s`, `2 m 05 s` — the shape a reader scans rather than parses.
#[must_use]
fn human_millis(millis: u64) -> String {
    if millis < 1_000 {
        return format!("{millis} ms");
    }
    let seconds = millis / 1_000;
    if seconds < 60 {
        return format!("{}.{:01} s", seconds, (millis % 1_000) / 100);
    }
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    if minutes < 60 {
        return format!("{minutes} m {seconds:02} s");
    }
    format!("{} h {minutes:02} m", minutes / 60)
}

// ── the worker, and the job that asks instead of doing ─────────────────────────────────────────

/// The daemon's indexing, which is `fathom index`'s own chunk loop wearing the budget's slice and deadline.
///
/// Nothing here is new indexing: opening the store, the chunking, the classification of a failure into
/// "reopen the repository" / "the store is corrupt" / "try again", and the rule that an interrupted pass is
/// finished by the next one all belong to [`crate::watch::IndexJob`], which `fathom watch` already drives.
/// The daemon adds one thing — the *slice* and the *deadline* come from the budget rather than from a
/// constant — and that is what makes "budgeted background indexing" a property of the process rather than a
/// line in a config file.
struct Worker {
    root: PathBuf,
    store_path: PathBuf,
}

impl Worker {
    fn new(root: &Path) -> Worker {
        Worker {
            root: root.to_path_buf(),
            store_path: crate::watch::store_path(root),
        }
    }

    /// ADR-004 §9.2's resumption, said once at start-up: mark whatever the last writer left running as
    /// crashed and continue. `None` is the boring and usual answer, and it is the one that should print
    /// nothing.
    fn recover(&mut self) -> Option<String> {
        let mut store = Store::open(&self.store_path).ok()?;
        let recovered = store.recover_runs(now_millis()).ok()?;
        if recovered == 0 {
            return None;
        }
        Some(format!(
            "{recovered} run(s) left running by a previous writer were marked crashed; the index continues \
             from the last durable commit"
        ))
    }
}

impl daemon::Work for Worker {
    fn pass(
        &mut self,
        interest: &Interest,
        slice: usize,
        budget: Duration,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        let mut job = crate::watch::IndexJob::for_root(&self.root, self.store_path.clone(), slice);
        job.start_slice(budget);
        // Every chunk is forwarded to the daemon's pass record, which is why a `--json` client never sees a
        // chunk line: the chunks are the daemon's private progress and the pass is the public one.
        job.refresh(interest, progress)
    }

    fn capabilities(&self) -> Value {
        json!({
            "worker": "fathom index",
            "store": self.store_path.display().to_string(),
            "backend": if cfg!(feature = "sqlite-bundled") { "sqlite-bundled" } else { "none" },
            "epoch": Store::open(&self.store_path)
                .ok()
                .and_then(|store| store.epoch().ok()),
        })
    }

    fn settle(&mut self) -> Result<(), String> {
        // ADR-004 §5's checkpoint policy, operational at the one moment it was written for: a daemon that
        // exits without this leaves the WAL for the next opener to replay, which is correct and slower. An
        // error here is reported and not retried, because the store is consistent either way (§7.1 step 3 is
        // an efficiency, not a correctness, requirement).
        let store = Store::open(&self.store_path).map_err(|err| err.to_string())?;
        store
            .connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(|err| err.to_string())
    }
}

/// A [`Job`] that indexes nothing: it asks the daemon and waits (§1).
///
/// This is what makes "three TUIs and a CLI mean one pass" true for a verb that already had its own job. The
/// pass is the daemon's — the counters come from its reply — so a watcher's debounce, `--once` and
/// `--max-cycles` keep meaning what they say while the work happens in one process. A refusal is
/// `Transient`, never `Fatal`: the daemon decides when to run, and a watcher must not stop because a queue
/// was momentarily full.
pub(crate) struct Asked {
    root: PathBuf,
    /// Who this is, for the daemon's counters and its coalescing key.
    origin: String,
    /// How long to hold each request.
    wait: Duration,
}

impl Asked {
    #[must_use]
    pub(crate) fn new(root: &Path, origin: &str, wait: Duration) -> Asked {
        Asked {
            root: root.to_path_buf(),
            origin: origin.to_owned(),
            wait,
        }
    }
}

impl Job for Asked {
    fn refresh(
        &mut self,
        _interest: &Interest,
        _progress: &mut dyn FnMut(Progress),
    ) -> Result<Progress, JobError> {
        match request_pass(
            &self.root,
            None,
            &self.origin,
            u64::try_from(self.wait.as_millis()).unwrap_or(u64::MAX),
        ) {
            Ok(Some(reply)) => Ok(Progress {
                commits: reply.get("commits").and_then(Value::as_u64).unwrap_or(0),
                units: reply.get("units").and_then(Value::as_u64).unwrap_or(0),
                chunks: 1,
                complete: reply
                    .get("settled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            }),
            Ok(None) => Err(JobError::Transient(
                "the daemon stopped answering, so this process will index itself".to_owned(),
            )),
            Err(why) => Err(JobError::Transient(format!("the daemon refused: {why}"))),
        }
    }
}

/// The line `fathom watch` prints when it hands its indexing to a daemon.
#[must_use]
pub fn delegation_note(health: &Value, socket: &socket::Endpoint) -> String {
    format!(
        "a daemon (pid {}) already indexes this repository at {}, so this process will ask it for each \
         pass instead of doing a second copy; `fathom daemon stop` is the other answer",
        health.get("pid").and_then(Value::as_u64).unwrap_or(0),
        health
            .pointer("/socket/path")
            .and_then(Value::as_str)
            .unwrap_or(&socket.path.display().to_string()),
    )
}
