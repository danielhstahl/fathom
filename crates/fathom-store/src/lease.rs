//! The writer lease: one process per repository writes the store, and the lock — not the binary that
//! was invoked — decides which one (`e6.3` / `reco-kvk`, ADR-004 §7.1).
//!
//! # Why a lock and not a convention
//!
//! ADR-004 §7.1's sentence is the whole design argument: *"who writes is a fact about a lock, not a fact
//! about which binary you ran."* Three TUIs, an ambient pane and a supervised daemon are five processes
//! with a reason to write, and any rule of the form "the daemon is the writer" breaks the first time
//! somebody runs `fathom index` next to them. So the writer role is a resource that is claimed, and a
//! claim that cannot be won twice.
//!
//! # What the claim is made of
//!
//! Two facts, in one file, `.fathom/daemon.lock`:
//!
//! * a **kernel lock** — `flock(2)` with `LOCK_EX | LOCK_NB` on a descriptor the [`Lease`] keeps open,
//!   which is the arbiter. It cannot be leaked: the kernel drops it when the descriptor closes, so a
//!   `kill -9`, a segfault and an OOM all release a lease that was never released. That is the
//!   difference between a crash that needs an operator and one that does not;
//! * a **record** — `{pid, boot_id, pid_start, started_utc_ms, socket, writer_epoch, build, root}` —
//!   written once when the claim is won and never rewritten while it is held.
//!
//! The record is never rewritten because a reader must never see half of one. The lock file is *not*
//! renamed into place: a rename would swap the inode under the live `flock`, and a second process could
//! then lock a different inode while the first still believed it had won. So the file is written in
//! place, once, which leaves exactly one window in which a reader can see something partial — between
//! the file being created and the record being written — and that window reads as an empty file and is
//! reported as *"a claim being written or one just released"*, never as *"free"*.
//!
//! The **heartbeat** is the file's mtime, touched once a beat by [`Lease::heartbeat`]. It is not a
//! field: a field would have to be rewritten, and rewriting is the torn-read problem again. Touching a
//! mtime costs one syscall, cannot tear, and answers exactly the question a heartbeat asks — *when was
//! this last touched*.
//!
//! # The probes, in order of trustworthiness
//!
//! [`Lease::state`] answers *"is that lock mine, is it dead, is it someone else's"* from three probes:
//!
//! 1. the flock, which is the truth wherever `flock` is honoured — and ADR-004 §7.8 refuses the writer
//!    role on NFS and SMB precisely because it is not honoured there;
//! 2. the recorded pid, checked against the process table **and** against the recorded process start
//!    time, so a recycled pid is not mistaken for the holder that wrote the record;
//! 3. the heartbeat mtime against [`StaleAfter`] — the last probe standing where the first two are
//!    unavailable.
//!
//! `writer_epoch` increments per acquisition, which is what makes ADR-004 §7.1's zombie-writer rule
//! checkable: a row can be attributed to the writer generation that wrote it.
//!
//! # What this module deliberately is not
//!
//! Not a general lock manager: no re-entrance, no upgrade, no wait-queue. A caller that cannot take the
//! lease sends its mutation to whoever holds it or says so — which is the daemon's IPC reason for
//! existing at all.
//!
//! And on Windows, where this workspace forbids the `unsafe` FFI that `LockFileEx` needs, the claim is
//! the file's own exclusive creation plus the heartbeat. That is a weaker guarantee, and
//! [`Platform::note`] says so out loud rather than letting the same prose cover both: a dead holder's
//! lease there is broken after the staleness window, not the instant it dies.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// The record format version, distinct from the store schema version: a daemon and a CLI of different
/// builds read this file, and a field they disagree about must be named rather than assumed.
pub const RECORD_VERSION: u64 = 1;

/// ADR-004 §7.1's `daemon.lease_stale_s`, as a default: thirty seconds. Longer than a beat by an order
/// of magnitude, shorter than the time a person waits before assuming a daemon is wedged.
pub const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(30);

/// The `unknown` sentinel, for a platform that cannot name its boot or a process's start.
pub const UNKNOWN: &str = "unknown";

