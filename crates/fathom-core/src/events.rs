//! The analysis event bus: what a run did, in the order it did it (`e6.7`, **reco-raw**).
//!
//! Three claims hold this module together, and each one has a test with its name in it.
//!
//! **One. A subscriber is somebody else's code, and its failure is not this analysis' failure.** Delivery
//! catches unwinds, counts them, and keeps delivering. A bus that let a subscriber panic would make an
//! optional feature — "let a tool watch what `fathom` did" — able to break the thing it watches, which is
//! the trade nobody agreed to.
//!
//! **Two. A reader ignores what it does not know, and says so.** Every line carries a `contract` and a
//! `schema_version`; an unknown *field* is skipped silently because that is what additive versioning means,
//! an unknown *kind* is kept as a kind because a newer producer's vocabulary is not this build's error, and
//! a *version this build cannot read* is refused with the two numbers named. A stream is read line by line
//! and a bad line is reported, never fatal — a run killed mid-write leaves one torn line at the end, and
//! every event before it is still worth having.
//!
//! **Three. Values in, values out.** No file is opened, no clock is read, no environment is consulted
//! (ADR-002 §4.1). [`Bus`] is a value; [`crate::events::kind`] is a list of strings; [`Event::at`] takes the
//! instant from the caller. That is what lets `crates/fathom-cli` hold the file handle and the clock while
//! this crate holds the contract, and what lets every test here be a function of its input.
//!
//! # The wire format
//!
//! One JSON object per line:
//!
//! ```text
//! {"contract":"fathom-analysis-events","schema_version":1,"kind":"detector.finished","seq":7,
//!  "run":"why-1780000000000","episode":"9f2c1d","detector":"untested_new_logic","outcome":"ran",
//!  "counts":{"findings":1,"hidden":0,"suppressed":0}}
//! ```
//!
//! `seq` is stamped by the bus, not by the producer, so it is a fact about one run rather than a timestamp
//! two producers disagree about. Absent optional fields are absent, not `null`: a stream a human greps
//! should not read like a database dump.
//!
//! # What is *not* here
//!
//! No filtering policy (a [`Filter`] answers "does this pattern match", the caller decides what to attach it
//! to), no delivery guarantees beyond "in order, on this bus, in this process", and no persistence. The
//! append-only file, its redaction and its rotation live in `crates/fathom-cli/src/events.rs`, which is
//! where a path can be refused for escaping the repository and a secret can be redacted on the way out —
//! decisions that belong to whoever owns the filesystem, not to the contract.

use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

pub mod kind;
pub mod schema;

/// The version of the event shapes published here.
///
/// Bumped by a removal, a rename, a narrowed type or a reordered vocabulary — the same rule
/// `ADR-004 §5.2` applies to every serialised structure in the product, and the same one
/// [`crate::insight`] states for the delta document. Additive changes — a new optional field, a new kind, a
/// new key inside `counts` — keep the version, because readers here ignore what they do not know, and that
/// is a tested promise rather than an accident.
pub const SCHEMA_VERSION: u32 = 1;

/// The contract id every line carries, so a reader that was handed the wrong stream says so instead of
/// printing it.
///
/// `paths.events` holds the harness stream, whose contract is `fathom-hooks`; a reader pointed at the wrong
/// one refuses line by line rather than inventing an answer.
pub const CONTRACT: &str = "fathom-analysis-events";

/// The largest number of subscribers one bus accepts.
///
/// A subscription list that grows without bound is a leak with a trait attached: a long-lived `fathom watch`
/// that subscribed once per event would grow forever. Sixteen is above anything the shipped surfaces use and
/// below the point where "who is listening?" stops being answerable on one screen. The refusal is counted
/// ([`Stats::refused`]) and returned as `None`, because a silent drop is the version of this bug that takes
/// a week to find.
pub const MAX_SUBSCRIBERS: usize = 16;

/// How many events a [`Recorder`] keeps by default.
pub const DEFAULT_RECORD_CAP: usize = 256;

/// The kinds a subscriber can subscribe to, as a value rather than as a match arm.
///
/// A kind this build does not declare is still a kind: `from_line` keeps it ([`Kind::is_declared`] says no,
/// nothing else cares), because the producer may be a newer build and the reader may be this one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Kind {
    name: String,
}

impl From<&str> for Kind {
    fn from(name: &str) -> Self {
        Kind::new(name)
    }
}

impl From<String> for Kind {
    fn from(name: String) -> Self {
        Kind::new(name)
    }
}

