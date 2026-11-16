//! `daemon.rs` — the acceptance criteria of `e6.3` (`reco-kvk`), driven against the real loop.
//!
//! The ticket's criteria are each about a property that only the *whole* loop has: one pass per four askers,
//! a CPU share that is enforced rather than configured, a shutdown that leaves the store and the lease in a
//! state the next process can use, and a status document that says what is true. A unit test of the queue or
//! the budget alone cannot show any of them, because each is a statement about the order the steps run in.
//!
//! So every test here drives [`Daemon::step`] — the same function `fathom daemon start` drives through
//! [`Daemon::run`] — with a scripted [`Work`], and talks to the daemon over its real socket with the real
//! [`proto`] client. The daemon is never moved to another thread on purpose: `Daemon` holds the lease
//! descriptor and the clock box, and a test that had to make it `Send` to prove something about it would be
//! proving a property nobody asks for at runtime.
//!
//! The store is deliberately absent — `fathom-ingest` is uphill of nothing that opens one — so "the store is
//! consistent" is asserted here as *the worker was asked to settle and the lease went back*, and the SQL half
//! is `crates/fathom-cli/tests/daemon_verb.rs`.
//!
//! Why the whole file is `cfg(unix)`: not the loop — the transport. Every test here asks the daemon
//! something over its socket, and off Unix there is no socket to ask over: [`socket::bind`] answers
//! `Unsupported` because binding a named pipe needs the FFI this workspace forbids (that crate's own
//! header names the gap). A daemon nobody can ask is not a loop under test, so these are gated rather than
//! left to fail on a platform that cannot host one — which is what `reco-wgcj` found.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(all(unix, any(feature = "watch", feature = "watch-poll")))]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use common::Fixture;
use fathom_core::time::{Clock, Timestamp};
use fathom_ingest::daemon::{self, Daemon, Options, Work, proto, socket};
use fathom_store::lease::{self, Lease, Record, StaleAfter, State, UNKNOWN};

/// The clock the daemon is handed (ADR-002 §4.4): a daemon that read its own clock could not be asked what
/// it would have said at a given instant, and the lease stamps come from here.
#[derive(Debug, Clone, Copy)]
struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_micros(1_700_000_000_000_000)
    }
}

/// The scripted index. It counts what it was asked for, how long it was told to spend, and whether it was
/// asked to settle on the way out — which is the whole surface the daemon owns over the work.
#[derive(Default)]
struct Scripted {
    /// Every pass, as the `(slice, budget)` the daemon asked for.
    passes: Vec<(usize, Duration)>,
    settles: usize,
    /// Wall time to burn in each pass, to make the budget have something to enforce.
    burn: Duration,
    /// Fail instead of working, when set.
    fail: Option<fathom_ingest::watch::JobError>,
}

impl Scripted {
    fn burning(millis: u64) -> Scripted {
        Scripted {
            burn: Duration::from_millis(millis),
            ..Scripted::default()
        }
    }
}

impl Work for Scripted {
    fn pass(
        &mut self,
        interest: &fathom_ingest::watch::Interest,
        slice: usize,
        budget: Duration,
        _progress: &mut dyn FnMut(fathom_ingest::watch::Progress),
    ) -> Result<fathom_ingest::watch::Progress, fathom_ingest::watch::JobError> {
        let _ = interest;
        if let Some(err) = &self.fail {
            return Err(err.clone());
        }
        let started = Instant::now();
        while started.elapsed() < self.burn {
            std::hint::spin_loop();
        }
        self.passes.push((slice, budget));
        Ok(fathom_ingest::watch::Progress {
            commits: 1,
            units: u64::try_from(slice).unwrap_or(u64::MAX),
            chunks: 1,
            complete: true,
        })
    }

    fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({"worker": "scripted"})
    }

    fn settle(&mut self) -> Result<(), String> {
        self.settles += 1;
        Ok(())
    }
}

/// A client thread told to ask: the sender fires once the request bytes are in the socket, the handle
/// yields the reply it got.
type Pending = (
    std::sync::mpsc::Receiver<()>,
    JoinHandle<Result<proto::Reply, std::io::Error>>,
);