/// What the holder writes into `.fathom/daemon.lock`.
///
/// Every field exists to answer a question somebody asks at 02:14. `pid` and `pid_start`: *is it
/// running, and is it the one that wrote this?* `boot_id`: *is it running on this boot?* — a pid
/// survives a reboot, the thing it pointed at does not. `socket`: *where do I send a request?*
/// `writer_epoch`: *which generation wrote that row?* `build`: *is the running thing older than the
/// thing I just upgraded?* `root`: *which checkout is this, when there are four?*
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The record format version ([`RECORD_VERSION`]).
    pub version: u64,
    /// The holder's process id.
    pub pid: u32,
    /// The kernel boot identifier, where one can be read ([`boot_id`]); [`UNKNOWN`] elsewhere.
    pub boot_id: String,
    /// The holder's start time as `/proc/<pid>/stat` field 22 (clock ticks since boot), or [`UNKNOWN`].
    /// This is what tells "pid reused" from "process died" on a machine that has been up for weeks.
    pub pid_start: String,
    /// Wall-clock milliseconds at acquisition, passed in (ADR-002 §4.4 — this module never reads a
    /// clock, and a probe that read its own clock could not be tested at a fixed instant).
    pub started_utc_ms: i64,
    /// The IPC endpoint the holder listens on, already resolved. Empty means the holder published no
    /// socket, which is a state `fathom status` prints rather than one it invents around.
    pub socket: String,
    /// How many claims this path has had, including this one.
    pub writer_epoch: u64,
    /// The build that took the claim, e.g. `fathom 0.1.0-alpha.1`.
    pub build: String,
    /// The repository the holder watches, as it was named.
    pub root: String,
}

impl Record {
    /// The one-line form `fathom status` and `fathom doctor` print.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.socket.is_empty() {
            format!("pid {} · epoch {}", self.pid, self.writer_epoch)
        } else {
            format!(
                "pid {} · epoch {} · {}",
                self.pid, self.writer_epoch, self.socket
            )
        }
    }
}

/// What the platform can offer as a writer lock — and therefore what it promises about a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// `flock(2)`: the kernel releases the lease when the descriptor closes, so a dead holder is
    /// detected the instant anybody next tries to claim.
    Flock,
    /// Exclusive file creation plus the heartbeat: a dead holder's claim is broken only once the
    /// staleness window has passed.
    ClaimFile,
}

impl Platform {
    /// The stable spelling for `--json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Platform::Flock => "flock",
            Platform::ClaimFile => "claim-file",
        }
    }

    /// The caveat, printed beside the guarantee. A guarantee nobody knows the edge of is a surprise.
    #[must_use]
    pub const fn note(self) -> &'static str {
        match self {
            Platform::Flock => {
                "a dead holder's lease is released by the kernel, not detected by a timeout"
            }
            Platform::ClaimFile => {
                "no advisory lock primitive is available here, so a dead holder's lease is broken \
                 only once its heartbeat goes stale"
            }
        }
    }

    /// The platform this build runs on.
    #[must_use]
    pub const fn current() -> Platform {
        if cfg!(unix) {
            Platform::Flock
        } else {
            Platform::ClaimFile
        }
    }
}

/// How long a heartbeat may go silent before a claim counts as stale.
#[derive(Debug, Clone, Copy)]
pub struct StaleAfter(pub Duration);

impl Default for StaleAfter {
    fn default() -> Self {
        StaleAfter(DEFAULT_STALE_AFTER)
    }
}

/// Why a claim could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// Somebody else holds it and looks alive; the record is theirs, verbatim.
    Held(Option<Box<Record>>),
    /// The filesystem or the lock refused for a reason the OS gave.
    Failed(String),
}

impl Taken {
    /// The sentence a CLI prints, remedy included: a refusal that does not name the next verb is a wall.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Taken::Held(Some(record)) => format!(
                "another writer already holds this repository ({}; root {}; claimed at utc {} ms) — \
                 it does the watching and indexing for this checkout. `fathom status` asks it what it \
                 is doing; `fathom daemon stop` is how to stop it.",
                record.summary(),
                record.root,
                record.started_utc_ms,
            ),
            Taken::Held(None) => {
                "another writer holds this repository and its lease record cannot be \
                                  read; `fathom doctor` adjudicates the lease"
                    .to_owned()
            }
            Taken::Failed(why) => format!("the writer lease could not be taken: {why}"),
        }
    }
}