impl Kind {
    /// Name one. Any string is legal at this boundary, and a producer with a typo gets a kind nobody
    /// subscribes to rather than a panic in somebody else's subscriber.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Kind { name: name.into() }
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// The part before the first `.`, which is what a [`Filter`] with `analysis.*` matches on.
    ///
    /// A name with no dot is its own family, which is the honest answer rather than an empty string: a
    /// subscriber that filters by family still sees it.
    #[must_use]
    pub fn family(&self) -> &str {
        self.name.split('.').next().unwrap_or(&self.name)
    }

    /// Whether this build's [`kind::ALL`] lists it.
    ///
    /// `false` means "newer producer, or a typo", and nothing more: the event is still delivered and still
    /// printed.
    #[must_use]
    pub fn is_declared(&self) -> bool {
        kind::ALL.contains(&self.name.as_str())
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

/// One thing that happened, in one run.
///
/// Every field but the first five is optional, and the optionality is the design: a `detector.finished`
/// carries a `detector` and a `run.finished` does not, so the stream stays readable instead of carrying a
/// column of nulls for the sake of a uniform shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Which stream this belongs to. A reader refuses a line whose contract is not [`CONTRACT`].
    pub contract: String,
    /// The version of this shape. A reader that cannot read it says so by number.
    pub schema_version: u32,
    /// What happened, spelled `family.fact`.
    pub kind: Kind,
    /// This event's position in its run, stamped by the [`Bus`] and never by the producer.
    #[serde(default)]
    pub seq: u64,
    /// The run this event belongs to: one id per verb invocation, so two runs writing one stream can be
    /// pulled apart without a timestamp clock they agree on.
    pub run: String,
    /// When it happened. Supplied by the caller — this crate has no clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<Timestamp>,
    /// The episode it is about, where there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<String>,
    /// The detector it is about, where there is one. For `finding.raised` this is the namespaced type when
    /// the finding came from a plugin, which is what makes a plugin's findings routable (`e6.7`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detector: Option<String>,
    /// What the thing did: `ran`, `disabled`, `not_examined`, `suppressed`, `ok`, `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// The subject *within* the episode — a path, a symbol, a queue id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// The severity, spelled `high | medium | low`, as the finding model spells it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// The numbers. Names are stable within a major; a new one is additive.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub counts: BTreeMap<String, i64>,
    /// The sentence: a notice, a refusal's reason, or a finding's message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// What produced the stream, version included — so a stream written across an upgrade says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    /// The repository, as a label. Never an absolute path: the caller has a redactor for a reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

impl Event {
    /// Name one event in one run. Everything else is a builder, because everything else is optional.
    #[must_use]
    pub fn new(kind: impl Into<Kind>, run: impl Into<String>) -> Self {
        Event {
            contract: CONTRACT.to_owned(),
            schema_version: SCHEMA_VERSION,
            kind: kind.into(),
            seq: 0,
            run: run.into(),
            ts: None,
            episode: None,
            detector: None,
            outcome: None,
            subject: None,
            severity: None,
            counts: BTreeMap::new(),
            note: None,
            producer: None,
            repository: None,
        }
    }

    /// The episode it is about.
    #[must_use]
    pub fn episode(mut self, id: impl Into<String>) -> Self {
        self.episode = Some(id.into());
        self
    }
    /// The detector it is about.
    #[must_use]
    pub fn detector(mut self, id: impl Into<String>) -> Self {
        self.detector = Some(id.into());
        self
    }
    /// What the thing did.
    #[must_use]
    pub fn outcome(mut self, outcome: impl Into<String>) -> Self {
        self.outcome = Some(outcome.into());
        self
    }
    /// The subject within the episode.
    #[must_use]
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }
    /// The severity, spelled `high | medium | low`.
    #[must_use]
    pub fn severity(mut self, severity: impl Into<String>) -> Self {
        self.severity = Some(severity.into());
        self
    }
    /// One number.
    #[must_use]
    pub fn count(mut self, key: impl Into<String>, value: i64) -> Self {
        self.counts.insert(key.into(), value);
        self
    }
    /// The sentence.
    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
    /// When it happened, as a value the caller supplies.
    #[must_use]
    pub fn at(mut self, ts: Timestamp) -> Self {
        self.ts = Some(ts);
        self
    }
    /// What produced it.
    #[must_use]
    pub fn produced_by(mut self, producer: impl Into<String>) -> Self {
        self.producer = Some(producer.into());
        self
    }

    /// The one line this event is, with no trailing newline.
    ///
    /// `serde_json` keeps keys in the field order above, which is deliberate: the first five fields are the
    /// ones a reader greps for, and a stream whose prefix is stable is a stream you can read with `head`.
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Read one line back, with the rule of §Two applied: what is not understood is refused by name, and the
    /// refusal is a value rather than a panic or an `Err` the caller has to special-case.
    #[must_use]
    pub fn from_line(line: &str) -> Reading {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Reading::Ignored {
                reason: "blank line".to_owned(),
            };
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            return Reading::Ignored {
                reason: format!("not JSON: {}", summarize(trimmed)),
            };
        };
        // The contract first, because the likeliest mistake is a stream that is not ours at all — a reader
        // aimed at `paths.events`, say — and "I was not given my own stream" is a more useful sentence than
        // "the field `kind` is missing".
        if let Some(contract) = value.get("contract").and_then(serde_json::Value::as_str)
            && contract != CONTRACT
        {
            return Reading::Ignored {
                reason: format!(
                    "that stream's contract is `{contract}`, not `{CONTRACT}` — this reader reads the \
                     analysis event stream and nothing else"
                ),
            };
        }
        if let Some(version) = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            && u32::try_from(version).unwrap_or(u32::MAX) > SCHEMA_VERSION
        {
            return Reading::Ignored {
                reason: format!(
                    "that line claims schema version {version} and this build reads version \
                     {SCHEMA_VERSION}; a newer stream is refused rather than half-understood"
                ),
            };
        }
        match serde_json::from_value::<Event>(value) {
            Ok(event) => Reading::Accepted(event),
            Err(err) => Reading::Ignored {
                reason: err.to_string(),
            },
        }
    }
}

