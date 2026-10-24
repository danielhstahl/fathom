//! The event stream's IO half: who writes it, what lands in it, and how to read it back (`e6.7`,
//! `reco-raw`).
//!
//! [`fathom_core::events`] owns the *shape* of an analysis event and the bus that hands it to subscribers;
//! it may not open a file or read a clock (ADR-002 §4.1). This module is the half that may: it reads the
//! `[events]` table, opens the file, filters, redacts, and appends. That split is the reason a test in
//! `fathom-analyze` can assert on a lifecycle without a filesystem and the reason `fathom watch` can hold a
//! bus for a week without one.
//!
//! # Where the stream goes
//!
//! Three destinations, in the order a reviewer would reach for them:
//!
//! * **a file**, when `[events] enabled = true` (or `--events <path>`): one JSON object per line, appended,
//!   under the repository. `fathom events` reads it back.
//! * **stderr**, when `--events -`: the same lines, live, while the verb runs. Chosen over stdout because
//!   `--json` promises stdout holds exactly one document, and a stream that broke that promise would be a
//!   stream nobody could pipe anywhere.
//! * **nowhere**, which is the default: a run that switches on nothing writes nothing, and says nothing about
//!   it (ADR-001 §4.6 — the product does not write files a repository did not ask for).
//!
//! ```text
//! $ fathom analyze HEAD --json --events - 2>/dev/null | head -2
//! {"contract":"fathom-analysis-events","schema_version":1,"kind":"run.started","seq":1,"run":"analyze-1",…
//! {"contract":"fathom-analysis-events","schema_version":1,"kind":"analysis.started","seq":2,"run":"analy…
//! ```
//!
//! # Two refusals
//!
//! A repository's config may not point the stream outside the repository, and every line is redacted before
//! it is written. The first is the rule `paths.*` already follows for a cloned `fathom.toml` — a file that
//! arrives with `git clone` does not get to choose where on your machine it writes. The second is
//! [`crate::sanitize`]'s rule for anything that leaves the process: an event that carries an absolute build
//! path or a secret-shaped string is an event that turns a CI log into a disclosure.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use fathom_core::events::{self, Bus, Event, Filter, Subscriber, Subscription};
use fathom_core::exit::ExitCode;

use crate::extens::Extensibility;
use crate::sanitize::Redactor;

/// The default file the stream goes to, when `[events]` is on and says nothing.
pub const DEFAULT_PATH: &str = ".fathom/analysis-events.jsonl";

/// What `[events]` and `--events` resolved to.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether a run writes the stream at all.
    pub enabled: bool,
    /// Where it writes. `-` means stderr, and is the only value that is not a path.
    pub destination: Destination,
    /// Which kinds survive.
    pub filter: Filter,
    /// One line per value that was refused, and what stood instead.
    pub notices: Vec<String>,
}

/// Where the stream goes.
#[derive(Debug, Clone)]
pub enum Destination {
    /// Append to this file under the repository.
    File(PathBuf),
    /// Write to stderr as the run happens.
    Stderr,
}

