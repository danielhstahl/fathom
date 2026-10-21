//! The wire between a client and the daemon: one request line in, one response line out
//! (`e6.3` / `reco-kvk`).
//!
//! # Why a protocol at all, when there is a status file
//!
//! `.fathom/watch/status.json` (reco-8bx's heartbeat) answers *"is a watcher alive and what is its
//! phase"* with no socket, no lock and no protocol, and that is worth keeping. What it cannot do is
//! *ask for something*: take an index request, coalesce it with four others, and tell the requester
//! which pass covered it. That is the half ADR-004 §7.2 means by "a write-shaped request from a reader
//! is an IPC message, not an escape hatch", and it needs a request, not a file.
//!
//! # The shape
//!
//! Newline-delimited JSON, one request per connection:
//!
//! ```text
//!   → {"v":1,"request":"status"}
//!   ← {"v":1,"ok":true,"request":"status","data":{ … }}
//! ```
//!
//! Line-delimited rather than length-prefixed because the exchange is one request and one answer, and
//! because `nc -U .fathom/daemon.sock` then works, which is the difference between a control surface a
//! person can debug and one only this binary can talk to. One request per connection for the same
//! reason as the accept loop's read timeout: a connection that can be held open is a way for one client
//! to occupy the loop that also does the indexing.
//!
//! Every response carries `ok`. A failure response carries a `code` from [`code`], and the codes are
//! stable strings for the same reason the exit codes are — a script greps for them, and a rename of a
//! Rust variant must not silently change what a script greps for.
//!
//! # Versioning
//!
//! `v` is the protocol version and it is checked before anything else, in both directions. A v2 client
//! talking to a v1 daemon gets `protocol-version` naming both numbers, which is a sentence an operator
//! can act on; the alternative — a daemon that half-understands a new field — is the failure mode that
//! takes a week to find. `hello` is there so a client can ask before it assumes, and `fathom status`
//! prints what it answered.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

/// The protocol version this build speaks.
pub const VERSION: u64 = 1;

/// The stable name, printed by `hello` and by `fathom status`.
pub const PROTOCOL: &str = "fathom.daemon/1";

/// The error codes a refusal may carry. Stable strings, same argument as the exit codes.
pub mod code {
    /// The peer speaks a different protocol version.
    pub const PROTOCOL_VERSION: &str = "protocol-version";
    /// The request was not JSON, or not a request.
    pub const MALFORMED: &str = "malformed";
    /// A well-formed request for a method this daemon does not have.
    pub const UNKNOWN_METHOD: &str = "unknown-method";
    /// The daemon will not do it: e.g. an index request against a build with no store backend.
    pub const REFUSED: &str = "refused";
    /// The client could not connect at all.
    pub const UNAVAILABLE: &str = "unavailable";
    /// The client gave up waiting.
    pub const TIMEOUT: &str = "timeout";
    /// The exchange broke mid-flight: a request line that could not be read, a reply that could not be
    /// written. Distinct from [`UNAVAILABLE`] because the daemon was there and stopped being.
    pub const FAILED: &str = "failed";
}

/// What a client may ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// What are you, what do you speak, what can you do?
    Hello,
    /// The full health document. The same answer whether or not anything changed, which is what makes
    /// `fathom status` a *poll* rather than a subscription.
    Status,
    /// Index what is new. `wait` asks the daemon to hold the connection until a pass has covered this
    /// request, which is what lets `fathom index` defer to the daemon without racing it.
    Index {
        /// Who asked, for the counters and for the coalescing key.
        origin: String,
        /// How long to hold the connection, in milliseconds. `0` means "acknowledge and move on".
        wait_ms: u64,
    },
    /// Stop, cleanly: finish the pass in flight, checkpoint, release the lease.
    Stop {
        /// Who said so, printed in the daemon's last line.
        reason: String,
    },
}

impl Request {
    /// The method name, which is also its `--json` spelling.
    #[must_use]
    pub fn method(&self) -> &'static str {
        match self {
            Request::Hello => "hello",
            Request::Status => "status",
            Request::Index { .. } => "index",
            Request::Stop { .. } => "stop",
        }
    }

    /// Render as a request line.
    #[must_use]
    pub fn line(&self) -> String {
        let value = match self {
            Request::Hello => json!({"v": VERSION, "request": "hello"}),
            Request::Status => json!({"v": VERSION, "request": "status"}),
            Request::Index { origin, wait_ms } => json!(
                {"v": VERSION, "request": "index", "origin": origin, "wait_ms": wait_ms}
            ),
            Request::Stop { reason } => json!({"v": VERSION, "request": "stop", "reason": reason}),
        };
        // A `Value` built here cannot fail to render, and `to_string` (rather than `to_string_pretty`)
        // is the one-request-one-line promise.
        value.to_string()
    }

    /// Parse a request line. The error text is what the daemon echoes back as `malformed`, so it names
    /// the problem rather than the parser.
    ///
    /// # Errors
    ///
    /// A sentence when the line is not JSON, is not an object, is missing `v` or `request`, or carries a
    /// version this build does not speak.
    pub fn parse(line: &str) -> Result<Request, (String, Option<&'static str>)> {
        let value: Value = serde_json::from_str(line).map_err(|err| {
            (
                format!("the request is not JSON: {err}"),
                Some(code::MALFORMED),
            )
        })?;
        let version = value.get("v").and_then(Value::as_u64).ok_or_else(|| {
            (
                String::from("the request has no `v` version field"),
                Some(code::MALFORMED),
            )
        })?;
        if version != VERSION {
            return Err((
                format!("this daemon speaks protocol v{VERSION} and the request is v{version}"),
                Some(code::PROTOCOL_VERSION),
            ));
        }
        match value.get("request").and_then(Value::as_str) {
            Some("hello") => Ok(Request::Hello),
            Some("status") => Ok(Request::Status),
            Some("index") => Ok(Request::Index {
                origin: value
                    .get("origin")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .unwrap_or("client")
                    .to_owned(),
                wait_ms: value
                    .get("wait_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(MAX_WAIT_MS),
            }),
            Some("stop") => Ok(Request::Stop {
                reason: value
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .unwrap_or("client")
                    .to_owned(),
            }),
            Some(other) => Err((
                format!("`{other}` is not a method of {PROTOCOL}"),
                Some(code::UNKNOWN_METHOD),
            )),
            None => Err((
                String::from("the request has no `request` field"),
                Some(code::MALFORMED),
            )),
        }
    }
}