impl std::fmt::Display for Taken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Taken {}

/// What [`Lease::state`] found: a question with four honest answers and no fifth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// No file. Nobody claimed the role.
    Free,
    /// A record, and the holder is running.
    Live(Box<Record>),
    /// A record, and the holder is not: dead pid, different boot, or a heartbeat that went silent.
    /// `why` names which probe fired, because "stale" alone is not a diagnosis.
    Stale {
        /// The last record the file would give, if any.
        record: Option<Box<Record>>,
        /// Which probe decided it.
        why: String,
    },
    /// A file exists and holds no parseable record: a claim in mid-air, a truncated file, or a foreign
    /// file sitting at this path. Reported, never guessed at.
    Unreadable(String),
}

impl State {
    /// Is there a live writer? The one question a caller asks before deciding to write itself.
    #[must_use]
    pub fn held(&self) -> bool {
        matches!(self, State::Live(_))
    }

    /// The holder's record, live or dead.
    #[must_use]
    pub fn record(&self) -> Option<&Record> {
        match self {
            State::Live(record)
            | State::Stale {
                record: Some(record),
                ..
            } => Some(record),
            _ => None,
        }
    }

    /// The `--json` spelling: `free`, `live`, `stale`, `unreadable`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Free => "free",
            State::Live(_) => "live",
            State::Stale { .. } => "stale",
            State::Unreadable(_) => "unreadable",
        }
    }
}

/// A held lease. Dropping it releases the claim, which is the only release path that cannot be
/// forgotten: an early `return`, a `?`, a panic — the descriptor closes and the kernel lets go.
#[derive(Debug)]
pub struct Lease {
    path: PathBuf,
    record: Record,
    file: std::fs::File,
    platform: Platform,
    /// Set when winning this claim meant breaking one that a probe said was dead. ADR-004 §7.1's "broken
    /// with a log line, never silently" rule needs a *carrier* for that sentence, and the daemon's
    /// status and notices are where a human will actually read it.
    broken: Option<String>,
}