/// Read `[events]` and `paths.analysis_events` out of a repository's `fathom.toml`, then apply `--events`.
///
/// The `--events` flag wins over the file, which wins over the default — the same precedence every other key
/// in this product uses, and the reason it is worth restating: a reviewer debugging a repository whose
/// config turned events off still has to be able to turn them on for one run.
#[must_use]
pub fn config(root: &Path, cli_file: Option<&str>) -> Config {
    let mut notices = Vec::new();
    let text = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();
    let document = text.parse::<toml_edit::DocumentMut>().ok();
    let mut enabled = false;
    let mut configured_path: Option<String> = None;
    let mut patterns: Vec<String> = Vec::new();
    if let Some(document) = document.as_ref() {
        if let Some(table) = document.get("events").and_then(toml_edit::Item::as_table) {
            if let Some(value) = table.get("enabled").and_then(toml_edit::Item::as_value) {
                match value.as_bool() {
                    Some(flag) => enabled = flag,
                    None => notices.push(
                        "events.enabled is not a boolean; events stay off for this repository"
                            .to_owned(),
                    ),
                }
            }
            if let Some(on) = table.get("on").and_then(toml_edit::Item::as_array) {
                for entry in on.iter() {
                    if let Some(pattern) = entry.as_str() {
                        patterns.push(pattern.to_owned());
                    } else {
                        notices.push(
                            "events.on holds a non-string entry, which is not a kind pattern"
                                .to_owned(),
                        );
                    }
                }
            }
        }
        if let Some(path) = document
            .get("paths")
            .and_then(toml_edit::Item::as_table)
            .and_then(|table| table.get("analysis_events"))
            .and_then(toml_edit::Item::as_value)
            .and_then(toml_edit::Value::as_str)
        {
            configured_path = Some(path.to_owned());
        }
    }

    let requested = cli_file.or(configured_path.as_deref());
    let destination = match requested {
        Some("-") => Destination::Stderr,
        Some(path) => {
            let candidate = Path::new(path);
            let absolute = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                root.join(candidate)
            };
            if !stays_inside(root, &absolute) {
                notices.push(format!(
                    "`{path}` points outside the repository, and a cloned fathom.toml does not get to \
                     choose where this process writes; the default ({DEFAULT_PATH}) stood"
                ));
                Destination::File(root.join(DEFAULT_PATH))
            } else {
                Destination::File(absolute)
            }
        }
        None => Destination::File(root.join(DEFAULT_PATH)),
    };
    // `--events` on the command line is an explicit request for this run, whatever the file said.
    if cli_file.is_some() {
        enabled = true;
    }
    let (filter, filter_notices) = Filter::parse(&patterns);
    notices.extend(filter_notices);
    Config {
        enabled,
        destination,
        filter,
        notices,
    }
}

/// Whether `candidate` is inside `root`, which is the rule `paths.*` already follows and for the same
/// reason: a `fathom.toml` that arrives with `git clone` does not get to choose where on your machine this
/// process writes.
///
/// Canonicalising is the only honest answer — a `..` in the middle, a symlink at the front, and a relative
/// path against a relative root are three ways a lexical string comparison gets this wrong — but the stream
/// file usually does not exist yet, so the nearest ancestor that *does* exist is what gets resolved and the
/// last component is checked for hiding a `..` of its own.
fn stays_inside(root: &Path, candidate: &Path) -> bool {
    let base = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    if let Ok(real) = std::fs::canonicalize(&absolute) {
        return real.starts_with(&base);
    }
    let parent = absolute
        .parent()
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf);
    let real_parent = std::fs::canonicalize(&parent).unwrap_or_else(|_| parent.clone());
    let last_is_a_name = absolute
        .file_name()
        .is_some_and(|name| name != "." && name != "..");
    real_parent.starts_with(&base) && last_is_a_name
}

/// A subscriber that appends the stream somewhere, filters what it is not asked for, and redacts what it
/// writes.
struct Sink {
    destination: Mutex<SinkKind>,
    filter: Filter,
    redactor: Redactor,
    written: AtomicU64,
    refused: AtomicU64,
}

enum SinkKind {
    File(std::fs::File),
    Stderr,
}