/// The longest a client may ask to be held: a minute. Past that, a client that wants to wait should poll
/// `status` instead of occupying a connection, and a daemon should not be able to be parked by a
/// caller's optimism.
pub const MAX_WAIT_MS: u64 = 60_000;

/// A success response line.
#[must_use]
pub fn ok(request: &str, data: Value) -> String {
    json!({"v": VERSION, "ok": true, "request": request, "data": data}).to_string()
}

/// A failure response line.
#[must_use]
pub fn err(request: &str, why: &str, message: &str) -> String {
    json!({
        "v": VERSION,
        "ok": false,
        "request": request,
        "error": {"code": why, "message": message},
    })
    .to_string()
}

/// What came back, as the client sees it.
#[derive(Debug, Clone)]
pub enum Reply {
    /// A success, with its `data`.
    Ok(Value),
    /// A refusal the daemon actually sent.
    Refused {
        /// One of [`code`]'s constants, or something newer than this build knows about.
        why: String,
        /// The daemon's own sentence, printed verbatim: it knows something the client does not.
        message: String,
    },
    /// The answer was not a response this build can read.
    Malformed(String),
}

impl Reply {
    /// Parse a response line.
    #[must_use]
    pub fn parse(line: &str) -> Reply {
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(err) => return Reply::Malformed(format!("the response is not JSON: {err}")),
        };
        let Some(ok) = value.get("ok").and_then(Value::as_bool) else {
            return Reply::Malformed(String::from(
                "the response has no `ok` field, so it is not a response",
            ));
        };
        if ok {
            return Reply::Ok(value.get("data").cloned().unwrap_or(Value::Null));
        }
        let why = value
            .pointer("/error/code")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let message = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("the daemon refused without saying why")
            .to_owned();
        Reply::Refused { why, message }
    }

    /// The data, if this was a success.
    #[must_use]
    pub fn data(&self) -> Option<&Value> {
        match self {
            Reply::Ok(data) => Some(data),
            _ => None,
        }
    }
}

/// Why a client could not get an answer. Distinct from [`Reply::Refused`]: a refusal is an answer, and
/// the difference is what a remedy has to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dial {
    /// Nothing is listening at this endpoint. The normal state in a repository with no daemon.
    Unavailable(String),
    /// Connected, then nothing came back in time.
    Timeout,
    /// The bytes that came back were not a response.
    Malformed(String),
    /// A write or read failed mid-exchange.
    Failed(String),
}

impl std::fmt::Display for Dial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dial::Unavailable(why) => write!(f, "nothing is listening: {why}"),
            Dial::Timeout => write!(f, "the daemon did not answer in time"),
            Dial::Malformed(why) => write!(f, "the daemon's answer could not be read: {why}"),
            Dial::Failed(why) => write!(f, "the request to the daemon failed: {why}"),
        }
    }
}

impl std::error::Error for Dial {}

/// Send one request to one endpoint and read one response.
///
/// One connection per call, deliberately: it is what lets the daemon's accept loop stay trivial —
/// read a line, answer, close — and it is what makes a client that crashes mid-request harmless.
///
/// # Errors
///
/// A [`Dial`] for the four ways a conversation fails before it produced a [`Reply`]. A refusal the
/// daemon *sent* is a `Ok(Reply::Refused)`, not an error: it answered.
#[cfg(unix)]
pub fn dial(endpoint: &Path, request: &Request, timeout: Duration) -> Result<Reply, Dial> {
    let mut stream = std::os::unix::net::UnixStream::connect(endpoint)
        .map_err(|err| Dial::Unavailable(format!("{}: {err}", endpoint.display())))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| Dial::Failed(err.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| Dial::Failed(err.to_string()))?;
    use std::io::Write;
    stream
        .write_all(request.line().as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|err| Dial::Failed(format!("sending the request: {err}")))?;
    let mut line = String::new();
    use std::io::BufRead;
    let read = std::io::BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::WouldBlock
                || err.kind() == std::io::ErrorKind::TimedOut
            {
                Dial::Timeout
            } else {
                Dial::Failed(format!("reading the answer: {err}"))
            }
        })?;
    if read == 0 {
        // The daemon closed without answering. That is a timeout with a different shape — usually a
        // daemon that was mid-shutdown — and saying so beats parsing an empty string.
        return Err(Dial::Timeout);
    }
    Ok(Reply::parse(line.trim_end()))
}

/// No local socket transport exists on this platform, so a client cannot ask. Stated, not silent: the
/// caller prints the sentence and falls back to the heartbeat file.
#[cfg(not(unix))]
pub fn dial(endpoint: &Path, _request: &Request, _timeout: Duration) -> Result<Reply, Dial> {
    Err(Dial::Unavailable(format!(
        "this build has no local socket transport, so {} cannot be dialed; `fathom status` reads \
         the heartbeat file instead",
        endpoint.display()
    )))
}