impl Lease {
    /// Take the lease at `path`, writing `record` into the file.
    ///
    /// `writer_epoch` is filled in here — one more than the previous record's — because a caller that
    /// had to increment it itself is a caller that eventually forgets, and an epoch nobody increments
    /// stops being evidence.
    ///
    /// # Errors
    ///
    /// [`Taken::Held`] when a live process holds it; [`Taken::Failed`] when the filesystem or the lock
    /// refused. A *stale* claim is not an error: it is broken, the epoch still increments, and the
    /// caller gets the lease.
    pub fn acquire(
        path: &Path,
        mut record: Record,
        now: SystemTime,
        stale: StaleAfter,
    ) -> Result<Lease, Taken> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|err| Taken::Failed(format!("{}: {err}", parent.display())))?;
        }
        record.version = RECORD_VERSION;
        let won = claim(path, now, stale)?;
        let (file, seen, broken, carried) = match won {
            Claim::Won { file, seen } => (file, seen, None, None),
            Claim::Busy(record) => return Err(Taken::Held(record)),
            // Only a platform with no advisory lock can find a claim that is present *and* not already
            // released by the kernel; there the probes are the arbiter, so a stale claim is broken —
            // loudly, and once — and the claim is retried. Twice in a row is not a race to keep chasing:
            // something else is claiming the path as fast as it is being broken, and that is a failure to
            // report rather than a loop to run.
            #[cfg(not(unix))]
            Claim::Stale { why, dead } => {
                std::fs::remove_file(path)
                    .map_err(|err| Taken::Failed(format!("{}: {err}", path.display())))?;
                match claim(path, now, stale)? {
                    Claim::Won { file, seen } => (
                        file,
                        seen.or_else(|| Some(dead.clone())),
                        Some(format!("the previous claim was broken: {why}")),
                        Some(dead),
                    ),
                    Claim::Busy(holder) => return Err(Taken::Held(holder)),
                    Claim::Stale { why: again, .. } => {
                        return Err(Taken::Failed(format!(
                            "{} was found stale, broken, and stale again: {again}",
                            path.display()
                        )));
                    }
                }
            }
        };
        let last = seen.clone().or(carried);
        record.writer_epoch = last.map_or(1, |last| last.writer_epoch + 1);
        // Winning a lock that still carried somebody's record is the crash case: a holder that released
        // cleanly blanked the file, so a record sitting under a free lock is a holder that never got to
        // say goodbye. Name it and say which probe decided — ADR-004 §7.1's "broken with a log line,
        // never silently", carried on the lease so the caller prints it where a human will read it.
        let broken = broken.or_else(|| {
            let dead = seen.as_ref()?;
            staleness(path, dead, now, stale)
                .map(|why| format!("the previous claim ({}) was broken: {why}", dead.summary()))
        });
        let lease = Lease {
            path: path.to_path_buf(),
            record,
            file,
            platform: Platform::current(),
            broken,
        };
        lease.write_record()?;
        Ok(lease)
    }

    /// The claim as written.
    #[must_use]
    pub fn record(&self) -> &Record {
        &self.record
    }

    /// This generation's epoch — the value ADR-004 §7.1 wants stamped on every row the writer touches.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.record.writer_epoch
    }

    /// Where the claim lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Which lock primitive this rests on, so the status page can state its own caveat.
    #[must_use]
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// "I had to break somebody's claim to get this", when that happened — the dead holder's record and
    /// the probe that decided it. `None` is the normal case.
    #[must_use]
    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    /// Beat the heartbeat: touch the mtime. The holder calls this once per loop pass; a monitor calls
    /// [`Lease::state`] to notice a holder that stopped calling it.
    ///
    /// # Errors
    ///
    /// The OS's own words. A failed beat does **not** mean the lease was lost — on
    /// [`Platform::Flock`] the kernel is holding it on a descriptor this process owns — so a caller
    /// reports it rather than shutting down over it.
    pub fn heartbeat(&self) -> std::io::Result<()> {
        let file = std::fs::OpenOptions::new().write(true).open(&self.path)?;
        file.set_modified(SystemTime::now())
    }

    /// Give the claim back early. Dropping does this as well; this is for the shutdown path that wants
    /// to print *released* rather than *went away*, and to know which it printed.
    ///
    /// # Errors
    ///
    /// The OS's own words from the truncate that blanks the record.
    pub fn release(&mut self) -> std::io::Result<()> {
        self.file.set_len(0)?;
        self.file.sync_all()?;
        unlock(&self.file);
        Ok(())
    }

    /// Write the record into the held file: truncate, write, flush, sync. One shot, in place — see the
    /// module header for why this file is never renamed.
    fn write_record(&self) -> Result<(), Taken> {
        let rendered = serde_json::to_vec_pretty(&self.record)
            .map_err(|err| Taken::Failed(format!("the lease record cannot be rendered: {err}")))?;
        let mut file = self
            .file
            // `File` is not `Clone`, and the lease owns it, so the write goes through the lease's own
            // descriptor: the same descriptor that holds the lock is the only one that should write.
            .try_clone()
            .map_err(|err| Taken::Failed(format!("{}: {err}", self.path.display())))?;
        use std::io::Seek;
        file.rewind()
            .and_then(|()| file.write_all(&rendered))
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_all())
            .map_err(|err| Taken::Failed(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }

    /// Read the claim and say what it means, without taking it.
    ///
    /// `now` is passed in for the staleness arithmetic (ADR-004 §6.5: the wall clock is not an ordering
    /// key, and a probe that reads its own clock cannot be tested at a fixed instant).
    #[must_use]
    pub fn state(path: &Path, now: SystemTime, stale: StaleAfter) -> State {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return State::Free,
            Err(err) => return State::Unreadable(format!("{}: {err}", path.display())),
        };
        if text.trim().is_empty() {
            // A released lease blanks this file — `release` and `Drop` both truncate it — so an empty file
            // means *nobody holds it*: the same fact a missing file states, and the answer `fathom status`
            // has to give after a clean `fathom daemon stop`. A half-written record is not empty and stays
            // `Unreadable`, because that is the case where guessing "free" could seat two writers in one
            // repository. The file is truncated rather than deleted for the same reason: a process that had
            // opened it before an unlink would then flock an inode no reader can see.
            return State::Free;
        }
        let record: Record = match serde_json::from_str(&text) {
            Ok(record) => record,
            Err(err) => return State::Unreadable(format!("{}: {err}", path.display())),
        };
        match staleness(path, &record, now, stale) {
            None => State::Live(Box::new(record)),
            Some(why) => State::Stale {
                record: Some(Box::new(record)),
                why,
            },
        }
    }

    /// The heartbeat's age, as the OS reports it. `None` when there is no file or its mtime cannot be
    /// read — which `fathom status` prints as `unknown`, never as zero.
    #[must_use]
    pub fn heartbeat_age(path: &Path, now: SystemTime) -> Option<Duration> {
        std::fs::metadata(path)
            .ok()?
            .modified()
            .ok()
            .and_then(|mtime| now.duration_since(mtime).ok())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Best effort on top of the kernel's own release: blank the record so a reader sees a claim
        // being written rather than a dead pid, then let go. Nothing in a destructor can act on a
        // failure, and closing the descriptor on the way out does the part that matters.
        let _ = self.file.set_len(0);
        unlock(&self.file);
    }
}