/// Ask as a client that announces itself *after* its request bytes are in the socket and *before* it reads
/// the answer.
///
/// The accept loop takes whatever is pending in one pass, so a test that wants N requests coalesced into one
/// pass has to know they are all pending before it steps. That is the only race in this file, and it is worth
/// synchronising on rather than sleeping over: a sleep would make the acceptance test pass by luck.
fn ask_pending(
    harness: &Harness,
    origin: &str,
    wait_ms: u64,
) -> (
    std::sync::mpsc::Receiver<()>,
    JoinHandle<Result<proto::Reply, std::io::Error>>,
) {
    use std::io::{BufRead, Write};
    use std::sync::mpsc;

    let path = harness.daemon.endpoint().path.clone();
    let origin = origin.to_owned();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut stream = std::os::unix::net::UnixStream::connect(&path)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let request = proto::Request::Index {
            origin: origin.clone(),
            wait_ms,
        };
        stream.write_all(request.line().as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        // The bytes are the kernel's now: a loop pass that accepts will find this one.
        let _ = tx.send(());
        let mut line = String::new();
        let read = std::io::BufReader::new(&stream).read_line(&mut line)?;
        if read == 0 {
            return Ok(proto::Reply::Malformed(
                "the daemon closed without answering".to_owned(),
            ));
        }
        Ok(proto::Reply::parse(line.trim_end()))
    });
    (rx, handle)
}

/// Wait until every pending asker has written, so the next loop pass sees all of them at once.
fn await_writes(pending: &[Pending]) {
    for (started, _) in pending {
        started
            .recv_timeout(Duration::from_secs(10))
            .expect("the client wrote its request");
    }
}

/// Step until a raw client thread has finished, with the same wall-clock guard [`Harness::drive`] carries.
fn step_until<T>(daemon: &mut Daemon, asker: &JoinHandle<T>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !asker.is_finished() {
        daemon.step(&mut Scripted::default());
        assert!(
            Instant::now() < deadline,
            "the client was never answered — the loop did not accept it"
        );
    }
}

/// The options for a test: everything short enough that a loop pass costs milliseconds, and the endpoint
/// inside the fixture so a stray socket never escapes the temporary tree.
fn options(root: &Path) -> Options {
    let mut options = Options::for_root(root);
    options.watch.poll_interval = Duration::from_millis(5);
    options.watch.poll_interval_max = Duration::from_millis(5);
    options.watch.debounce = Duration::from_millis(1);
    options.watch.heartbeat = Duration::from_millis(5);
    options.watch.initial_refresh = false;
    options.tick = Duration::from_millis(1);
    options.cpu_window = Duration::from_millis(200);
    options.cpu_share = 0.90;
    options.slice = 40;
    options.yield_slice = 5;
    options.build = "fathom test".to_owned();
    options
}

/// A daemon under test: the fixture it was opened on, and the handle that puts its files back.
struct Harness {
    fixture: Fixture,
    daemon: Daemon,
}

impl Harness {
    fn new(label: &str) -> Harness {
        Harness::with(label, |options| options)
    }

    fn with(label: &str, tune: impl FnOnce(Options) -> Options) -> Harness {
        let fixture = Fixture::new(label);
        let root = fixture.init();
        fixture.commit(&root, "first", &[("a.txt", "one\n")]);
        let daemon =
            Daemon::open(&root, tune(options(&root)), Box::new(Fixed)).expect("open the daemon");
        Harness { fixture, daemon }
    }

    fn root(&self) -> &Path {
        self.fixture.root()
    }

    fn lease_path(&self) -> PathBuf {
        lease::default_path(self.root())
    }

    /// Ask the daemon something, from a client thread, and return the handle so the caller can keep
    /// stepping the loop while the answer is in flight.
    fn ask(&self, request: proto::Request) -> JoinHandle<proto::Reply> {
        let path = self.daemon.endpoint().path.clone();
        thread::spawn(move || {
            proto::dial(&path, &request, Duration::from_secs(5)).expect("the daemon answers")
        })
    }
}

// ---------------------------------------------------------------------------------------------
// §1: three TUIs and a CLI, one indexing pass
// ---------------------------------------------------------------------------------------------