/// What reading one line produced: an event, or the reason there is not one.
///
/// A two-arm enum rather than an `Option` because the reason is the point — a stream that silently loses
/// half its lines is a stream nobody trusts, and the notices go to stderr beside the lines that did read.
#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "an accepted event is handed over whole and moved out of `from_line` by every caller; boxing               it would tax every good line to make the rare refusal cheaper, and the refusal is the one               that gets dropped immediately"
)]
pub enum Reading {
    /// A line this build could read.
    Accepted(Event),
    /// A line it could not, and the sentence that says why.
    Ignored {
        /// Why this line was refused.
        reason: String,
    },
}

impl Reading {
    /// The event, if there is one.
    #[must_use]
    pub fn event(&self) -> Option<&Event> {
        match self {
            Reading::Accepted(event) => Some(event),
            Reading::Ignored { .. } => None,
        }
    }

    /// The refusal, if that is what it was.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Reading::Accepted(_) => None,
            Reading::Ignored { reason } => Some(reason),
        }
    }
}

/// A stream read back: the events, and one sentence per line that was not an event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stream {
    /// What read, in the order it was read.
    pub events: Vec<Event>,
    /// One line per refused line, in the order it was refused.
    pub ignored: Vec<String>,
}

/// Read a whole stream — the append-only file's contents, or a test's `concat!`.
///
/// Blank lines are skipped without a notice: a file that ends in `\n` is not a file with a bad line in it.
/// Everything else goes through [`Event::from_line`], so the reader of a file and the reader of one line are
/// the same reader and cannot drift.
#[must_use]
pub fn read_stream(text: &str) -> Stream {
    let mut stream = Stream::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match Event::from_line(line) {
            Reading::Accepted(event) => stream.events.push(event),
            Reading::Ignored { reason } => stream.ignored.push(reason),
        }
    }
    stream
}

/// A short quote of a line that would not parse, so a notice about a torn write fits on one line.
fn summarize(line: &str) -> String {
    let mut out = String::new();
    for character in line.chars().take(40) {
        if character.is_control() {
            out.push(' ');
        } else {
            out.push(character);
        }
    }
    if line.chars().count() > 40 {
        out.push('…');
    }
    out
}

/// Something that wants to be told what happened.
///
/// `Send + Sync` because a bus is held by a run that may publish from any of its threads, and the
/// alternative — a subscriber that is only callable from the thread that registered it — is a constraint
/// invented for no reason.
pub trait Subscriber: Send + Sync {
    /// One event happened. Anything this does to fail — including panicking — is the bus's problem, not the
    /// analysis' (rule one).
    fn on_event(&self, event: &Event);
}

impl<F> Subscriber for F
where
    F: Fn(&Event) + Send + Sync,
{
    fn on_event(&self, event: &Event) {
        self(event);
    }
}

/// What a bus has done, for a report that has to account for itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Events handed to the bus.
    pub published: u64,
    /// Deliveries that succeeded, counted per subscriber.
    pub delivered: u64,
    /// Subscriber panics, caught and survived.
    pub panics: u64,
    /// Subscriptions refused at [`MAX_SUBSCRIBERS`].
    pub refused: u64,
    /// Who is listening right now.
    pub subscribers: usize,
}