/// Read the record, if the file holds one.
#[must_use]
pub fn read(path: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The default lease path for a repository: `<root>/.fathom/daemon.lock` (ADR-004 §3).
///
/// The daemon and every client derive it the same way, which is the only reason a `fathom status` in a
/// second terminal can find a daemon nobody pointed it at.
#[must_use]
pub fn default_path(root: &Path) -> PathBuf {
    root.join(".fathom").join("daemon.lock")
}

/// The kernel boot identifier, where one can be read.
///
/// Linux exposes a uuid per boot for exactly this question — *same pid, same file, different boot*.
/// Elsewhere there is no safe, portable way to ask, so `None` is returned and the pid and its start time
/// carry liveness on their own, which is the majority of what the field is for.
#[must_use]
pub fn boot_id() -> Option<String> {
    let text = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let id = text.trim().to_owned();
    (!id.is_empty() && id != UNKNOWN).then_some(id)
}

/// This process's start token, or [`UNKNOWN`] where procfs cannot answer.
#[must_use]
pub fn self_start_token() -> String {
    process_start(std::process::id()).unwrap_or_else(|| UNKNOWN.to_owned())
}

/// `/proc/<pid>/stat` field 22 — clock ticks since boot at which `pid` started.
fn process_start(pid: u32) -> Option<String> {
    // Field 22 counting from the line's start; the `comm` field is parenthesised and may hold spaces and
    // parentheses, so the arithmetic runs from the closing paren, not from the head of the line.
    proc_field(&PathBuf::from(format!("/proc/{pid}/stat")), 22)
}

/// Whitespace-separated 1-based `field` of a procfs line, counting from the start of the line.
fn proc_field(path: &Path, field: usize) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let tail = text.rfind(')')?;
    text[tail + 1..]
        .split_whitespace()
        // The token after `)` is field 3 (`state`), so field N sits at index N − 3.
        .nth(field.checked_sub(3)?)
        .map(str::to_owned)
}

/// Is `pid` running, *and* is it the process that wrote this record?
///
/// Where procfs does not exist, "cannot tell" is answered as *assume the lock is real*: the conservative
/// reading, because the failure it prevents — two writers — is worse than the one it causes, a lease
/// that has to be waited out.
fn process_alive(pid: u32, pid_start: &str) -> bool {
    if pid == 0 {
        return false;
    }
    let stat = PathBuf::from(format!("/proc/{pid}/stat"));
    if !stat.exists() {
        return !cfg!(target_os = "linux");
    }
    if pid_start == UNKNOWN {
        return true;
    }
    proc_field(&stat, 22).is_none_or(|start| start == pid_start)
}

