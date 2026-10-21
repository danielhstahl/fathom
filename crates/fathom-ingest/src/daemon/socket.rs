//! Where the daemon's IPC endpoint lives, and what it is allowed to be (`e6.3` / `reco-kvk`).
//!
//! # The question this answers
//!
//! A client has to find the daemon without being told where it is: `fathom status` in a second terminal,
//! run by a person who started the daemon an hour ago in a first one, has only the repository path. So
//! the endpoint must be *derivable* from the repository, and the derivation has to survive four things
//! that all happen in real deployments:
//!
//! 1. **the path-length limit of a unix socket**, which is not a filesystem limit — `sun_path` is 108
//!    bytes on Linux and 104 on macOS, and a repository under a deep CI workspace
//!    (`/home/runner/_work/fathom/fathom/.fathom/daemon.sock`) passes 104 without trying. A socket that
//!    cannot be bound is a daemon with no IPC, so the resolver falls back to a short name in a short
//!    directory rather than failing;
//! 2. **XDG and Windows locations**, which are different philosophies, not just different separators:
//!    `$XDG_RUNTIME_DIR` is the POSIX answer for a per-user, per-boot, tmpfs-backed socket directory
//!    (`/run/user/1000`), and the equivalent Windows namespace is a named pipe,
//!    `\\.\pipe\fathom-<digest>`, which has no directory at all and therefore no path-length problem;
//! 3. **a repository on a filesystem that cannot host sockets** — SMB shares, some container bind
//!    mounts — where `.fathom/daemon.sock` fails to bind for a reason that has nothing to do with fathom;
//! 4. **a path somebody else chose**: `--socket`, `[daemon] socket` in the config, and
//!    `$FATHOM_DAEMON_SOCKET` are all honoured verbatim, because the person running the container knows
//!    their own mount layout better than this module does.
//!
//! # What is *not* portable, said out loud
//!
//! The transport on Windows is a named pipe, and binding one needs the `windows`/`named_pipe` FFI that
//! every crate in this workspace forbids (`#![forbid(unsafe_code)]`). So an endpoint can be *resolved* on
//! Windows — that is what [`resolve`] is for, and the rules are below — but not *bound* by this build:
//! [`bind`] answers `Unsupported`, the daemon runs with the lease and the heartbeat file it has always
//! had, and `fathom status` reads those instead. That is a stated gap with a named reason, not a silent
//! one, and it is the same shape as ADR-005 §8's deferred clauses.
//!
//! # Precedence
//!
//! ```text
//!   --socket PATH                            verbatim (the caller resolved the config key too)
//!   $FATHOM_DAEMON_SOCKET                    verbatim
//!   <repo>/.fathom/daemon.sock               if it fits the platform's name limit
//!   $XDG_RUNTIME_DIR/fathom/daemon-<d>.sock  if $XDG_RUNTIME_DIR is set
//!   $TMPDIR/fathom-<user>/daemon-<d>.sock    last resort
//! ```
//!
//! The fallback name is `daemon-<12 hex of sha256(repository path)>` — 29 bytes — so two checkouts of
//! one repository (two users, two worktrees) get two endpoints that cannot collide, and the derivation is
//! a pure function of the repository, which is what lets a client find it without being told.

use std::io;
use std::path::{Path, PathBuf};

use crate::hooks::digest_bytes;

/// The `sun_path` limit for this platform, with the two numbers that matter.
///
/// POSIX leaves the size to the implementation; Linux uses 108, the BSDs and macOS 104, both including
/// the NUL terminator. Exceeding it is `ENAMETOOLONG` at bind time — a confusing error to discover *after*
/// the daemon has taken its lease and printed its banner, so it is checked before anything is bound.
pub const SUN_PATH_LIMIT: usize = if cfg!(any(target_os = "macos", target_os = "ios")) {
    104
} else {
    108
};

/// What kind of endpoint an [`Endpoint`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A unix domain socket at a real path.
    UnixSocket,
    /// A Windows named pipe (`\\.\pipe\…`). Resolvable, not bindable here — see the module header.
    NamedPipe,
}