/// The acceptance criterion, counted rather than argued: three TUIs and a CLI ask for a pass and the loop
/// runs one. The origins are what the real verbs send, because the criterion is about those verbs and not
/// about an idealised client.
#[test]
fn three_tuis_and_a_cli_make_one_indexing_pass() {
    let mut harness = Harness::new("daemon-one-pass");
    let before = harness.daemon.passes();

    let origins = ["fathom tui", "fathom tui", "fathom tui", "fathom index"];
    let pending: Vec<_> = origins
        .iter()
        .map(|origin| ask_pending(&harness, origin, 0))
        .collect();
    await_writes(&pending);

    let mut work = Scripted::default();
    // One pass of the loop: accept all four, then run the work the queue asked for.
    harness.daemon.step(&mut work);
    for (_, asker) in pending {
        let reply = asker
            .join()
            .unwrap()
            .expect("the client's exchange completed");
        assert!(
            reply.data().is_some(),
            "every asker is answered, not hung up on: {reply:?}"
        );
    }

    assert_eq!(
        harness.daemon.passes() - before,
        1,
        "four askers must produce one pass: {}",
        harness.daemon.health()
    );
    let queue = harness.daemon.queue().stats();
    assert_eq!(queue["pushed"].as_u64(), Some(4), "all four were counted");
    assert_eq!(
        queue["coalesced"].as_u64(),
        Some(2),
        "the three identical asks folded into one entry: {queue}"
    );
    assert_eq!(
        queue["shed"].as_u64(),
        Some(0),
        "and none were refused: {queue}"
    );
    let by_origin = harness.daemon.requests_by_origin();
    assert_eq!(
        by_origin.get("fathom tui").copied(),
        Some(3),
        "{by_origin:?}"
    );
    assert_eq!(
        by_origin.get("fathom index").copied(),
        Some(1),
        "{by_origin:?}"
    );
    assert_eq!(work.passes.len(), 1, "the worker ran once, for all of them");
}