/// Which probes say the holder is gone, if any. Checked best-first; the first one that fires is the
/// sentence printed, because the strongest probe is the one worth quoting.
fn staleness(path: &Path, record: &Record, now: SystemTime, stale: StaleAfter) -> Option<String> {
    if !process_alive(record.pid, &record.pid_start) {
        return Some(format!("pid {} is not running", record.pid));
    }
    if record.boot_id != UNKNOWN
        && let Some(current) = boot_id()
        && current != record.boot_id
    {
        return Some(format!(
            "the claim is from boot {} and this is boot {current}",
            record.boot_id
        ));
    }
    if let Some(age) = Lease::heartbeat_age(path, now)
        && age > stale.0
    {
        return Some(format!(
            "its last heartbeat was {} s ago and the window is {} s",
            age.as_secs(),
            stale.0.as_secs()
        ));
    }
    None
}

/// The outcome of trying to claim the file, before any record is written.
enum Claim {
    /// This call holds it. `seen` is whatever record the file held beforehand, which is where the epoch
    /// comes from — read under the lock, so the increment cannot be raced.
    Won {
        file: std::fs::File,
        seen: Option<Record>,
    },
    /// Somebody else holds it. The record is what the file said, if anything.
    Busy(Option<Box<Record>>),
    /// A claim file exists, its holder is provably gone, and it must be removed before it can be won.
    /// Only reachable where there is no advisory lock to ask instead, which is why it is gated: an
    /// ungated variant this platform can never produce is a branch nobody tests.
    #[cfg(not(unix))]
    Stale {
        /// Which probe decided it, for the sentence the caller prints.
        why: String,
        /// The record it carried, for the epoch.
        dead: Record,
    },
}
/// Take the claim: `flock` on Unix, exclusive creation elsewhere.
///
/// On Unix there is no stale case to handle, and that is the point of using `flock`: a dead holder's
/// lock is already gone, so a failed non-blocking lock *means* a live holder.
#[cfg(unix)]
fn claim(path: &Path, _now: SystemTime, _stale: StaleAfter) -> Result<Claim, Taken> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|err| Taken::Failed(format!("{}: {err}", path.display())))?;
    match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => {
            let seen = read(path);
            Ok(Claim::Won { file, seen })
        }
        Err(err) => {
            let record = read(path);
            // The kernel says it is locked. If the record names a process that is gone, say so in the
            // message — the flock still wins, because the flock is the fact and the record is a
            // description of one.
            match record {
                Some(record) => Ok(Claim::Busy(Some(Box::new(record)))),
                None => Err(Taken::Failed(format!(
                    "{} is locked by a live process and holds no readable record \
                     (the flock is the arbiter, so it is not taken over): {err}",
                    path.display()
                ))),
            }
        }
    }
}

/// Take the claim where there is no advisory lock: exclusive creation, with staleness as the arbiter.
#[cfg(not(unix))]
fn claim(path: &Path, now: SystemTime, stale: StaleAfter) -> Result<Claim, Taken> {
    let fresh = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path);
    if let Ok(file) = fresh {
        return Ok(Claim::Won { file, seen: None });
    }
    match read(path) {
        Some(record) => match staleness(path, &record, now, stale) {
            Some(why) => Ok(Claim::Stale { why, dead: record }),
            None => Ok(Claim::Busy(Some(Box::new(record)))),
        },
        None => Ok(Claim::Busy(None)),
    }
}

#[cfg(unix)]
fn unlock(file: &std::fs::File) {
    let _ = rustix::fs::flock(file, rustix::fs::FlockOperation::Unlock);
}

#[cfg(not(unix))]
fn unlock(_file: &std::fs::File) {}

/// How long `now` is after a `started_utc_ms` stamp, for a status line that says *up for 4 h*.
#[must_use]
pub fn uptime(started_utc_ms: i64, now_utc_ms: i64) -> Option<Duration> {
    if started_utc_ms <= 0 || now_utc_ms < started_utc_ms {
        return None;
    }
    Some(Duration::from_millis(
        (now_utc_ms - started_utc_ms).max(0) as u64
    ))
}

/// Parse a record from bytes — for a test or a tool that writes one by hand.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<Record> {
    serde_json::from_slice(bytes).ok()
}

/// Render a record to text — the same renderer the writer uses, so a hand-written record and a real one
/// cannot drift apart.
#[must_use]
pub fn render(record: &Record) -> String {
    serde_json::to_string_pretty(record).unwrap_or_default()
}