impl Kind {
    /// The stable spelling for `--json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::UnixSocket => "unix",
            Kind::NamedPipe => "pipe",
        }
    }
}

/// How an endpoint came to be where it is — because "why is my socket *there*" is a real question, and the
/// answer is one sentence if the resolver kept track and a source read if it did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// `--socket`, or the equivalent config key the caller resolved.
    Explicit,
    /// `$FATHOM_DAEMON_SOCKET`.
    Environment,
    /// The repository-local path of ADR-004 §3, used because it fits.
    Repository,
    /// `$XDG_RUNTIME_DIR`, because the repository path did not fit the name limit.
    RuntimeDir,
    /// The temporary directory, because there was no `$XDG_RUNTIME_DIR` either.
    Temporary,
    /// A Windows named pipe, which is what "no directory" means on that platform.
    Pipe,
}

impl Reason {
    /// The stable spelling for `--json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Reason::Explicit => "explicit",
            Reason::Environment => "environment",
            Reason::Repository => "repository",
            Reason::RuntimeDir => "xdg-runtime-dir",
            Reason::Temporary => "temporary-dir",
            Reason::Pipe => "named-pipe",
        }
    }
}

/// A resolved endpoint: where, what kind, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The path or pipe name.
    pub path: PathBuf,
    /// Which transport it names.
    pub kind: Kind,
    /// Why it is there.
    pub reason: Reason,
    /// The path that *would* have been used, when the resolver fell back from it — so a status line can
    /// say "the repository socket was too long for `sun_path`, so this is the fallback" instead of leaving
    /// the difference to be discovered.
    pub fallback_from: Option<PathBuf>,
    /// The repository this endpoint was derived from.
    pub repository: PathBuf,
}

impl Endpoint {
    /// The short digest that identifies this repository's endpoint, for a message that must not print a
    /// path the user did not type.
    #[must_use]
    pub fn digest(repository: &Path) -> String {
        digest_bytes(repository.to_string_lossy().as_bytes())
            .chars()
            .take(12)
            .collect()
    }

    /// Whether this build can listen on it.
    ///
    /// Only a unix socket, and only where `std::os::unix::net` exists. A pipe name resolves and is
    /// reported; binding it is the gap named in the module header.
    #[must_use]
    pub fn bindable(&self) -> bool {
        self.kind == Kind::UnixSocket && cfg!(unix)
    }

    /// Whether the name is short enough to bind. Meaningless for a pipe, which has no such limit, and
    /// answered honestly as "yes" there rather than as a fake failure.
    #[must_use]
    pub fn fits(&self) -> bool {
        match self.kind {
            Kind::NamedPipe => true,
            Kind::UnixSocket => self.path.as_os_str().len() <= SUN_PATH_LIMIT,
        }
    }
}

/// The environment the resolver reads.
///
/// Passed in, because a resolver that reads the process environment itself cannot be tested at a fixed
/// state — and because ADR-002 §4.5 gives `fathom-cli` the job of reading variables, not this crate.
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// `$FATHOM_DAEMON_SOCKET`.
    pub socket: Option<PathBuf>,
    /// `$XDG_RUNTIME_DIR`.
    pub runtime_dir: Option<PathBuf>,
    /// `$XDG_STATE_HOME`, used to place state when the socket is not inside the repository.
    pub state_home: Option<PathBuf>,
    /// `$TMPDIR`, or the platform's equivalent.
    pub temp_dir: Option<PathBuf>,
    /// The user name, for the last-resort directory name.
    pub user: Option<String>,
    /// The Windows per-user application-data directory, where the equivalent of XDG lives.
    pub local_app_data: Option<PathBuf>,
}

impl Env {
    /// Read the endpoint-relevant environment, in one function so the set of variables this feature
    /// touches is a list a reviewer can count.
    #[must_use]
    pub fn from_process() -> Env {
        let var = |key: &str| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Env {
            socket: var("FATHOM_DAEMON_SOCKET"),
            runtime_dir: var("XDG_RUNTIME_DIR"),
            state_home: var("XDG_STATE_HOME"),
            temp_dir: var("TMPDIR"),
            user: std::env::var("USER").ok().filter(|value| !value.is_empty()),
            local_app_data: var("LOCALAPPDATA"),
        }
    }
}