#[derive(Default)]
struct Inner {
    subscribers: Mutex<Vec<(u64, Arc<dyn Subscriber>)>>,
    next_id: AtomicU64,
    seq: AtomicU64,
    published: AtomicU64,
    delivered: AtomicU64,
    panics: AtomicU64,
    refused: AtomicU64,
}

/// The bus. A value, cheap to clone, safe to publish on from anywhere, and silent when nobody is listening.
///
/// Every verb constructs one whether or not anything will read it — that is the only way "events are free
/// when off" can be true rather than asserted — so the empty case is the hot path and takes one atomic and
/// one lock on an empty vector.
#[derive(Clone, Default)]
pub struct Bus {
    inner: Arc<Inner>,
}

/// What a bus looks like from the outside: its numbers, not its subscriber list.
///
/// The list cannot be printed — a subscriber is somebody else's `Arc<dyn Subscriber>` and has no `Debug` to
/// ask — and printing the counts is the useful answer anyway: "three listeners, 41 delivered, 2 panics".
impl fmt::Debug for Bus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Bus")
            .field("stats", &self.stats())
            .finish()
    }
}

impl Bus {
    /// An empty bus.
    #[must_use]
    pub fn new() -> Self {
        Bus::default()
    }

    /// Listen. `None` once [`MAX_SUBSCRIBERS`] is reached — a refusal with a count behind it
    /// ([`Stats::refused`]), not a subscription that quietly does nothing.
    ///
    /// The returned [`Subscription`] unsubscribes when dropped, which is the whole lifetime story: a
    /// subscriber that outlives the run that made it is the leak this exists to prevent.
    #[must_use]
    pub fn subscribe(&self, subscriber: Arc<dyn Subscriber>) -> Option<Subscription> {
        let mut guard = self.lock();
        if guard.len() >= MAX_SUBSCRIBERS {
            drop(guard);
            self.inner.refused.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        guard.push((id, subscriber));
        Some(Subscription {
            id,
            bus: self.clone(),
        })
    }

    /// Hand one event to everyone listening, in subscription order, stamping its position first.
    ///
    /// The list is copied under the lock and the calls happen outside it, so a subscriber may publish to the
    /// same bus from inside a callback without deadlocking against itself — a subscriber that re-emits is a
    /// normal thing for a fan-out to want to do.
    pub fn publish(&self, mut event: Event) {
        event.seq = self.inner.seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.inner.published.fetch_add(1, Ordering::Relaxed);
        let subscribers: Vec<Arc<dyn Subscriber>> = self
            .lock()
            .iter()
            .map(|(_, subscriber)| Arc::clone(subscriber))
            .collect();
        for subscriber in subscribers {
            let delivered = catch_unwind(AssertUnwindSafe(|| subscriber.on_event(&event)));
            match delivered {
                Ok(()) => {
                    self.inner.delivered.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    self.inner.panics.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// What has happened on this bus so far.
    #[must_use]
    pub fn stats(&self) -> Stats {
        Stats {
            published: self.inner.published.load(Ordering::Relaxed),
            delivered: self.inner.delivered.load(Ordering::Relaxed),
            panics: self.inner.panics.load(Ordering::Relaxed),
            refused: self.inner.refused.load(Ordering::Relaxed),
            subscribers: self.lock().len(),
        }
    }

    /// Whether nobody is listening, which is how a hot call site skips the clone it does not need.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.lock().is_empty()
    }

    /// The sequence number the next event will be given, for a test or a report that wants to say how far a
    /// stream got.
    #[must_use]
    pub fn next_seq(&self) -> u64 {
        self.inner.seq.load(Ordering::Relaxed) + 1
    }

    /// The lock, with a poisoned guard treated as usable.
    ///
    /// The only thing under this lock is a list of subscribers, and a panic that happened while it was held
    /// — by a subscriber's code, on another thread — has not corrupted the list in a way that matters. The
    /// alternative is that a panic in one subscriber makes every later `publish` on this bus a panic, which
    /// is rule one inverted.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(u64, Arc<dyn Subscriber>)>> {
        self.inner
            .subscribers
            .lock()
            .unwrap_or_else(poisoned::PoisonError::into_inner)
    }
}

mod poisoned {
    //! The one place the module admits a lock can be poisoned, named so it reads as a decision.
    pub(super) use std::sync::PoisonError;
}

/// The handle that keeps a subscription alive. Drop it to stop listening.
#[derive(Debug)]
pub struct Subscription {
    id: u64,
    bus: Bus,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut guard = self.bus.lock();
        guard.retain(|(id, _)| *id != self.id);
    }
}

/// A subscriber that keeps the last few events, for tests and for a verb that wants to print what it just
/// emitted without opening a file.
///
/// Bounded on purpose, and the overflow is counted rather than dropped silently: an unbounded recorder that
/// happens to be attached to `fathom watch` is a memory leak wearing a test utility's clothes.
#[derive(Debug)]
pub struct Recorder {
    cap: usize,
    kept: Mutex<std::collections::VecDeque<Event>>,
    dropped: AtomicU64,
}

impl Recorder {
    /// One that keeps `cap` events, at least one.
    #[must_use]
    pub fn new(cap: usize) -> Arc<Self> {
        Arc::new(Recorder {
            cap: cap.max(1),
            kept: Mutex::new(std::collections::VecDeque::new()),
            dropped: AtomicU64::from(0),
        })
    }

    /// The events kept, oldest first.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.kept
            .lock()
            .unwrap_or_else(poisoned::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Just their kinds, which is what an assertion about a lifecycle actually wants.
    #[must_use]
    pub fn kinds(&self) -> Vec<String> {
        self.kept
            .lock()
            .unwrap_or_else(poisoned::PoisonError::into_inner)
            .iter()
            .map(|event| event.kind.to_string())
            .collect()
    }

    /// How many are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.kept
            .lock()
            .unwrap_or_else(poisoned::PoisonError::into_inner)
            .len()
    }

    /// Whether nothing has been kept, which clippy insists on beside [`Recorder::len`] and which a test for
    /// "the analysis emitted nothing" wants anyway.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many were pushed out by the cap. A nonzero here means the recorder was the wrong tool, not that
    /// the bus lost something.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Subscriber for Recorder {
    fn on_event(&self, event: &Event) {
        let mut guard = self
            .kept
            .lock()
            .unwrap_or_else(poisoned::PoisonError::into_inner);
        guard.push_back(event.clone());
        while guard.len() > self.cap {
            guard.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Which kinds a subscriber wants, expressed as the two shapes a reviewer will actually type in
/// `fathom.toml`: an exact kind (`detector.finished`) or a family (`analysis.*`).
///
/// An empty filter matches everything, which is the opposite of the intuitive reading and deliberate:
/// `events.on = []` in a config file means "I did not narrow anything", and a run that emitted nothing
/// because an empty list was read as "match nothing" is the worse failure by a mile.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    patterns: Vec<String>,
}

impl Filter {
    /// The filter that lets everything through.
    #[must_use]
    pub fn all() -> Self {
        Filter {
            patterns: vec!["*".to_owned()],
        }
    }

    /// Parse a list of patterns, returning the filter and one sentence per pattern this build could not
    /// use. A bad pattern never narrows the filter for the good ones — the same rule `detectors.suppress`
    /// follows, for the same reason: a typo should cost the reader a notice, not their whole stream.
    #[must_use]
    pub fn parse<S, I>(list: I) -> (Self, Vec<String>)
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        let mut patterns = Vec::new();
        let mut notices = Vec::new();
        for entry in list {
            let pattern = entry.as_ref().trim();
            if pattern.is_empty() {
                notices.push(
                    "an empty `events.on` entry matches nothing and was ignored; the shapes are \
                     `detector.finished` or `analysis.*`"
                        .to_owned(),
                );
                continue;
            }
            if !pattern.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '*' | '+')
            }) {
                notices.push(format!(
                    "`{pattern}` is not a kind pattern: lowercase, digits, `.`, `_`, `*` and `+`, \
                     with at most one trailing `*`"
                ));
                continue;
            }
            patterns.push(pattern.to_owned());
        }
        (Filter { patterns }, notices)
    }

    /// The patterns as given, for a report that prints its own knobs.
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Whether this kind survives the filter.
    #[must_use]
    pub fn allows(&self, kind: &Kind) -> bool {
        if self.patterns.is_empty() || self.patterns.iter().any(|p| p == "*") {
            return true;
        }
        self.patterns.iter().any(|pattern| {
            if let Some(family) = pattern.strip_suffix(".*") {
                kind.family() == family
            } else if let Some(prefix) = pattern.strip_suffix('*') {
                kind.as_str().starts_with(prefix)
            } else if let Some(suffix) = pattern.strip_prefix('+') {
                kind.as_str() == suffix
            } else {
                kind.as_str() == pattern
            }
        })
    }
}