/// A queue at its bound refuses the *new* asker rather than growing, and the refusal says why — which is the
/// back-pressure of §4 seen from the client's side, and the reason a flood of askers cannot turn into a
/// flood of work.
#[test]
fn a_queue_at_its_bound_refuses_a_new_asker_with_a_reason() {
    let mut harness = Harness::with("daemon-shed", |mut options| {
        options.queue_depth = 1;
        options
    });
    let pending = vec![
        ask_pending(&harness, "early-asker", 0),
        ask_pending(&harness, "late-asker", 0),
    ];
    await_writes(&pending);

    let mut work = Scripted::default();
    harness.daemon.step(&mut work);
    let replies = pending
        .into_iter()
        .map(|(_, asker)| asker.join().unwrap().expect("both exchanges completed"))
        .collect::<Vec<_>>();
    let refused = replies
        .iter()
        .filter(|reply| matches!(reply, proto::Reply::Refused { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        refused.len(),
        1,
        "one of the two fits and one does not: {replies:?}"
    );
    match &refused[0] {
        proto::Reply::Refused { why, message } => {
            assert_eq!(why, proto::code::REFUSED, "{message}");
            assert!(
                message.contains("queue is full"),
                "the refusal has to say why, not just no: {message}"
            );
        }
        _ => unreachable!("filtered above"),
    }
    assert_eq!(
        harness.daemon.queue().stats()["shed"].as_u64(),
        Some(1),
        "and the counter agrees with the refusal"
    );
}

/// A client that asked to be waited for is answered when the pass covers it, not when the wait runs out —
/// the difference between `fathom index` printing a pass and printing a shrug.
#[test]
fn a_waiting_client_is_answered_by_the_pass_that_covers_it() {
    let mut harness = Harness::new("daemon-wait");
    let asker = harness.ask(proto::Request::Index {
        origin: "fathom index".to_owned(),
        wait_ms: 5_000,
    });
    let mut work = Scripted::default();
    while !asker.is_finished() {
        harness.daemon.step(&mut work);
    }
    let reply = asker.join().unwrap();
    let data = reply.data().expect("the wait was answered");
    assert_eq!(data["settled"].as_bool(), Some(true), "{data}");
    assert_eq!(data["commits"].as_u64(), Some(1), "{data}");
}

// ---------------------------------------------------------------------------------------------
// §4: the budget, enforced
// ---------------------------------------------------------------------------------------------

/// A worker that burns more CPU than its share gets the loop to sleep, which is the enforcement: a budget
/// that is only measured is a status field, not a limit.
#[test]
fn a_worker_over_its_share_makes_the_loop_sleep() {
    let mut harness = Harness::with("daemon-budget", |mut options| {
        // 10 % of one core, measured over 50 ms. A 20 ms pass in every round cannot fit inside that, so a
        // loop that only measured would report a high share and a loop that enforces would sleep.
        options.cpu_share = 0.10;
        options.cpu_window = Duration::from_millis(50);
        options.tick = Duration::from_millis(2);
        options
    });
    let mut work = Scripted::burning(20);
    // Several rounds, because a budget is a statement about a *window*: one pass cannot be over a share.
    // This is the shape the ticket's §2 describes — a repository with something always happening.
    for round in 0..6 {
        let asker = harness.ask(proto::Request::Index {
            origin: format!("round-{round}"),
            wait_ms: 0,
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        while !asker.is_finished() {
            harness.daemon.step(&mut work);
            assert!(Instant::now() < deadline, "round {round} was never served");
        }
        asker.join().unwrap();
    }
    let cpu = harness.daemon.health()["cpu"].clone();
    assert!(
        cpu["windows"].as_u64().unwrap_or(0) >= 1,
        "a share is a statement about a closed window, so at least one has to have closed: {cpu}"
    );
    assert!(
        cpu["over_windows"].as_u64().unwrap_or(0) >= 1,
        "a worker over its share has to close at least one window over it: {cpu}"
    );
    assert!(
        cpu["slept_ms"].as_u64().unwrap_or(0) > 0,
        "and the debt has to be paid with sleep rather than with the foreground's latency: {cpu}"
    );
}

/// The slice the worker is asked for shrinks when a client has just been heard — the second half of the
/// ticket's "yields to foreground interaction", which is not the same thing as the total share.
#[test]
fn a_recent_client_shrinks_the_work_slice() {
    let mut harness = Harness::with("daemon-yield", |mut options| {
        options.slice = 100;
        options.yield_slice = 7;
        options
    });
    let quiet = vec![ask_pending(&harness, "a tui that just spoke", 0)];
    await_writes(&quiet);
    // The request itself marks the foreground present, so the pass that follows it is the yielded one.
    let mut work = Scripted::default();
    harness.daemon.step(&mut work);
    harness.daemon.step(&mut work);
    for (_, asker) in quiet {
        asker
            .join()
            .unwrap()
            .expect("the client's exchange completed");
    }
    let asked_for: Vec<usize> = work.passes.iter().map(|(slice, _)| *slice).collect();
    assert!(
        asked_for.contains(&7),
        "a pass taken right after a client was heard has to take the yield slice: {asked_for:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// §3: shutdown, crash and resumption
// ---------------------------------------------------------------------------------------------

/// A clean stop settles the store and releases the lease, in that order: the next process must find a
/// checkpointed store *and* a free claim, or `fathom daemon start` after a stop is a puzzle.
#[test]
fn a_stop_request_settles_then_releases() {
    let mut harness = Harness::new("daemon-stop");
    let stopper = harness.ask(proto::Request::Stop {
        reason: "test".to_owned(),
    });
    let mut work = Scripted::default();
    // `run` rather than `step`, because the shutdown path is `run`'s: it is the loop that calls `leave`.
    let stop = harness.daemon.run(&mut work, &|| false, &mut |_| {});
    assert!(matches!(stop, daemon::Stop::Requested(_)), "{stop:?}");
    assert_eq!(
        work.settles, 1,
        "the worker was asked to settle exactly once"
    );
    assert_eq!(
        Lease::state(
            &harness.lease_path(),
            SystemTime::now(),
            StaleAfter::default()
        ),
        State::Free,
        "the lease is back on the shelf"
    );
    assert!(
        stopper.join().unwrap().data().is_some(),
        "the stopper was answered"
    );
}

/// The caller's stop flag takes the same path as a client's request, because ADR-004 §7.1 does not rank
/// its reasons: Ctrl-C, SIGTERM, SIGHUP and `fathom daemon stop` all end in the same settle and the same
/// released lease. That identity is the property worth pinning — a shutdown that only worked over the socket
/// would be a shutdown with a hole in it.
#[test]
fn the_signals_stop_flag_takes_the_same_path_as_a_client() {
    let mut harness = Harness::new("daemon-signal");
    let flag = Arc::new(AtomicBool::new(false));
    let raiser = Arc::clone(&flag);
    let mut work = Scripted::default();
    let stop = harness.daemon.run(
        &mut work,
        &move || raiser.load(Ordering::SeqCst),
        &mut |daemon| {
            if daemon.cycles() >= 2 {
                // The handler's whole job is to set a bit; the loop decides when that means stop.
                flag.store(true, Ordering::SeqCst);
            }
        },
    );
    assert!(matches!(stop, daemon::Stop::Requested(_)), "{stop:?}");
    assert_eq!(work.settles, 1, "the same settle a `stop` request performs");
    assert_eq!(
        Lease::state(
            &harness.lease_path(),
            SystemTime::now(),
            StaleAfter::default()
        ),
        State::Free,
        "and the same released claim"
    );
}

/// A fatal worker ends the loop rather than the pass, and still gets the orderly exit — the case where the
/// store is corrupt and the operator needs the daemon to stop rather than to retry forever.
#[test]
fn a_fatal_worker_ends_the_loop_and_still_settles() {
    let mut harness = Harness::new("daemon-fatal");
    let mut work = Scripted {
        fail: Some(fathom_ingest::watch::JobError::Fatal(
            "the store is corrupt".to_owned(),
        )),
        ..Scripted::default()
    };
    let asker = harness.ask(proto::Request::Index {
        origin: "doomed".to_owned(),
        wait_ms: 0,
    });
    let stop = harness.daemon.run(&mut work, &|| false, &mut |_| {});
    assert!(matches!(stop, daemon::Stop::Fatal(_)), "{stop:?}");
    assert_eq!(work.settles, 1);
    let _ = asker.join().unwrap();
    assert_eq!(
        stop.summary(),
        "stopped: the store is corrupt",
        "the reason is the loop's last line, which is what `fathom daemon start` prints"
    );
    assert!(
        harness.daemon.health()["passes"]["last"]["fatal"]
            .as_bool()
            .unwrap_or(false),
        "and the fatal pass is in the document a client reads: {}",
        harness.daemon.health()
    );
}

/// The crash case, simulated honestly: a claim whose holder is long dead, and the socket file it left
/// behind still sitting at the endpoint. The daemon that starts on top of that wreckage has to break the
/// claim *and say so*, take the epoch forward, and bind anyway — every one of which is a thing an operator
/// would otherwise do by hand with `rm`.
#[test]
fn a_dead_holders_claim_is_broken_and_said() {
    let fixture = Fixture::new("daemon-crash");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("a.txt", "one\n")]);
    let path = lease::default_path(&root);
    std::fs::create_dir_all(path.parent().unwrap()).expect("the state directory");
    let endpoint = socket::resolve(&root, None, &socket::Env::default());
    std::fs::create_dir_all(endpoint.path.parent().unwrap()).expect("the endpoint's directory");
    // The socket a crashed daemon leaves: a file at the endpoint that answers nothing.
    let wreck =
        std::os::unix::net::UnixListener::bind(&endpoint.path).expect("leave a stale socket");
    drop(wreck);
    let dead = Record {
        version: lease::RECORD_VERSION,
        // A pid that cannot exist: `process_alive` treats pid 0 as gone on every platform.
        pid: 0,
        boot_id: lease::boot_id().unwrap_or_else(|| UNKNOWN.to_owned()),
        pid_start: UNKNOWN.to_owned(),
        started_utc_ms: 1,
        socket: endpoint.path.display().to_string(),
        writer_epoch: 41,
        build: "fathom crashed".to_owned(),
        root: root.display().to_string(),
    };
    std::fs::write(&path, lease::render(&dead)).expect("write the dead holder's claim");

    let reopened = Daemon::open(&root, options(&root), Box::new(Fixed))
        .expect("a stale claim is broken, not waited for");
    assert!(
        reopened
            .notices()
            .iter()
            .any(|notice| notice.kind == "lease" && notice.text.contains("taken over")),
        "the takeover is a notice: {:?}",
        reopened.notices()
    );
    assert_eq!(
        reopened.lease().epoch(),
        42,
        "the epoch continues across the crash, so rows stay attributable"
    );
    assert!(
        Lease::state(&path, SystemTime::now(), StaleAfter::default()).held(),
        "and the new claim is live"
    );
    assert!(
        socket::listening(&endpoint.path),
        "the stale socket was replaced by a live one"
    );
}

/// Two daemons, one repository: the second is refused with the holder named, which is the single-instance
/// guarantee from the outside.
#[test]
fn a_second_daemon_is_told_who_holds_the_lease() {
    let harness = Harness::new("daemon-double");
    let error = match Daemon::open(harness.root(), options(harness.root()), Box::new(Fixed)) {
        Ok(_) => panic!("one daemon per repository: the second one opened anyway"),
        Err(err) => err,
    };
    let message = error.to_string();
    assert!(
        message.contains(&std::process::id().to_string()),
        "the refusal names the holder, not just the fact of it: {message}"
    );
    assert_eq!(
        fathom_core::error::Diagnosable::code(&error),
        fathom_core::error::ErrorCode::Cancelled,
        "a live writer is the cancellation-shaped failure"
    );
}

// ---------------------------------------------------------------------------------------------
// §2: the endpoint, and what a crash leaves behind
// ---------------------------------------------------------------------------------------------

/// A socket file left by a dead daemon is not a reason to fail and not a reason to make the operator run
/// `rm`: the bind asks the path whether anything is listening, and unlinks it when nothing is.
#[test]
fn a_stale_socket_file_is_replaced_not_complained_about() {
    let fixture = Fixture::new("daemon-stale-socket");
    let root = fixture.init();
    let endpoint = socket::resolve(&root, None, &socket::Env::default());
    std::fs::create_dir_all(endpoint.path.parent().unwrap()).expect("the endpoint's directory");
    let left = std::os::unix::net::UnixListener::bind(&endpoint.path).expect("bind the socket");
    drop(left);
    assert!(endpoint.path.exists(), "and leave the file behind");
    let _daemon = Daemon::open(&root, options(&root), Box::new(Fixed))
        .expect("the next daemon clears the path for itself");
    assert!(endpoint.path.exists(), "with a live listener in its place");
    assert!(
        socket::listening(&endpoint.path),
        "and something answers there"
    );
}

/// A live listener is *not* stale: binding over it would split one repository across two daemons, which is
/// the exact failure ADR-004 §7.1's client contract exists to prevent.
#[test]
fn a_live_listener_is_never_stale() {
    let fixture = Fixture::new("daemon-socket-busy");
    let root = fixture.init();
    let endpoint = socket::resolve(&root, None, &socket::Env::default());
    std::fs::create_dir_all(endpoint.path.parent().unwrap()).expect("the endpoint's directory");
    let live = std::os::unix::net::UnixListener::bind(&endpoint.path).expect("bind the socket");
    let error = match socket::bind(&endpoint) {
        Ok(_) => panic!("a live listener is not a stale file: the bind succeeded over it"),
        Err(err) => err,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    drop(live);
}

/// The `sun_path` limit, which is the portability problem the resolver exists for: a deep repository path
/// cannot hold a socket name, so the endpoint moves to a short directory and says where it came from.
#[test]
fn a_repository_path_too_long_for_a_socket_name_falls_back() {
    let deep = std::env::temp_dir().join(format!(
        "fathom-daemon-endpoint-{}-{}",
        std::process::id(),
        "x".repeat(70)
    ));
    std::fs::create_dir_all(deep.join(".fathom")).expect("the deep tree");
    let env = socket::Env {
        runtime_dir: Some(std::env::temp_dir()),
        user: Some("tester".to_owned()),
        ..socket::Env::default()
    };
    let endpoint = socket::resolve(&deep, None, &env);
    assert!(
        endpoint.fallback_from.is_some(),
        "the repository path {} is {} bytes, over the {} limit, so the endpoint has to move",
        deep.display(),
        deep.as_os_str().len(),
        socket::SUN_PATH_LIMIT
    );
    assert!(endpoint.fits(), "and the fallback fits: {endpoint:?}");
    assert_eq!(endpoint.reason, socket::Reason::RuntimeDir);
    assert!(
        endpoint.path.to_string_lossy().contains("daemon-"),
        "the fallback name is derived from the repository: {endpoint:?}"
    );
    std::fs::remove_dir_all(&deep).ok();
}

/// `--socket` and `$FATHOM_DAEMON_SOCKET` are honoured verbatim, because the person running the container
/// knows their own mount layout.
#[test]
fn an_explicit_endpoint_wins_in_order() {
    let fixture = Fixture::new("daemon-endpoint-explicit");
    let root = fixture.init();
    let explicit = Path::new("/tmp/fathom-explicit-daemon.sock");
    let by_flag = socket::resolve(&root, Some(explicit), &socket::Env::default());
    assert_eq!(by_flag.path, explicit);
    assert_eq!(by_flag.reason, socket::Reason::Explicit);
    let by_env = socket::resolve(
        &root,
        None,
        &socket::Env {
            socket: Some(explicit.to_path_buf()),
            ..socket::Env::default()
        },
    );
    assert_eq!(by_env.path, explicit);
    assert_eq!(by_env.reason, socket::Reason::Environment);
    let derived = socket::resolve(&root, None, &socket::Env::default());
    assert_eq!(derived.reason, socket::Reason::Repository);
    assert!(derived.path.ends_with("daemon.sock"), "{derived:?}");
}

// ---------------------------------------------------------------------------------------------
// the harness stream's back-pressure
// ---------------------------------------------------------------------------------------------

/// The hook stream is read at most `hook_read_bytes` per pass, and the bytes left behind are reported as
/// lag rather than dropped — ADR-005 §2 counts a dropped event as a coverage gap, so back-pressure has to
/// be a *lag* and not a shed.
#[test]
fn the_hook_stream_is_capped_per_pass_and_the_lag_is_reported() {
    let mut harness = Harness::with("daemon-hooks", |mut options| {
        options.hook_read_bytes = 4_096;
        options
    });
    let events = harness.root().join(".fathom").join("events.jsonl");
    let line = format!(
        "{}\n",
        serde_json::json!({"seq": 1, "type": "noop", "pad": "x".repeat(180)})
    );
    std::fs::write(&events, vec![line.as_str(); 200].join("").as_bytes())
        .expect("write an events file bigger than one pass may read");

    let mut work = Scripted::default();
    harness.daemon.step(&mut work);
    let hooks = harness.daemon.hooks_json();
    let read = hooks["offset"].as_u64().unwrap_or(0);
    assert!(read > 0, "the cursor advanced: {hooks}");
    assert!(
        read <= 4_096 + u64::try_from(line.len()).unwrap_or(u64::MAX),
        // One pass reads at most its cap, rounded up to a whole line: a torn tail is not a readable event,
        // so the bound is a statement about passes and not about bytes to the last one.
        "one pass reads at most its cap plus the line that crossed it: {hooks}"
    );
    assert!(
        read < 40_000,
        "and it does not read the whole file in one pass — that is the back-pressure: {hooks}"
    );
    assert!(
        hooks["lag_bytes"].as_u64().unwrap_or(0) > 0,
        "the unread bytes are the lag, and the lag is printed: {hooks}"
    );
    assert_eq!(
        hooks["stream"]["saturated_cycles"].as_u64(),
        Some(1),
        "a pass that hit its cap is counted: {hooks}"
    );

    // And the lag drains, given passes: back-pressure is a queue, not a leak.
    for _ in 0..60 {
        harness.daemon.step(&mut work);
        if harness.daemon.hooks_json()["lag_bytes"].as_u64() == Some(0) {
            break;
        }
    }
    assert_eq!(
        harness.daemon.hooks_json()["lag_bytes"].as_u64(),
        Some(0),
        "the stream is eventually drained"
    );
}

/// A truncated-and-replaced events file — what `logrotate` does — is reported, because the cursor is
/// durable state and a reader that silently restarted at zero would double-count every event it had
/// already folded into the index.
#[test]
fn a_truncated_events_file_is_reported() {
    let mut harness = Harness::new("daemon-rotate");
    let events = harness.root().join(".fathom").join("events.jsonl");
    let line = |seq: u64| format!("{}\n", serde_json::json!({"seq": seq, "type": "noop"}));
    std::fs::write(&events, (1..=20).map(line).collect::<String>().as_bytes())
        .expect("write twenty events");
    let mut work = Scripted::default();
    harness.daemon.step(&mut work);
    let before = harness.daemon.hooks_json();
    assert!(before["offset"].as_u64().unwrap_or(0) > 0, "{before}");

    std::fs::write(&events, line(1).as_bytes()).expect("truncate it, as a rotation does");
    harness.daemon.step(&mut work);
    let after = harness.daemon.hooks_json();
    assert!(
        after["rotations"].as_u64().unwrap_or(0) >= 1 || !harness.daemon.notices().is_empty(),
        "either a rotation counter or a notice, never silence: before {before}, after {after}"
    );
}

// ---------------------------------------------------------------------------------------------
// the health document, as a contract
// ---------------------------------------------------------------------------------------------

/// Every field `fathom status` prints is asserted here, so the two ends of the socket cannot drift into a
/// status line that prints zeros because a field was renamed.
#[test]
fn the_health_document_carries_every_field_the_cli_prints() {
    let harness = Harness::new("daemon-health");
    let health = harness.daemon.health();
    for pointer in [
        "/protocol",
        "/protocol_version",
        "/pid",
        "/build",
        "/uptime_ms",
        "/cycles",
        "/lease/path",
        "/lease/epoch",
        "/lease/platform",
        "/socket/path",
        "/socket/kind",
        "/socket/reason",
        "/socket/listening",
        "/socket/fits",
        "/cpu/budget_share",
        "/cpu/measured_share",
        "/cpu/window_ms",
        "/cpu/source",
        "/cpu/over_windows",
        "/cpu/slept_ms",
        "/cpu/slice_units",
        "/cpu/foreground",
        "/queue/depth",
        "/queue/bound",
        "/queue/pushed",
        "/queue/coalesced",
        "/queue/shed",
        "/queue/served",
        "/hooks/offset",
        "/hooks/lag_bytes",
        "/hooks/stream/cap_bytes",
        "/hooks/stream/saturated_cycles",
        "/passes/count",
        "/passes/clients_served",
        "/passes/requests_by_origin",
        "/watch/phase",
        "/notices",
    ] {
        assert!(
            health.pointer(pointer).is_some(),
            "`fathom status` reads {pointer}, and the document has no such field: {health}"
        );
    }
    assert_eq!(health["protocol"], proto::PROTOCOL);
    assert_eq!(health["protocol_version"], proto::VERSION);
    assert_eq!(
        health["socket"]["kind"].as_str(),
        Some(socket::Kind::UnixSocket.as_str())
    );
}

/// A `hello` says what the daemon can do before a client waits for a pass, which is the difference between
/// "no worker compiled in" and "the worker is slow".
#[test]
fn hello_says_what_the_worker_can_do() {
    let mut harness = Harness::new("daemon-hello");
    let asker = harness.ask(proto::Request::Hello);
    let mut work = Scripted::default();
    while !asker.is_finished() {
        harness.daemon.step(&mut work);
    }
    let reply = asker.join().unwrap();
    let data = reply.data().expect("hello is answered");
    assert_eq!(data["worker"]["worker"], "scripted");
    assert!(
        data["methods"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("status"))
    );
}

/// An unknown method is a refusal with a code, not a hang and not a closed connection: the protocol is
/// versioned, and a newer client must learn that cleanly.
#[test]
fn an_unknown_method_is_a_refusal_with_a_code() {
    let mut harness = Harness::new("daemon-unknown");
    let path = harness.daemon.endpoint().path.clone();
    let asker = thread::spawn(move || {
        let mut stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
        use std::io::{BufRead, Write};
        stream
            .write_all(b"{\"v\":1,\"request\":\"teleport\"}\n")
            .unwrap();
        stream.flush().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut line)
            .unwrap();
        line
    });
    step_until(&mut harness.daemon, &asker);
    let line = asker.join().unwrap();
    let reply = proto::Reply::parse(&line);
    match reply {
        proto::Reply::Refused { why, .. } => assert_eq!(why, proto::code::UNKNOWN_METHOD),
        other => panic!("an unknown method is a refusal: {other:?}"),
    }
}

/// A request from a newer protocol version is refused *as a version problem*: silent compatibility is how a
/// daemon ends up answering a question it was not asked.
#[test]
fn a_request_from_another_protocol_version_is_refused_as_one() {
    let mut harness = Harness::new("daemon-version");
    let path = harness.daemon.endpoint().path.clone();
    let asker = thread::spawn(move || {
        let mut stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
        use std::io::{BufRead, Write};
        stream
            .write_all(b"{\"v\":99,\"request\":\"status\"}\n")
            .unwrap();
        stream.flush().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut line)
            .unwrap();
        line
    });
    step_until(&mut harness.daemon, &asker);
    let reply = proto::Reply::parse(&asker.join().unwrap());
    match reply {
        proto::Reply::Refused { why, .. } => assert_eq!(why, proto::code::PROTOCOL_VERSION),
        other => panic!("a version mismatch is refused: {other:?}"),
    }
}

/// A connect that says nothing — a liveness probe, a script that hung up — is not an error and is not
/// counted as a served client.
#[test]
fn a_client_that_hangs_up_is_not_a_refusal() {
    let mut harness = Harness::new("daemon-probe");
    let path = harness.daemon.endpoint().path.clone();
    thread::spawn(move || {
        let _stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
    })
    .join()
    .ok();
    let mut work = Scripted::default();
    harness.daemon.step(&mut work);
    harness.daemon.step(&mut work);
    let health = harness.daemon.health();
    assert_eq!(health["passes"]["clients_refused"].as_u64(), Some(0));
    assert_eq!(health["passes"]["clients_served"].as_u64(), Some(0));
}