/// Resolve the endpoint for `repository`.
///
/// `explicit` is `--socket` or the config key, already resolved by the caller; `None` means "derive it",
/// which is the normal case and the one this module exists for.
#[must_use]
pub fn resolve(repository: &Path, explicit: Option<&Path>, env: &Env) -> Endpoint {
    let kind = if cfg!(windows) {
        Kind::NamedPipe
    } else {
        Kind::UnixSocket
    };
    if let Some(path) = explicit {
        return Endpoint {
            path: path.to_path_buf(),
            kind,
            reason: Reason::Explicit,
            fallback_from: None,
            repository: repository.to_path_buf(),
        };
    }
    if let Some(path) = env.socket.as_deref() {
        return Endpoint {
            path: path.to_path_buf(),
            kind,
            reason: Reason::Environment,
            fallback_from: None,
            repository: repository.to_path_buf(),
        };
    }
    if kind == Kind::NamedPipe {
        // Windows: a pipe name, always short, always in the global pipe namespace. There is no
        // repository-local variant to prefer, because a pipe is not a file and there is nothing to fit.
        return Endpoint {
            path: PathBuf::from(format!(r"\\.\pipe\fathom-{}", Endpoint::digest(repository))),
            kind,
            reason: Reason::Pipe,
            fallback_from: None,
            repository: repository.to_path_buf(),
        };
    }

    let repo_socket = repository.join(".fathom").join("daemon.sock");
    if repo_socket.as_os_str().len() <= SUN_PATH_LIMIT {
        return Endpoint {
            path: repo_socket,
            kind,
            reason: Reason::Repository,
            fallback_from: None,
            repository: repository.to_path_buf(),
        };
    }
    // The repository path is too long for a socket name, so fall back to a per-user directory with a
    // content-derived name: the same derivation, two orders of magnitude fewer bytes.
    let name = format!("daemon-{}.sock", Endpoint::digest(repository));
    let (dir, reason) = if let Some(dir) = env.runtime_dir.clone() {
        (dir.join("fathom"), Reason::RuntimeDir)
    } else {
        let user = env.user.clone().unwrap_or_else(|| "user".to_owned());
        let base = env.temp_dir.clone().unwrap_or_else(std::env::temp_dir);
        (base.join(format!("fathom-{user}")), Reason::Temporary)
    };
    Endpoint {
        path: dir.join(&name),
        kind,
        reason,
        fallback_from: Some(repo_socket),
        repository: repository.to_path_buf(),
    }
}

/// Where a daemon's out-of-repository state goes when the socket is not inside the repository — the same
/// XDG reasoning for the same reasons. In the normal case everything lives under `.fathom/` and this is
/// not consulted.
#[must_use]
pub fn state_dir(repository: &Path, env: &Env) -> PathBuf {
    let base = env
        .state_home
        .clone()
        .or_else(|| env.local_app_data.clone())
        .unwrap_or_else(std::env::temp_dir);
    base.join("fathom").join(Endpoint::digest(repository))
}

/// A bound listener: one per daemon, on one endpoint. The type exists mainly to make two things that are
/// easy to forget impossible — unlinking a stale socket file, and setting the mode so a stranger on a
/// shared machine cannot ask a daemon what it is doing.
#[cfg(unix)]
pub struct Listener {
    inner: std::os::unix::net::UnixListener,
    endpoint: Endpoint,
    /// Failed accepts, counted rather than logged: one bad client must not end a daemon, and a counter on
    /// the status page is the difference between a weird night and a week of a client that cannot connect.
    accept_errors: std::sync::atomic::AtomicU64,
}