impl Subscriber for Sink {
    fn on_event(&self, event: &Event) {
        if !self.filter.allows(&event.kind) {
            return;
        }
        // Redaction is per line, at the door, because an event that reached a subscriber has already left
        // the code that made it and nothing else is standing between it and a CI log.
        let mut line = event.clone();
        if let Some(note) = &line.note {
            line.note = Some(self.redactor.text(note));
        }
        if let Some(subject) = &line.subject {
            line.subject = Some(self.redactor.text(subject));
        }
        if let Some(repository) = &line.repository {
            line.repository = Some(self.redactor.text(repository));
        }
        let Ok(text) = line.to_line() else {
            self.refused.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let mut guard = match self.destination.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let written = match &mut *guard {
            SinkKind::File(file) => writeln!(file, "{text}").and_then(|()| file.flush()),
            SinkKind::Stderr => writeln!(std::io::stderr(), "{text}"),
        };
        match written {
            Ok(()) => {
                self.written.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                self.refused.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Open the file the stream goes to, with the sink that writes it.
///
/// The parent directory is created because `[events] enabled = true` is a request for a stream, and a run
/// that refused to write one into a repository that has no `.fathom/` yet would make the reviewer do the
/// part of the job the switch was supposed to do for them.
fn open_stream(
    path: &Path,
    config: &Config,
    redactor: &Redactor,
) -> Result<Arc<Sink>, std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    Ok(Arc::new(Sink {
        destination: Mutex::new(SinkKind::File(file)),
        filter: config.filter.clone(),
        redactor: redactor.clone(),
        written: AtomicU64::from(0),
        refused: AtomicU64::from(0),
    }))
}

/// One verb's run on the bus: an id every event of this run carries, and the clock this crate is allowed to
/// read.
pub struct Run {
    /// The bus. Cheap to hold and cheap to publish on when nothing is listening.
    pub bus: Bus,
    /// This run's id, stamped into every event it emits.
    pub id: String,
    /// The repository, as a label — a name, never an absolute path.
    pub repository: String,
    /// What produced the events.
    pub producer: String,
    /// Whether a terminal event was written, so [`Drop`] can tell a finished run from one that left through
    /// an early `return`.
    terminal: std::cell::Cell<bool>,
    /// The file (or stderr) subscriber, kept alive for exactly as long as the run is.
    ///
    /// A [`Subscription`] unsubscribes when it is dropped, so a bus that is dropped — as a whole bus is, when
    /// a run ends — must be the thing holding the handle. This is not ceremony: the first version of this
    /// struct let the subscription go at the end of [`Run::start`], which meant the bus had no subscribers,
    /// which meant the stream file was created and then never written to, which is a feature that looks like
    /// it works right up until somebody reads the file.
    ///
    /// Read by nothing, and that is the field's job: its `Drop` is the code.
    #[expect(
        dead_code,
        reason = "the field's Drop is what keeps the subscriber attached"
    )]
    subscriptions: Vec<Subscription>,
}

/// The last event a run that never said how it ended gets: a stream whose final line is `run.started` is a
/// stream that records a crash, and saying so is the difference between a log and a guess.
impl Drop for Run {
    fn drop(&mut self) {
        if !self.terminal.get() {
            self.terminal.set(true);
            self.publish(
                Event::new(events::kind::RUN_FAILED, &self.id)
                    .outcome("error")
                    .note("the verb returned without recording an outcome"),
            );
        }
    }
}

impl Run {
    /// Start a run of `verb` over `root`, wiring whatever the config asked for.
    ///
    /// The returned notices are the ones a report should print: a `events.path` that escaped the repository,
    /// an `events.on` pattern that would not parse, a file that could not be opened. None of them stop the
    /// verb, and none of them are swallowed.
    #[must_use]
    pub fn start(verb: &str, root: &Path, extens: &Extensibility) -> (Run, Vec<String>) {
        let config = config(root, extens.events.as_deref());
        let mut notices = config.notices.clone();
        let bus = Bus::new();
        let mut subscriptions = Vec::new();
        if config.enabled {
            let redactor = Redactor::for_root(root);
            let sink = match &config.destination {
                Destination::File(path) => match open_stream(path, &config, &redactor) {
                    Ok(sink) => Some(sink),
                    Err(err) => {
                        notices.push(format!(
                            "the event stream `{}` could not be opened ({err}), so this run emitted \
                             nothing; the analysis is unaffected",
                            path.display()
                        ));
                        None
                    }
                },
                Destination::Stderr => Some(Arc::new(Sink {
                    destination: Mutex::new(SinkKind::Stderr),
                    filter: config.filter.clone(),
                    redactor: redactor.clone(),
                    written: AtomicU64::from(0),
                    refused: AtomicU64::from(0),
                })),
            };
            if let Some(sink) = sink {
                // The only way this refuses is a bus that already has 16 subscribers, and this bus was
                // constructed three lines ago with none — so the refusal branch is a notice, not a panic,
                // and it says what a reviewer would otherwise have to debug: a stream that stayed empty.
                match bus.subscribe(sink as Arc<dyn Subscriber>) {
                    Some(subscription) => subscriptions.push(subscription),
                    None => notices.push(
                        "the event stream's subscriber was refused by the bus, so this run emitted                          nothing; the analysis is unaffected"
                            .to_owned(),
                    ),
                }
            }
        }
        let repository = root
            .canonicalize()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| ".".to_owned());
        let run = Run {
            bus,
            id: format!("{verb}-{}", crate::clock::now_millis()),
            repository,
            producer: format!("fathom {}", env!("CARGO_PKG_VERSION")),
            terminal: std::cell::Cell::new(false),
            subscriptions,
        };
        run.publish(Event::new(events::kind::RUN_STARTED, &run.id).note(format!("{verb} started")));
        (run, notices)
    }

    /// Stamp one event with what this run knows and hand it to the bus.
    pub fn publish(&self, mut event: Event) {
        event.run = self.id.clone();
        event.ts = Some(fathom_core::time::Timestamp::from_unix_micros(
            crate::clock::now_millis().saturating_mul(1_000),
        ));
        event.repository = Some(self.repository.clone());
        event.producer = Some(self.producer.clone());
        if event.schema_version == 0 {
            event.schema_version = events::SCHEMA_VERSION;
        }
        self.bus.publish(event);
    }

    /// Say which detectors this run was asked to run, in the stream, so a reader can tell a scan that found
    /// nothing from a scan that never ran.
    pub fn selection(&self, describe: &str) {
        self.publish(Event::new(events::kind::SELECTION_RESOLVED, &self.id).note(describe));
    }

    /// The scan of one episode began, with how many detectors were asked.
    pub fn analysis_started(&self, episode: &str, detector_count: usize) {
        self.publish(
            Event::new(events::kind::ANALYSIS_STARTED, episode)
                .count("detectors", count(detector_count)),
        );
    }

    /// The scan of one episode is over: the counts, so a reader can check them against the rows.
    pub fn analysis_finished(&self, report: &fathom_analyze::detectors::Report) {
        self.publish(
            Event::new(events::kind::ANALYSIS_FINISHED, &report.episode_id)
                .outcome("ok")
                .count("findings", count(report.findings.len()))
                .count("detectors", count(report.runs.len()))
                .count("gaps", count(report.gaps.len())),
        );
    }

    /// What each detector did: its state, its count, and the sentence that explains a state a reader might
    /// otherwise read as "nothing" (`e6.7`). A detector that did not answer gets `detector.skipped` rather
    /// than `detector.finished`, because "disabled" and "ran, found nothing" are different answers and a
    /// subscriber that routes on the kind must not have to parse a note to tell them apart.
    pub fn detectors(&self, report: &fathom_analyze::detectors::Report) {
        for run in &report.runs {
            let name = outcome_name(run.outcome);
            let kind = if name == "ran" {
                events::kind::DETECTOR_FINISHED
            } else {
                events::kind::DETECTOR_SKIPPED
            };
            self.publish(
                Event::new(kind, &report.episode_id)
                    .detector(run.detector)
                    .outcome(&name)
                    .count("findings", count(run.findings))
                    .count("hidden", count(run.hidden))
                    .count("suppressed", count(run.suppressed)),
            );
        }
    }

    /// What each finding said — one event each, carrying what a subscriber needs to route it: severity,
    /// subject, and the detector that stands behind it.
    pub fn findings(&self, report: &fathom_analyze::detectors::Report) {
        for finding in &report.findings {
            self.publish(
                Event::new(events::kind::FINDING_RAISED, &finding.episode_id)
                    .detector(&finding.detector_id)
                    .severity(severity_name(finding.severity))
                    .subject(finding.subject.clone())
                    .note(finding.message.clone())
                    .count("confidence", (finding.confidence * 1000.0).round() as i64),
            );
        }
    }

    /// What the scan refused, and why.
    pub fn notice(&self, episode: &str, notice: &str) {
        self.publish(Event::new(events::kind::NOTICE, episode).note(notice));
    }

    /// The run is over, with the exit code it is leaving with.
    pub fn finished(&self, code: i32) {
        self.terminal.set(true);
        self.publish(
            Event::new(events::kind::RUN_FINISHED, &self.id)
                .outcome(if code == 0 { "ok" } else { "error" })
                .count("code", i64::from(code)),
        );
    }

    /// The run gave up, with the sentence that says why.
    pub fn failed(&self, reason: &str) {
        self.terminal.set(true);
        self.publish(
            Event::new(events::kind::RUN_FAILED, &self.id)
                .outcome("error")
                .note(reason),
        );
    }
}

/// `fathom events`: read the stream back.
///
/// The file is append-only JSONL, so the reader is the same one the bus's own tests use — and it inherits
/// its rule: a line it cannot read is reported and skipped, never fatal. A stream written by a newer build
/// is a stream an older `fathom events` can still print.
pub fn read(
    root: &Path,
    path: Option<&str>,
    tail: Option<usize>,
    on: &[String],
    as_json: bool,
    explain: bool,
) -> eyre::Result<ExitCode> {
    let config = config(root, path);
    let target = match &config.destination {
        Destination::File(path) => path.clone(),
        Destination::Stderr => root.join(DEFAULT_PATH),
    };
    if explain {
        crate::output::line(format_args!("stream: {}", describe_path(root, &target)));
        crate::output::line(format_args!("enabled: {}", config.enabled));
        crate::output::line(format_args!("on: {}", config.filter.patterns().join(",")));
        for notice in &config.notices {
            crate::output::line(format_args!("notice: {notice}"));
        }
        return Ok(ExitCode::Ok);
    }
    let text = match std::fs::read_to_string(&target) {
        Ok(text) => text,
        Err(_) => {
            eprintln!(
                "acme: nothing at {} yet — `[events] enabled = true` (or `--events -`) makes a run \
                 write it",
                describe_path(root, &target)
            );
            return Ok(ExitCode::Ok);
        }
    };
    let stream = events::read_stream(&text);
    let (filter, notices) = if on.is_empty() {
        (config.filter.clone(), Vec::new())
    } else {
        Filter::parse(on)
    };
    for notice in notices {
        crate::output::problem(format_args!("fathom events: {notice}"));
    }
    let mut kept: Vec<&Event> = stream
        .events
        .iter()
        .filter(|event| filter.allows(&event.kind))
        .collect();
    if let Some(tail) = tail
        && kept.len() > tail
    {
        kept.drain(..kept.len() - tail);
    }
    for event in &kept {
        if as_json {
            if let Ok(line) = event.to_line() {
                crate::output::line(format_args!("{line}"));
            }
        } else {
            crate::output::line(format_args!("{}", render(event)));
        }
    }
    for ignored in &stream.ignored {
        crate::output::problem(format_args!("fathom events: ignored {ignored}"));
    }
    if !as_json && kept.is_empty() {
        crate::output::problem(format_args!(
            "fathom events: {} events at {}, {} of them kinds this build knows",
            stream.events.len(),
            describe_path(root, &target),
            stream
                .events
                .iter()
                .filter(|event| event.kind.is_declared())
                .count()
        ));
    }
    Ok(ExitCode::Ok)
}

/// One line of the text rendering: time, kind, and whatever identifies the thing it is about.
#[must_use]
pub fn render(event: &Event) -> String {
    let mut out = format!(
        "{:18} {}",
        event.kind,
        event.outcome.as_deref().unwrap_or("-")
    );
    if let Some(episode) = &event.episode {
        out.push_str(&format!(" {episode}"));
    }
    if let Some(detector) = &event.detector {
        out.push_str(&format!(" {detector}"));
    }
    if let Some(subject) = &event.subject {
        out.push_str(&format!(" · {subject}"));
    }
    if let Some(severity) = &event.severity {
        out.push_str(&format!(" [{severity}]"));
    }
    let counts: Vec<String> = event
        .counts
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    if !counts.is_empty() {
        out.push_str(&format!(" {}", counts.join(" ")));
    }
    if let Some(note) = &event.note {
        out.push_str(&format!(" — {note}"));
    }
    out
}

/// A path as the reviewer would write it, not as the process resolved it.
fn describe_path(root: &Path, target: &Path) -> String {
    Redactor::for_root(root).path(target)
}

/// A count that fits the wire format without pretending a `usize` is signed.
fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// [`Outcome`] as the stream spells it, which is the serde spelling so the two never part.
fn outcome_name(outcome: fathom_analyze::detectors::Outcome) -> String {
    format!("{outcome:?}").to_lowercase()
}

/// [`Severity`] as the stream spells it.
fn severity_name(severity: fathom_analyze::detectors::Severity) -> String {
    format!("{severity:?}").to_lowercase()
}
