//! The daemon driven through the real binary — the ticket's four criteria, as far as a process boundary lets
//! them be driven: a running daemon answers `fathom status --json` with pid, uptime, offsets, queue depth and
//! cache stats; several clients are served by one pass; a clean stop and a SIGTERM both leave the lease
//! released and the store readable; and a restarted daemon picks the repository up where the last one left it.
//!
//! Why the binary and not the library: `fathom-ingest`'s own daemon test (`crates/fathom-ingest/tests/daemon.rs`)
//! already drives the loop with a scripted worker, and can assert *exactly* one pass because it controls when
//! the loop steps. Here the daemon is a separate process on a real socket indexing a real repository, so the
//! assertions are the ones a user can observe — a client that got an answer, a lease that was released, a
//! store that still opens — and the pass-counting assertions are the weaker "fewer passes than clients",
//! which is what coalescing has to look like from outside.
//!
//! Every test starts its own daemon on its own socket in its own repository, and every daemon is waited for:
//! a leaked foreground daemon holds a writer lease, and the next test in the same repository would report a
//! live lease for reasons that have nothing to do with what it is testing.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fathom_ingest::daemon::{proto, socket};
use serde_json::Value;

/// A throwaway git repository with a little history in it, and a `HOME` that keeps user config out of the way.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(tag: &str) -> Repo {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "fathom-cli-daemon-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"]);
        // Same reason as every other fixture here: a real repository ignores `.fathom/` (ADR-004 §12), and a
        // daemon writing its lease, its socket and its heartbeat into an *unignored* `.fathom/` races a test's
        // own `git add -A` — git lists `.fathom/watch/status.json.tmp`, the heartbeat renames it away, and git
        // dies on the vanished path. Local exclude rather than a committed `.gitignore`, so the history stays
        // the three commits the assertions count.
        let exclude = dir.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().expect("git created info/")).expect("info/");
        std::fs::write(&exclude, ".fathom/\n").expect("write the local exclude");
        for n in 0..3 {
            std::fs::write(
                dir.join(format!("src{n}.rs")),
                format!("fn f{n}() -> u32 {{ {n} }}\n"),
            )
            .expect("write a fixture file");
            git(&dir, &["add", "-A"]);
            git(&dir, &["commit", "-q", "-m", &format!("commit {n}")]);
        }
        Repo { dir }
    }

    fn commit(&self, subject: &str) {
        let n = self.dir.join("more.rs");
        std::fs::write(&n, format!("// {subject}\n")).expect("write a file");
        git(&self.dir, &["add", "-A"]);
        git(&self.dir, &["commit", "-q", "-m", subject]);
    }

    /// The endpoint every verb would derive for this repository, which is what the tests use too: a daemon
    /// started on a `--socket` of the test's choosing is a daemon no `fathom index` would ever find, and the
    /// deferral is the thing under test.
    fn socket(&self) -> PathBuf {
        socket::resolve(&self.dir, None, &socket::Env::default()).path
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.invalid")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn fathom(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("the built fathom binary is runnable")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// A foreground daemon, started as its own process and waited for when the test is done with it.
struct Daemon {
    child: Child,
    socket: PathBuf,
    root: PathBuf,
    log: PathBuf,
}

impl Daemon {
    /// Where the daemon listens, as the verbs would derive it.
    fn endpoint(&self) -> &Path {
        &self.socket
    }
}

impl Daemon {
    /// Start one and wait until its socket answers — which is the same readiness rule `fathom daemon start`
    /// applies to the child it detaches.
    ///
    /// The cycle is deliberately slow: the batching test needs several clients to arrive inside one of it,
    /// and a daemon that polls every 250 ms would answer each one as it landed and call that three passes.
    fn start(repo: &Repo, tag: &str, extra: &[&str]) -> Daemon {
        let socket = repo.socket();
        let log = repo.dir.join(format!("{tag}.log"));
        let log_file = std::fs::File::create(&log).expect("open the daemon's log");
        let child = Command::new(env!("CARGO_BIN_EXE_fathom"))
            .arg("daemon")
            .arg("start")
            .arg("--foreground")
            .arg("--root")
            .arg(".")
            .arg("--interval-ms")
            .arg("1000")
            .arg("--debounce-ms")
            .arg("1000")
            .arg("--quiet")
            .args(extra)
            .current_dir(&repo.dir)
            .stdout(Stdio::null())
            .stderr(Stdio::from(log_file))
            .spawn()
            .expect("start the daemon");
        let mut daemon = Daemon {
            child,
            socket,
            root: repo.dir.clone(),
            log,
        };
        daemon.wait_until_serving();
        daemon
    }

    fn status(&self) -> Value {
        let out = fathom(&self.root, &["status", "--json"]);
        // A daemon whose first pass has not landed yet reports a degraded index, which is exit 12 and is a
        // correct answer about the repository rather than a failure of the verb. Tests that care whether the
        // index is current say so with `wait_until_index_current` instead of leaning on the exit code.
        assert!(
            matches!(out.status.code(), Some(0) | Some(12)),
            "`fathom status --json` left {}: {}",
            out.status.code().unwrap_or(-1),
            stderr(&out)
        );
        serde_json::from_str(stdout(&out).trim()).expect("status --json is one JSON document")
    }

    /// Poll until the report says HEAD is indexed: the point a daemon has taken over and caught up.
    fn wait_until_index_current(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.status()["index"]["head_indexed"].as_bool() == Some(true) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the daemon never brought the index up to date: {}",
                self.status()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Poll until the health document says a daemon is serving, or give up with the daemon's own log attached.
    fn wait_until_serving(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let out = fathom(&self.root, &["status", "--json"]);
            if let Ok(value) = serde_json::from_str::<Value>(stdout(&out).trim())
                && value["daemon"]["running"].as_bool() == Some(true)
            {
                return;
            }
            if self.child.try_wait().ok().flatten().is_some() {
                panic!(
                    "the daemon exited before it served: {}",
                    std::fs::read_to_string(&self.log).unwrap_or_default()
                );
            }
            assert!(
                Instant::now() < deadline,
                "the daemon never answered on {}: {}",
                self.socket.display(),
                std::fs::read_to_string(&self.log).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Wait for the daemon to leave on its own, which is what both clean exits look like from here.
    fn wait_until_exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the daemon did not exit: {}",
                std::fs::read_to_string(&self.log).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// Ask for a pass over the socket and park until the daemon answers, from this process.
    ///
    /// This is the part the CLI cannot give a test: three connections written back to back are three requests
    /// *inside one cycle*, which is the condition coalescing exists for. A test that spawned three `fathom
    /// index` processes would be measuring how fast the OS starts processes instead.
    fn ask_parked(&self, origin: &str) -> thread::JoinHandle<proto::Reply> {
        let socket = self.endpoint().to_path_buf();
        let origin = origin.to_owned();
        thread::spawn(move || {
            let stream =
                std::os::unix::net::UnixStream::connect(&socket).expect("connect to the daemon");
            stream
                .set_read_timeout(Some(Duration::from_secs(60)))
                .expect("a read timeout");
            let request = proto::Request::Index {
                origin,
                wait_ms: 55_000,
            };
            let mut stream = stream;
            stream
                .write_all(request.line().as_bytes())
                .expect("write the request");
            stream.write_all(b"\n").expect("terminate the request");
            stream.flush().expect("flush the request");
            let mut line = String::new();
            let read = BufReader::new(&stream)
                .read_line(&mut line)
                .expect("read the answer");
            assert_ne!(read, 0, "the daemon closed without answering");
            proto::Reply::parse(line.trim_end())
        })
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // A daemon left running holds the repository's writer lease for the rest of the test binary's life,
        // and a later test would then read a live lease for a reason that has nothing to do with it.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// The health document `fathom status --json` has to carry: pid, uptime, offsets, queue depth, cache stats —
/// the ticket's fourth criterion, read at the pointer each one lives at.
#[test]
fn status_reports_pid_uptime_offsets_queue_depth_and_cache() {
    let repo = Repo::new("health");
    let daemon = Daemon::start(&repo, "health", &[]);
    daemon.wait_until_index_current();
    let health = daemon.status();

    assert_eq!(health["contract"].as_str(), Some("fathom.cli.status/1"));
    assert_eq!(health["daemon"]["source"].as_str(), Some("daemon"));
    assert_eq!(
        health["daemon"]["running"].as_bool(),
        Some(true),
        "the daemon is answering: {health}"
    );
    assert_eq!(
        health["daemon"]["pid"].as_u64().map(|pid| pid as u32),
        Some(daemon.child.id()),
        "the pid the report prints is the process that is running"
    );
    assert!(
        health["daemon"]["uptime_ms"]
            .as_u64()
            .is_some_and(|ms| ms < 60_000),
        "uptime is a duration of this run: {health}"
    );
    assert_eq!(
        health["daemon"]["protocol"].as_str(),
        Some(proto::PROTOCOL),
        "the report says which protocol the answering daemon speaks"
    );
    // Offsets: the durable hook cursor and the bytes still unread behind it.
    assert!(
        health["daemon"]["hooks"]["offset"].is_number(),
        "the hook offset is in the report: {health}"
    );
    assert!(
        health["daemon"]["hooks"]["lag_bytes"].is_number(),
        "and so is the lag the read cap produces: {health}"
    );
    // Queue depth, with its bound and its counters.
    assert!(
        health["daemon"]["queue"]["depth"].is_number()
            && health["daemon"]["queue"]["bound"].is_number(),
        "queue depth and bound: {health}"
    );
    for counter in ["pushed", "coalesced", "shed", "served"] {
        assert!(
            health["daemon"]["queue"][counter].is_number(),
            "the queue counter {counter} is in the report: {health}"
        );
    }
    // The CPU budget, measured and not merely configured.
    assert!(
        health["daemon"]["cpu"]["budget_share"].is_number()
            && health["daemon"]["cpu"]["measured_share"].is_number(),
        "the budget and the measurement of it: {health}"
    );
    // Cache stats, from the store the client can read — the half a daemon cannot reach on its own.
    assert!(
        health["cache"].is_object(),
        "cache stats come from the store beside the daemon's own half: {health}"
    );
    assert!(
        health["index"].is_object(),
        "and so does the index state: {health}"
    );
    assert_eq!(
        health["exit_code"].as_i64(),
        Some(0),
        "a running daemon over a repository it has indexed is not a failure: {health}"
    );
}

/// Three clients, one pass. The requests arrive inside one cycle, which is the condition the queue coalesces
/// under, and the counters are the assertion — the same counters `fathom status` prints for a human.
#[test]
fn several_clients_inside_one_cycle_make_one_pass() {
    let repo = Repo::new("coalesce");
    assert_eq!(
        fathom(&repo.dir, &["index", "."]).status.code(),
        Some(0),
        "the repository is indexed before the daemon takes it over"
    );
    let daemon = Daemon::start(&repo, "coalesce", &[]);
    // Settle first: a pass still in flight from the daemon's own start-up would be counted as one of the
    // passes these three clients are supposed to coalesce into.
    daemon.wait_until_index_current();
    let before = daemon.status();
    let passes_before = before["daemon"]["passes"]["count"].as_u64().unwrap_or(0);
    let pushed_before = before["daemon"]["queue"]["pushed"].as_u64().unwrap_or(0);

    let askers: Vec<_> = ["fathom tui", "fathom tui", "fathom index"]
        .iter()
        .map(|origin| daemon.ask_parked(origin))
        .collect();
    let replies: Vec<_> = askers
        .into_iter()
        .map(|asker| asker.join().expect("the client thread did not panic"))
        .collect();

    for reply in &replies {
        match reply {
            proto::Reply::Ok(data) => assert!(
                data["settled"].as_bool() == Some(true)
                    || data["coalesced"].as_bool() == Some(true),
                "a parked client is answered by a pass, not by a hang-up: {reply:?}"
            ),
            other => panic!("the daemon refused a parked client: {other:?}"),
        }
    }

    let after = daemon.status();
    let queue = &after["daemon"]["queue"];
    assert!(
        queue["pushed"].as_u64().unwrap_or(0) - pushed_before >= 3,
        "all three requests were counted: {queue}"
    );
    let passes = after["daemon"]["passes"]["count"].as_u64().unwrap_or(0);
    assert!(
        passes > passes_before,
        "the requests produced a pass ({} -> {}): {after}",
        passes_before,
        passes
    );
    assert!(
        passes - passes_before < 3,
        "three clients did not make three passes ({passes_before} -> {passes}): {after}"
    );
    assert!(
        queue["coalesced"].as_u64().unwrap_or(0) > 0,
        "at least two of the three folded into one request, which is what \"one pass for several clients\" \
         means in the counters: {queue}"
    );
    assert!(
        queue["shed"].as_u64().unwrap_or(0) == 0,
        "and none of them was refused: {queue}"
    );
    let by_origin = &after["daemon"]["passes"]["requests_by_origin"];
    assert!(
        by_origin
            .get("fathom tui")
            .and_then(Value::as_u64)
            .is_some(),
        "the requests are counted per asker, which is what makes the coalescing visible: {by_origin}"
    );
}

/// `fathom index` with a daemon serving is a client, not a second writer: it defers, and says so in its own
/// contract rather than inventing a run report for a pass it did not do.
#[test]
fn fathom_index_defers_to_a_serving_daemon() {
    let repo = Repo::new("defer");
    let daemon = Daemon::start(&repo, "defer", &[]);
    repo.commit("fourth");

    let out = fathom(&repo.dir, &["index", ".", "--json"]);
    assert_eq!(fixture_code(&out), 0, "{}", stderr(&out));
    let payload: Value = serde_json::from_str(stdout(&out).trim()).expect("one JSON document");
    assert_eq!(
        payload["contract"].as_str(),
        Some("fathom.cli.index.deferred/1"),
        "a deferred index is a different document, and says which: {payload}"
    );
    assert_eq!(payload["delegated"].as_bool(), Some(true));
    assert_eq!(
        payload["daemon"]["settled"].as_bool(),
        Some(true),
        "the daemon did the pass this process asked for: {payload}"
    );

    let health = daemon.status();
    assert!(
        health["daemon"]["passes"]["requests_by_origin"]["fathom index"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "and counted the request where `fathom status` can show it: {health}"
    );
    assert_eq!(
        health["daemon"]["running"].as_bool(),
        Some(true),
        "deferring did not disturb the daemon: {health}"
    );
}

/// A clean stop settles, releases, and leaves the repository readable — the same path whether it was asked for
/// over the socket or by `fathom daemon stop`.
#[test]
fn a_clean_stop_releases_the_lease_and_the_store_is_readable() {
    let repo = Repo::new("stop");
    let mut daemon = Daemon::start(&repo, "stop", &[]);
    let health = daemon.status();
    assert_eq!(health["daemon"]["running"].as_bool(), Some(true));

    let out = fathom(&repo.dir, &["daemon", "stop"]);
    assert_eq!(fixture_code(&out), 0, "{}", stderr(&out));
    daemon.wait_until_exit();

    let after: Value = {
        let out = fathom(&repo.dir, &["status", "--json"]);
        // No daemon, and no finding either beyond the one that says so: a stopped daemon is the normal
        // state of a repository, not a degraded one.
        serde_json::from_str(stdout(&out).trim()).expect("status is one JSON document")
    };
    assert_eq!(after["daemon"]["running"].as_bool(), Some(false));
    assert_ne!(after["daemon"]["source"].as_str(), Some("daemon"));
    assert_eq!(
        after["daemon"]["lease"]["state"].as_str(),
        Some("free"),
        "a released lease reads as free, not as wreckage: {after}"
    );

    // And the store it left behind is a store: `fathom stats` opens it read-only and says so.
    let stats = fathom(&repo.dir, &["stats", ".", "--json"]);
    let stats: Value =
        serde_json::from_str(stdout(&stats).trim()).expect("stats is one JSON document");
    assert_eq!(
        stats["index"]["integrity"].as_str(),
        Some("ok"),
        "the store is intact after a clean stop: {stats}"
    );
    assert!(
        stats["index"]["commits"].as_u64().unwrap_or(0) >= 3,
        "and it still holds what the daemon indexed: {stats}"
    );
}

/// SIGTERM is the supervisor's way of stopping a daemon (`--foreground` exists so a supervisor can supervise
/// it), and it has to leave the same shape a clean stop leaves: no lease held, store readable, restart works.
#[test]
fn sigterm_takes_the_same_path_as_a_clean_stop() {
    let repo = Repo::new("sigterm");
    assert_eq!(
        fathom(&repo.dir, &["index", "."]).status.code(),
        Some(0),
        "a daemon taking over an indexed repository is the case worth testing"
    );
    let mut daemon = Daemon::start(&repo, "sigterm", &[]);
    assert_eq!(daemon.status()["daemon"]["running"].as_bool(), Some(true));

    // `kill` is a shell builtin more reliably than a binary, so the shell sends it.
    let status = Command::new("sh")
        .args([
            "-c",
            "kill -TERM $1",
            "fathom-test",
            &daemon.child.id().to_string(),
        ])
        .status()
        .expect("`sh` is on PATH");
    assert!(status.success(), "SIGTERM the daemon");
    daemon.wait_until_exit();

    let after = fathom(&repo.dir, &["status", "--json"]);
    let after: Value =
        serde_json::from_str(stdout(&after).trim()).expect("status is one JSON document");
    assert_eq!(after["daemon"]["running"].as_bool(), Some(false));
    assert_eq!(
        after["daemon"]["lease"]["state"].as_str(),
        Some("free"),
        "the lease was released on the way out, not left for a timeout: {after}"
    );

    // Restart resumes: a second daemon takes the repository, and the epoch continues rather than restarting at
    // one — which is what makes a dead writer's rows recognisable (ADR-004 §6.3).
    let before = after["daemon"]["lease"]["holder"]["writer_epoch"]
        .as_u64()
        .unwrap_or(0);
    let restarted = Daemon::start(&repo, "sigterm-again", &[]);
    let health = restarted.status();
    assert_eq!(health["daemon"]["running"].as_bool(), Some(true));
    assert!(
        health["daemon"]["passes"]["count"].as_u64().unwrap_or(0) >= 1,
        "a restarted daemon does its first pass: {health}"
    );
    assert!(
        before == 0 || health["daemon"]["lease"]["epoch"].as_u64().unwrap_or(0) > before,
        "the writer epoch continued across the restart ({before} -> {}): {health}",
        health["daemon"]["lease"]["epoch"].as_u64().unwrap_or(0)
    );
}

/// The daemon is the writer, so the other verbs that would take the writer lease ask it instead: `fathom
/// watch --once` beside a daemon is a client, not a second copy of the same work in a second process (ADR-004
/// §7.1 — "who writes is a fact about a lock, not a fact about which binary you ran").
#[test]
fn a_watcher_beside_a_daemon_asks_instead_of_writing() {
    let repo = Repo::new("writer");
    let daemon = Daemon::start(&repo, "writer", &[]);
    repo.commit("fifth");

    let out = fathom(&repo.dir, &["watch", "--once", "--json"]);
    let code = fixture_code(&out);
    let text = stdout(&out);
    assert_eq!(code, 0, "{}", stderr(&out));
    // One object per change; the last one is the state it stopped in.
    let last = text
        .trim()
        .lines()
        .last()
        .unwrap_or_else(|| panic!("`fathom watch --once --json` printed nothing: {text}"));
    let payload: Value = serde_json::from_str(last).expect("one JSON object per change");
    assert_eq!(
        payload["indexer"].as_str(),
        Some("daemon"),
        "the machine-readable form says who did the work, not just the line a human reads: {payload}"
    );
    assert!(
        stdout(&fathom(&repo.dir, &["watch", "--once"])).contains("daemon"),
        "and so does the human form"
    );

    let health = daemon.status();
    assert!(
        health["daemon"]["passes"]["requests_by_origin"]["fathom watch"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "and the daemon counted the request, which is the proof the work came over the socket: {health}"
    );
}

fn fixture_code(out: &Output) -> i32 {
    out.status.code().expect("fathom exits with a code")
}

/// A binary built without the daemon would refuse these verbs; the shipped binary has them, and `fathom
/// status --help` is where that shows. Cheap, and it catches a rename of a verb a script pins.
#[test]
fn the_verbs_exist() {
    let repo = Repo::new("help");
    for args in [
        vec!["status", "--help"],
        vec!["daemon", "--help"],
        vec!["daemon", "start", "--help"],
        vec!["daemon", "stop", "--help"],
    ] {
        let out = fathom(&repo.dir, &args);
        assert_eq!(fixture_code(&out), 0, "{args:?}: {}", stderr(&out));
    }
    let absent = fathom(&repo.dir, &["status", "/nowhere/is/not/a/repo"]);
    assert_eq!(
        fixture_code(&absent),
        3,
        "a root that is not a repository is the code ADR-002 §7.3 assigns to it"
    );
}