#[cfg(unix)]
impl Listener {
    /// Bind `endpoint`.
    ///
    /// A socket file left by a dead daemon is a real state, handled here rather than by asking somebody
    /// to `rm`: first the path is *asked* whether something is listening (a connect that succeeds means a
    /// live daemon, and binding over it would split the fleet in two), and only then is a file nobody
    /// answered unlinked.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::AlreadyExists`] when something is already listening, [`io::ErrorKind::InvalidInput`]
    /// when the name cannot fit, [`io::ErrorKind::Unsupported`] when the platform cannot bind this kind of
    /// endpoint at all; otherwise the OS's own failure.
    pub fn bind(endpoint: &Endpoint) -> io::Result<Listener> {
        if !endpoint.bindable() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "this build cannot listen on a {} endpoint ({})",
                    endpoint.kind.as_str(),
                    endpoint.path.display()
                ),
            ));
        }
        if !endpoint.fits() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} is {} bytes and a unix socket name is limited to {SUN_PATH_LIMIT}; pass a \
                     shorter --socket",
                    endpoint.path.display(),
                    endpoint.path.as_os_str().len()
                ),
            ));
        }
        if let Some(parent) = endpoint.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if live_listener(&endpoint.path).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "something is already listening on {} — either that is the daemon this process is \
                     about to duplicate, or the socket went away between the probe and the bind",
                    endpoint.path.display()
                ),
            ));
        }
        // Nothing answered. A leftover file is stale by definition — a live listener answers `connect`
        // even when its backlog is full — so removing it is the recovery rather than a gamble.
        if endpoint.path.exists() {
            std::fs::remove_file(&endpoint.path)?;
        }
        let inner = std::os::unix::net::UnixListener::bind(&endpoint.path)?;
        // Non-blocking accept: the loop that answers clients is the loop that indexes, and an accept that
        // can park is a client that can park the daemon.
        inner.set_nonblocking(true)?;
        // 0600. The endpoint is the daemon's control surface — it reports what a repository holds and can
        // be asked to index it — and a unix socket's filesystem mode is the only authentication it has.
        std::fs::set_permissions(
            &endpoint.path,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        )?;
        Ok(Listener {
            inner,
            endpoint: endpoint.clone(),
            accept_errors: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// Accept without blocking. `None` means nobody is knocking, or — for an error — see the counter.
    ///
    /// A returned connection has a read and write timeout, because a client that connected and then
    /// stalled must not be able to occupy the one thread that also does the indexing.
    pub fn accept(&self) -> Option<Connection> {
        match self.inner.accept() {
            Ok((stream, _addr)) => {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(250)));
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(250)));
                Some(Connection { stream })
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                None
            }
            Err(_err) => {
                // Not fatal — one bad client must not end a daemon — and not invisible either: the count
                // is on the status page, where "the daemon never answers me" turns into a number.
                self.accept_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                None
            }
        }
    }

    /// The endpoint this listener holds.
    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// How many accepts have failed since the daemon started.
    #[must_use]
    pub fn accept_errors(&self) -> u64 {
        self.accept_errors
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// A client connection the daemon is talking to.
#[cfg(unix)]
pub struct Connection {
    stream: std::os::unix::net::UnixStream,
}

#[cfg(unix)]
impl Connection {
    /// Read one request line. `None` on EOF or an empty line, which is the ordinary end of a
    /// one-request-per-connection exchange — and the shape of a liveness probe.
    ///
    /// # Errors
    ///
    /// The OS's words, including the read timeout the [`Listener`] armed.
    pub fn request(&mut self) -> io::Result<Option<String>> {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(self.stream.try_clone()?);
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            return Ok(None);
        }
        Ok(Some(trimmed.to_owned()))
    }

    /// A second handle on the same connection, so a reply can be written without moving the connection.
    ///
    /// Not for parking: a duplicated descriptor shares the socket, and `Drop` below shuts the *socket*
    /// down, so a parked client holding a clone would read EOF instead of its answer. Use [`Self::park`].
    #[must_use]
    pub fn try_clone(&self) -> Option<std::os::unix::net::UnixStream> {
        self.stream.try_clone().ok()
    }

    /// Hand the connection to the daemon's waiting room without closing it.
    ///
    /// A parked client is a process sitting in `read`, and this connection's `Drop` closes the socket —
    /// right for an exchange that has finished, wrong for one the daemon promised to keep. `forget` is how
    /// that promise is spelled: the descriptor stays open, the parked entry owns it, and it goes back to the
    /// OS when the answer is written or when the daemon itself exits.
    ///
    /// # Errors
    ///
    /// The OS's words from the descriptor duplication this cannot do.
    pub fn park(self) -> io::Result<std::os::unix::net::UnixStream> {
        let stream = self.stream.try_clone()?;
        std::mem::forget(self);
        Ok(stream)
    }

    /// Write one response line. Responses are one line, so this is a single `write_all`: a client that
    /// reads a line gets a whole one.
    ///
    /// # Errors
    ///
    /// The OS's words.
    pub fn reply(&mut self, line: &str) -> io::Result<()> {
        use std::io::Write;
        let mut stream = self.stream.try_clone()?;
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()
    }
}

#[cfg(unix)]
impl Drop for Connection {
    fn drop(&mut self) {
        // Closing *is* the end of the exchange: a client blocked on read gets EOF rather than a timeout,
        // which is the difference between a snappy CLI and a mysterious one. A parked connection is not at
        // its end — [`Connection::park`] arranges for this not to run — and the difference is the difference
        // between an answer and a hang.
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// The shape of a listener where no local socket transport exists: no fields, no constructor, so "the
/// daemon has no listener" can never be mistaken for "the daemon has a listener that is quiet". The type
/// exists so the daemon's `listener: Option<Listener>` compiles everywhere and stays honestly `None` off
/// Unix.
#[cfg(not(unix))]
pub struct Listener {
    /// `bind` is the only constructor and it always fails on this platform.
    _absent: (),
}

#[cfg(not(unix))]
impl Listener {
    /// Always `None`: there is nobody to accept.
    #[must_use]
    pub fn accept(&self) -> Option<Connection> {
        None
    }

    /// Always zero: there is nothing to fail.
    #[must_use]
    pub fn accept_errors(&self) -> u64 {
        0
    }
}

/// The connection type on a platform with no local socket transport. Unconstructable, and the type exists
/// only so a signature can name something that can never arrive.
#[cfg(not(unix))]
pub struct Connection {
    /// No way to build one.
    _absent: (),
}

/// Bind an endpoint, or say why it cannot be bound. The free function exists so the daemon's start-up path
/// reads as one step; the detail is on [`Listener::bind`].
#[cfg(unix)]
pub fn bind(endpoint: &Endpoint) -> io::Result<Listener> {
    Listener::bind(endpoint)
}

/// There is no local socket transport to bind on this platform, and the reason is in the error text rather
/// than in a comment — the daemon prints it, and `fathom status` prints it again.
#[cfg(not(unix))]
pub fn bind(endpoint: &Endpoint) -> io::Result<Listener> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "this build has no local socket transport, so {} cannot be listened on; clients read the \
             writer lease and the heartbeat file instead",
            endpoint.path.display()
        ),
    ))
}

/// Is something listening on `path`?
///
/// A connect that succeeds is the answer, and it is the only answer available without `SO_PEERCRED` and
/// the FFI that would come with it. The theoretical stall — a listener whose backlog is so full that
/// `connect` queues — is bounded by the daemon's own behaviour rather than by a socket option: it accepts
/// every loop pass and closes each connection after one request, so a backlog is not a thing that
/// accumulates.
#[cfg(unix)]
fn live_listener(path: &Path) -> Option<std::os::unix::net::UnixStream> {
    std::os::unix::net::UnixStream::connect(path).ok()
}

/// Whether anything is listening — a client's "is a daemon up?" probe, and the same question the resolver
/// asks before unlinking a stale socket file.
#[cfg(unix)]
#[must_use]
pub fn listening(path: &Path) -> bool {
    live_listener(path).is_some()
}

/// Nothing can be listening on a path this platform cannot open.
#[cfg(not(unix))]
#[must_use]
pub fn listening(_path: &Path) -> bool {
    false
}
