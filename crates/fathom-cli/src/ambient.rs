//! `fathom ambient` — the glance pane in a process of its own (`reco-8bx`).
//!
//! The arrangement this verb exists for is physical: a coding agent on the left, this pane on the right, a
//! person reading the agent and glancing at the pane. That puts three constraints on a process, and this
//! module is the answer to them:
//!
//! * **It fits the split.** The pane is drawn at whatever the terminal is, refuses below the 40×3 floor
//!   ADR-001 §4.1 promises, and prints the same lines the shell's `a` view prints — the copy is
//!   [`fathom_tui::ambient`]'s, so the two cannot drift.
//! * **It costs nothing while nothing happens.** The poll ([`Reader::probe`]) is a handful of `stat` calls
//!   and one ref read; the store is opened only when one of them moved, and the screen is written only when
//!   the *text* would be different. A quiet repository therefore produces one write and no reads, and
//!   `--self-check` measures the result with the kernel's own numbers ([`process_cpu`]) rather than
//!   claiming it.
//! * **It is the current truth, or it says it is not.** When nothing else is watching the repository this
//!   verb indexes new commits itself — the same [`fathom_ingest::watch::Job`] `fathom watch` runs, driven by
//!   the cheap probe — so a new high-risk episode reaches the screen in under a second without a second
//!   process. When a watcher *is* running this verb does not duplicate its work: it reads the heartbeat,
//!   prints the watcher's own phase, and stays a reader. `--no-index` forces the reader role, and a
//!   repository whose index does not know `HEAD` says `HEAD not indexed` on the face of the pane rather
//!   than looking calm while being wrong.
//!
//! # What the composition root owns here
//!
//! The clock, the store and the terminal — nothing else. The pane's copy and the ranking live below
//! (`fathom-tui` draws, `fathom-analyze` ranks) and this file reads and hands over (ADR-002 §4.3). One
//! clock read per poll, at the edge, is ADR-002 §4.4; [`Reader::glance`] is the only place `now` is asked
//! for.
//!
//! # The ways out
//!
//! `q` leaves with 0. `<Enter>` hands the same terminal to the shell with the pane's episode selected —
//! ADR-001 §4.1's "one keypress drills in", made literal through the session keys the shell already
//! restores. A signal leaves the way `fathom watch` does. A refused screen is an error with the terminal
//! already restored.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fathom_core::exit::ExitCode;
use fathom_core::time::{Clock as _, Timestamp};
use fathom_tui::ambient::{
    self, End, Glance, IndexState, Latest, Need, QueueState, Reason, WatchState,
};

use crate::output;
use crate::verbs::{AmbientArgs, Globals};

/// The idle budget, as a fraction of one core: the same number as `watch.idle_cpu.pct`
/// (`benchmarks/budgets.json`), because a pane that sits beside a watcher must not cost more than it.
const CPU_BUDGET: f64 = 0.01;

/// An index nobody refreshed for this long, with nothing watching, is a pane lying by omission.
const STALE_AFTER_SECS: i64 = 600;

/// A heartbeat older than this has stopped being a promise and become a fossil.
const HEARTBEAT_LIVE_SECS: i64 = 60;

/// The machine contract `--json` promises, spelled the way `fathom watch --json` spells its own.
const CONTRACT: &str = "fathom.cli.ambient/1";

/// Run `fathom ambient`.
pub fn run(args: &AmbientArgs, globals: &Globals) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    // The same refusal every other verb makes at the same code, so a script that wraps `fathom watch` and
    // `fathom ambient` branches once.
    fathom_ingest::repo::Repository::discover(root).map_err(crate::verbs::failed)?;
    if args.interval_ms == 0 {
        return Err(crate::verbs::Pending::usage(
            "`--interval-ms` must be at least 1",
            "a poll of 0 is a busy loop, which is the one thing this pane may not be",
        )
        .into());
    }
    let mut reader = Reader::open(root, args, Duration::from_millis(args.interval_ms))?;

    // Two separate questions, and conflating them is how a piped `--json` run prints one object and exits:
    // *is there a screen to take* (no: a pipe, `--plain`, `--no-alt-screen` aside — that flag takes the
    // screen but inline), and *does the run keep running after the first render* (`--json` and
    // `--self-check` follow changes; `--once` never does).
    let no_screen = args.plain || globals.plain || !std::io::stdout().is_terminal();
    let stream = !args.once && (args.json || args.self_check);
    if args.once || (no_screen && !stream) {
        // A pipe is the other contract, not a degraded one (ADR-006 §6.7): no screen is taken, one render
        // goes to stdout, and the exit code is the whole answer.
        if no_screen && !args.once && !args.plain && !globals.plain {
            output::problem(format_args!(
                "fathom: ambient: stdout is not a terminal, so the pane was drawn once as text;                  `--json` streams it as it changes (ADR-006 §6.7)"
            ));
        }
        let glance = reader.glance(true);
        if args.json {
            output::line(format_args!("{}", glance_json(&glance, args)));
        } else {
            for line in ambient::text_lines(&glance, ambient::Mode::default(), width(args)) {
                output::line(format_args!("{line}"));
            }
        }
        for note in &reader.notes {
            output::problem(format_args!("fathom: ambient: {note}"));
        }
        return Ok(ExitCode::Ok);
    }

    if stream {
        return follow(args, &mut reader);
    }

    let probe = fathom_tui::terminal::Probe::from_env_with(|key| std::env::var(key).ok());
    let (theme, notices) = crate::theme::resolve(globals, &probe);
    for notice in &notices {
        output::problem(format_args!("fathom: {notice}"));
    }
    let options = ambient::RunOptions {
        poll: reader.poll,
        alt_screen: !globals.no_alt_screen,
        max_draws: None,
        theme,
        term: probe,
    };
    let report = ambient::run(&mut reader, &options).map_err(|err| {
        crate::verbs::pending(
            ExitCode::Error,
            format!("the glance pane could not be drawn: {err}"),
            Some(
                "`fathom ambient --once` prints the same pane as text with no terminal taken"
                    .to_owned(),
            ),
        )
    })?;
    output::problem(format_args!("fathom: ambient: {}", report.summary()));
    if let End::Drill { episode } = &report.end {
        // The same repository the pane was reading, not whatever `pwd` says: `fathom ambient --root` exists
        // so a glance can watch a second checkout, and `<Enter>` that opened the wrong one would be a
        // worse surprise than a cursor in the wrong place.
        return crate::tui::dispatch_focus(
            globals,
            episode.as_deref(),
            root,
            crate::tui::Numbers::Line,
        );
    }
    Ok(ExitCode::Ok)
}

/// The stream modes. `--json` emits one object per change; `--self-check` adds the counters the second
/// acceptance criterion is about. Neither takes a screen, so both run under a pipe, in CI, and on a
/// repository with no terminal at all.
fn follow(args: &AmbientArgs, reader: &mut Reader) -> eyre::Result<ExitCode> {
    let started = Instant::now();
    let cpu_start = process_cpu();
    let mut emits = 0_u64;
    let mut polls = 0_u64;
    let mut last: Option<String> = None;
    let mut last_snapshot: Option<Glance> = None;
    loop {
        let glance = reader.glance(last.is_none());
        polls += 1;
        // The same test the interactive loop uses, on the same text: a repeated truth is not news, and in
        // `--json` a repeated object is worse than newsless — it is a stream a script has to dedupe.
        let printed =
            ambient::text_lines(&glance, ambient::Mode::default(), width(args)).join("\n");
        if last.as_deref() != Some(printed.as_str()) {
            last = Some(printed);
            emits += 1;
            // A self-check run's output is its one summary object: printing the pane once per change on the
            // way would make the counters unfindable in a pipe, and the counters are the whole point of the
            // flag.
            if args.self_check {
                last_snapshot = Some(glance.clone());
            }
            let value = glance_json(&glance, args);
            if args.self_check {
                // Counted, not printed: see the note above.
            } else if args.json {
                output::line(format_args!("{value}"));
            } else if let Some(lines) = value["lines"].as_array() {
                for line in lines {
                    output::line(format_args!(
                        "{}",
                        line.as_str().unwrap_or_default().trim_end()
                    ));
                }
            }
        }
        if reader.expired() {
            break;
        }
        std::thread::sleep(reader.poll);
    }
    let elapsed = started.elapsed();
    if !args.self_check {
        return Ok(ExitCode::Ok);
    }
    let cpu = match (cpu_start, process_cpu()) {
        (Some(from), Some(to)) => Some(to.saturating_sub(from)),
        _ => None,
    };
    let share = cpu.map_or(0.0, |busy| {
        micros(busy) / micros(elapsed).max(f64::MIN_POSITIVE)
    });
    let report = serde_json::json!({
        "contract": CONTRACT,
        "self_check": true,
        "root": args.root,
        "seconds": (micros(elapsed) / 1_000.0).round() / 1_000.0,
        "polls": polls,
        "emissions": emits,
        "redraws": emits,
        "redraws_per_poll": if polls == 0 { 0.0 } else { emits as f64 / polls as f64 },
        "cpu_ms": cpu.map(|busy| busy.as_millis() as u64),
        // Rounded to two decimals: the gate is 1 %, and a number that pretends to five decimals of a
        // sub-millisecond measurement is decoration.
        "cpu_pct": cpu.map(|_| (share * 10_000.0).round() / 100.0),
        "budget_pct": CPU_BUDGET * 100.0,
        "budget_note": if cpu.is_some() {
            "measured from this process's own user+system time (/proc/self/stat)"
        } else {
            "not measured on this platform, so the budget is unverified — not passed"
        },
        "over_budget": cpu.is_some_and(|_| share > CPU_BUDGET),
        "indexing_here": reader.index_here,
        "notes": reader.notes,
        "caught_up": last_snapshot.as_ref().is_some_and(Glance::caught_up),
        "last": last_snapshot.as_ref().map(|glance| glance_json(glance, args)),
    });
    output::line(format_args!("{report}"));
    // A blown budget exits with the code CI already branches on for a blown budget, rather than printing a
    // number and hoping somebody reads it.
    Ok(if cpu.is_some_and(|_| share > CPU_BUDGET) {
        ExitCode::BudgetExceeded
    } else {
        ExitCode::Ok
    })
}

#[must_use]
fn micros(value: Duration) -> f64 {
    f64::from(u32::try_from(value.as_micros()).unwrap_or(u32::MAX))
}

/// The `--json` object: the pane's facts, plus the pane's own text.
///
/// Assembled here rather than derived from the renderer's own types for the same reason `fathom watch`'s
/// heartbeat is: a renamed internal field must not silently rename what a script reads (ADR-004 §5).
fn glance_json(glance: &Glance, args: &AmbientArgs) -> serde_json::Value {
    let lines = ambient::text_lines(glance, ambient::Mode::default(), width(args));
    let latest = glance.latest.as_ref().map(|latest| {
        serde_json::json!({
            "id": latest.id,
            "summary": latest.summary,
            "author": latest.author,
            "state": latest.state,
            "provisional": latest.provisional,
            "created_utc": latest.created.map(|stamp| stamp.as_unix_micros() / 1_000),
            "risk": latest.risk.as_ref().map(|risk| serde_json::json!({
                "score": risk.score,
                "model": risk.model,
                "rank": risk.rank,
                "ranked": risk.ranked,
                "surfaces": risk.surfaces,
            })),
        })
    });
    let counts = glance.queue.counts;
    serde_json::json!({
        "contract": CONTRACT,
        "stamp_utc": glance.now.as_unix_micros() / 1_000,
        "root": glance.root,
        "branch": glance.branch,
        "caught_up": glance.caught_up(),
        "attention": glance
            .reason()
            .map(|reason| serde_json::json!({ "kind": reason.kind.as_str(), "text": reason.text })),
        "drill": glance.drill,
        "latest": latest,
        "queue": {
            "known": glance.queue.known,
            "over_day": glance.queue.over_day,
            "counts": counts
                .map(|counts| serde_json::to_value(counts).unwrap_or(serde_json::Value::Null)),
        },
        "index": {
            "read": glance.index.read.name(),
            "epoch": glance.index.epoch,
            "status": glance.index.status,
            "episodes": glance.index.episodes,
            "last_run_utc": glance.index.last_utc,
            "head_indexed": glance.index.head_indexed,
        },
        "watch": {
            "here": glance.watch.here,
            "present": glance.watch.present,
            "alive": glance.watch.alive,
            "summary": glance.watch.summary,
            "age_ms": glance.watch.age.map(|age| age.as_millis() as u64),
        },
        "notes": glance.notes,
        "lines": lines,
    })
}

/// The width the pane is rendered at for a pipe: what the caller asked for, or ADR-006 §5.7's ideal.
fn width(args: &AmbientArgs) -> usize {
    args.width.unwrap_or(80).max(1)
}

// ---------------------------------------------------------------------------------------------
// the reader: the cheap probe, the store read, and the facts between them
// ---------------------------------------------------------------------------------------------

/// The index job where one is compiled in, and nothing where it is not.
#[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
type Job = crate::watch::IndexJob;

/// Everything the pane says, as of the last time the store was read.
#[derive(Default)]
struct Facts {
    index: IndexState,
    latest: Option<Latest>,
    queue: QueueState,
    /// The queue's top-ranked open episode: what the reason line prints and what `<Enter>` opens.
    top: Option<Top>,
    notes: Vec<String>,
}

impl Facts {
    /// What `<Enter>` should open: the thing to look at, falling back to the newest row, because a pane
    /// with nothing ranked still has a newest row worth reading.
    fn drill(&self) -> Option<String> {
        self.top
            .as_ref()
            .map(|top| top.episode.clone())
            .or_else(|| self.latest.as_ref().map(|latest| latest.id.clone()))
    }
}

/// The queue's first open row, in the three things the pane needs from it.
struct Top {
    kind: Need,
    text: String,
    episode: String,
}

/// A file's mtime and length, or "there is no such file".
///
/// `pub(crate)` because `fathom tui`'s own watcher asks the same question with the same cheap probe
/// (`reco-bpc`); two copies of "what moved" is two answers that can drift.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Stamp {
    exists: bool,
    modified: u64,
    len: u64,
}

pub(crate) fn stamp(path: &Path) -> Stamp {
    let Ok(meta) = std::fs::metadata(path) else {
        return Stamp::default();
    };
    Stamp {
        exists: true,
        modified: meta
            .modified()
            .ok()
            .and_then(|when| when.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs()),
        len: meta.len(),
    }
}

/// The cheap probe's answer: sizes, mtimes and `HEAD`, and nothing parsed.
///
/// `pub(crate)` for the same reason [`stamp`] is: the shell's watcher and the glance pane's reader are one
/// probe with two callers.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Signature {
    store: Stamp,
    heartbeat: Stamp,
    git_dir: Stamp,
    head: String,
}

impl Signature {
    /// The `HEAD` the probe read: an oid, or empty when there was no head to read.
    #[must_use]
    pub(crate) fn head(&self) -> &str {
        &self.head
    }

    /// Is `HEAD` itself what moved — the one change that means the index is now behind the work?
    #[must_use]
    pub(crate) fn head_moved(&self, before: &Signature) -> bool {
        self.head != before.head
    }

    /// Is there anything a re-read could put on screen that was not there? `HEAD` moved, or somebody
    /// wrote the store.
    ///
    /// Deliberately *not* a whole-struct compare: `.git` moves for reasons that change nothing a frame
    /// draws — `git status` refreshing the index, a stash, a fetch of a branch nobody is looking at — and
    /// a watcher that re-read 500 episodes and toasted about it on every one of those is a watcher that
    /// was never asked to be the interesting kind.
    #[must_use]
    pub(crate) fn worth_reading(&self, before: &Signature) -> bool {
        self.head != before.head || self.store != before.store
    }
}

/// The cheap probe, run against one repository: four `stat`s and one ref read, and nothing parsed.
///
/// Shared by the glance pane and by `fathom tui`'s watcher (`reco-bpc`) because both need the same answer
/// at the same price, and because the price is the whole reason either is allowed to ask.
pub(crate) fn probe(root: &Path, store_path: &Path, heartbeat: &Path) -> Signature {
    let repository = fathom_ingest::repo::Repository::discover(root).ok();
    let (head, git_dir) = match &repository {
        Some(repo) => (
            repo.head_oid()
                .ok()
                .flatten()
                .map(|oid| oid.to_string())
                .unwrap_or_default(),
            stamp(repo.git_dir()),
        ),
        None => (String::new(), Stamp::default()),
    };
    Signature {
        store: stamp(store_path),
        heartbeat: stamp(heartbeat),
        git_dir,
        head,
    }
}

/// Who reads the pane's facts, and how often it is allowed to.
struct Reader {
    root: PathBuf,
    store_path: PathBuf,
    heartbeat: PathBuf,
    poll: Duration,
    deadline: Option<Duration>,
    started: Instant,
    /// Whether this process is the one keeping the index current.
    index_here: bool,
    /// Why it is not, when it is not: said on the pane, never hidden in a flag.
    index_note: Option<String>,
    /// Everything worth telling the reviewer that is not a reason to look.
    notes: Vec<String>,
    signature: Signature,
    facts: Facts,
    watch: WatchState,
    head: String,
    #[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
    job: Job,
}

/// Where the watcher's heartbeat lives for one repository: beside its store, in `.fathom/watch/`.
///
/// `pub(crate)` because `fathom tui`'s watcher reads the same file to decide whether it is the one allowed
/// to index (`reco-bpc`), and two spellings of a path is two places a layout can move without one noticing.
#[must_use]
pub(crate) fn heartbeat_path(store_path: &Path) -> PathBuf {
    store_path.parent().map_or_else(
        || PathBuf::from("watch-status.json"),
        |dir| dir.join("watch").join("status.json"),
    )
}

impl Reader {
    fn open(root: &Path, args: &AmbientArgs, poll: Duration) -> eyre::Result<Reader> {
        let store_path = crate::session::store_path(root);
        let heartbeat = heartbeat_path(&store_path);
        let now = stamp(&heartbeat);
        let mut notes = Vec::new();
        let watcher_alive = heartbeat_is_live(&heartbeat);
        let mut index_here = !args.no_index;
        let mut index_note = None;
        if args.no_index {
            index_note = Some(
                "--no-index: this pane reads whatever the last index run left, and writes nothing"
                    .to_owned(),
            );
            index_here = false;
        } else if watcher_alive {
            index_note = Some(
                "a watcher is running here, so this pane reads the index instead of writing it"
                    .to_owned(),
            );
            index_here = false;
        }
        // A daemon is a watcher with an endpoint (`e6.3`), and the heartbeat above is exactly the fallback
        // a reader uses when there is no endpoint to ask. But a daemon started with `--no-status-file`
        // writes no heartbeat, and a pane that began indexing behind that daemon would be a second writer
        // of the same store: the socket is the authoritative question, so it is asked second rather than
        // never.
        #[cfg(feature = "watch")]
        if index_here && crate::daemon::serving(root) {
            index_here = false;
            index_note = Some(
                "a daemon is serving this repository, so this pane asks it instead of writing the index                  itself; `fathom status` is the report"
                    .to_owned(),
            );
        }
        if !cfg!(all(feature = "watch", feature = "sqlite-bundled")) && index_here {
            index_here = false;
            notes.push(
                "this binary cannot index from the pane — it needs the `watch` and `sqlite-bundled` \
                 features — so `fathom watch` or `fathom index` is what keeps this current"
                    .to_owned(),
            );
        }
        let _ = now;
        Ok(Reader {
            root: root.to_path_buf(),
            store_path,
            heartbeat,
            poll,
            deadline: args.deadline.map(Duration::from_secs),
            started: Instant::now(),
            index_here,
            index_note,
            notes,
            signature: Signature::default(),
            facts: Facts::default(),
            watch: WatchState::default(),
            head: String::new(),
            #[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
            job: crate::watch::IndexJob::for_root(
                root,
                crate::session::store_path(root),
                args.chunk_units,
            ),
        })
    }

    #[must_use]
    fn expired(&self) -> bool {
        self.deadline
            .is_some_and(|limit| self.started.elapsed() >= limit)
    }

    /// The pane's facts, re-read only when the cheap probe saw something move.
    fn glance(&mut self, force: bool) -> Glance {
        let now = Timestamp::from_unix_micros(crate::clock::SystemClock.now().as_unix_micros());
        let signature = self.probe();
        let moved = force || signature != self.signature;
        self.signature = signature;
        self.head = self.signature.head.clone();
        if moved {
            if self.index_here {
                self.index_new_commits();
            }
            self.facts = self.read(&now);
            self.watch = read_heartbeat(&self.heartbeat, now);
        }
        let mut glance = self.compose(&now);
        glance.watch.here = self.index_here;
        glance
    }

    /// The cheap probe: four `stat`s and one ref read, which is the whole reason the pane can ask several
    /// times a second and still cost nothing. Nothing here parses a file or opens the store.
    fn probe(&self) -> Signature {
        probe(&self.root, &self.store_path, &self.heartbeat)
    }

    /// Ask the job `fathom watch` runs to take what is new, in one bounded slice.
    #[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
    fn index_new_commits(&mut self) {
        use fathom_ingest::watch::{Interest, InterestReason, Job as _, Progress};
        // One slice per poll, so the pane's own budget bounds the expensive side exactly the way the
        // watcher's does — and the slice has to be re-armed every time, because the deadline is absolute.
        self.job
            .start_slice(self.poll.max(Duration::from_millis(250)));
        let interest = Interest {
            reason: InterestReason::Commits,
            head: self.head.clone(),
            changes: Vec::new(),
            index_touched: false,
            replaced: false,
            signals: Vec::new(),
        };
        let mut sink = |_: Progress| {};
        if let Err(err) = self.job.refresh(&interest, &mut sink) {
            // A failed slice is a note on the pane, not a dead pane: the reviewer keeps what was on screen
            // and is told why it stopped moving.
            self.notes.push(format!(
                "the pane's own index run failed: {}",
                err.message()
            ));
        }
    }

    #[cfg(not(all(feature = "watch", feature = "sqlite-bundled")))]
    fn index_new_commits(&mut self) {}

    /// The store read. Only ever called when the probe moved, so its cost belongs to the change it
    /// describes and not to the wait between changes.
    #[cfg(feature = "sqlite-bundled")]
    fn read(&self, now: &Timestamp) -> Facts {
        use fathom_store::episode::{EpisodeQuery, ReviewState};
        use fathom_store::spine::Store;
        use fathom_tui::timeline::Episode;

        let mut out = Facts::default();
        let facts = crate::tui::index_facts(&self.root);
        out.index.read = facts.read;
        out.index.epoch = facts.epoch;
        out.index.status = facts.status.clone();
        out.index.episodes = facts.episodes;
        if !facts.is_present() {
            return out;
        }
        let Ok(store) = Store::open(&self.store_path) else {
            out.index.read = fathom_tui::index::Read::Unreadable;
            return out;
        };
        out.index.last_utc = store
            .run_summary()
            .ok()
            .and_then(|runs| runs.last)
            .and_then(|last| last.finished_utc);
        out.index.head_indexed = if self.head.is_empty() {
            None
        } else {
            store
                .known_oids(std::slice::from_ref(&self.head))
                .ok()
                .map(|known| known.contains(&self.head))
        };
        out.queue.over_day = store
            .count_episodes(&EpisodeQuery {
                state: Some(ReviewState::Unread),
                until_utc: Some(now.as_unix_micros() / 1_000 - 86_400_000),
                ..EpisodeQuery::default()
            })
            .ok()
            .and_then(|count| usize::try_from(count).ok());
        // The rows, the ranking and the counts, from the read the shell already makes: the pane may not
        // have its own idea of what is riskiest, and this is the only ordering in the product that can
        // explain itself (`reco-wsj`).
        let Some(batch) = crate::episodes::load(&self.root, &facts) else {
            out.queue.known = true;
            return out;
        };
        let setup = crate::queue::setup(
            &self.root,
            &crate::queue::actor(&self.root),
            &crate::tui::dismiss_reasons(&self.root).0,
            usize::try_from(facts.episodes.unwrap_or(0)).unwrap_or(usize::MAX),
        );
        let state = setup.state();
        let rows: Vec<&Episode> = batch.episodes.iter().collect();
        let ranking = fathom_tui::queue::ranking(&rows, &state, *now, facts.epoch.unwrap_or(0));
        out.notes = setup.notices;
        out.queue.known = true;
        out.queue.counts = Some(ranking.counts);
        let newest_id = batch.episodes.first().map(|episode| episode.id.clone());
        out.latest = batch.episodes.first().map(|episode| Latest {
            id: episode.id.clone(),
            summary: episode.summary.clone(),
            author: episode.author.as_str().to_owned(),
            created: Some(episode.created),
            state: episode.state.as_str().to_owned(),
            // Provisional means uncommitted work, and it is never folded into a committed count
            // (docs/concepts/episode.md). The kind is a column the page read does not carry, so it is
            // asked for directly rather than guessed from the summary.
            provisional: store
                .episode(newest_id.unwrap_or_default().as_str())
                .ok()
                .flatten()
                .is_some_and(|row| row.kind == "provisional"),
            risk: episode.risk().cloned(),
        });
        if let Some(top) = ranking.order.iter().find(|placement| placement.open) {
            let episode = batch
                .episodes
                .iter()
                .find(|row| row.id == top.episode)
                .or(batch.episodes.first());
            if let Some(episode) = episode {
                out.top = Some(Top::of(top, episode));
            }
        }
        out
    }

    /// A binary built without SQLite has no facts to read, and the pane says which limitation it is
    /// looking at instead of drawing zeroes (`fathom --version` names the feature that is missing).
    #[cfg(not(feature = "sqlite-bundled"))]
    fn read(&self, _now: &Timestamp) -> Facts {
        Facts {
            index: IndexState {
                read: fathom_tui::index::Read::NoBackend,
                ..IndexState::default()
            },
            ..Facts::default()
        }
    }

    /// Fold the cached facts, the clock and the heartbeat into what the pane draws.
    fn compose(&self, now: &Timestamp) -> Glance {
        let facts = &self.facts;
        let mut reasons = Vec::new();
        let mut notes = facts.notes.clone();
        if let Some(note) = &self.index_note {
            notes.push(note.clone());
        }
        notes.extend(self.notes.iter().cloned());
        if !matches!(
            facts.index.read,
            fathom_tui::index::Read::Present | fathom_tui::index::Read::Unknown
        ) {
            reasons.push(Reason::new(
                Need::IndexBroken,
                "the index line below says why there is nothing on this pane to trust",
            ));
        }
        if facts.index.head_indexed == Some(false) {
            reasons.push(Reason::new(
                Need::HeadNotIndexed,
                format!(
                    "`HEAD` ({}) is not in the index, so the newest work is not on this pane",
                    short(&self.head)
                ),
            ));
        }
        if !self.index_here
            && let Some(last) = facts.index.last_utc
        {
            let idle = now.as_unix_seconds() - last / 1_000;
            if idle > STALE_AFTER_SECS && !self.watch.alive {
                reasons.push(Reason::new(
                    Need::NothingWatching,
                    format!(
                        "nothing has indexed for {} — `fathom watch`, or drop `--no-index`",
                        ambient::ago(idle)
                    ),
                ));
            }
        }
        if let Some(top) = &facts.top {
            reasons.push(Reason::new(top.kind, top.text.clone()));
        }
        Glance {
            now: *now,
            root: self.root_display(),
            branch: branch(&self.root),
            latest: facts.latest.clone(),
            queue: facts.queue.clone(),
            index: facts.index.clone(),
            watch: self.watch.clone(),
            reasons,
            notes,
            drill: facts.drill(),
        }
    }

    /// The repository as the reviewer typed it — never a resolved path, which would be a host leak
    /// (ADR-001 §7.5).
    fn root_display(&self) -> String {
        self.root
            .to_str()
            .map_or_else(|| ".".to_owned(), |text| text.to_owned())
    }
}

impl fathom_tui::ambient::Feed for Reader {
    /// The pane asks, the reader answers: probe cheaply, re-read only what moved, hand over the facts.
    fn glance(&mut self, force: bool) -> Glance {
        Reader::glance(self, force)
    }

    fn expired(&self) -> bool {
        Reader::expired(self)
    }
}

impl Top {
    /// The reason sentence, from the placement that ranked the row and the row itself.
    ///
    /// Every figure in it comes from the queue's own placement, so the pane's headline and the queue's row
    /// are the same claim in two fonts (ADR-001 §7.10).
    #[cfg(feature = "sqlite-bundled")]
    fn of(
        top: &fathom_analyze::insight::QueuePlacement,
        episode: &fathom_tui::timeline::Episode,
    ) -> Top {
        use fathom_analyze::insight::Severity;
        let kind = match top.severity {
            Some(Severity::High) => Need::HighRisk,
            Some(Severity::Medium) | Some(Severity::Low) => Need::OpenQueue,
            None => Need::Unranked,
        };
        let risk = top.risk.map_or_else(
            || "unscored".to_owned(),
            |score| format!("risk {score} {}", top.place()),
        );
        Top {
            kind,
            episode: episode.id.clone(),
            text: format!(
                "{} — {} · {risk}",
                fathom_analyze::insight::short_id(&episode.id),
                episode.summary
            ),
        }
    }
}

/// Is a watcher's heartbeat young enough to be a live one?
pub(crate) fn heartbeat_is_live(path: &Path) -> bool {
    let now = Timestamp::from_unix_micros(crate::clock::SystemClock.now().as_unix_micros());
    read_heartbeat(path, now).alive
}

/// The watcher's heartbeat, as the two facts the pane prints about it.
///
/// The heartbeat is read rather than a socket or a lock for the reason `fathom watch` writes it that way
/// (`reco-949`): a reader needs no protocol. A heartbeat older than [`HEARTBEAT_LIVE_SECS`] has stopped
/// being a promise and become a fossil, and the pane says `stopped` rather than quoting a phase some
/// process last wrote before it died.
fn read_heartbeat(path: &Path, now: Timestamp) -> WatchState {
    let Some(text) = std::fs::read_to_string(path).ok() else {
        return WatchState::default();
    };
    let Ok(status) = serde_json::from_str::<serde_json::Value>(&text) else {
        return WatchState {
            here: false,
            present: true,
            alive: false,
            summary: "the watcher's heartbeat is not readable JSON".to_owned(),
            age: None,
        };
    };
    let idle = status
        .get("stamp_ms")
        .and_then(serde_json::Value::as_i64)
        .map(|millis| (now.as_unix_seconds() - millis / 1_000).max(0));
    let phase = status
        .get("phase_summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown phase");
    let backend = status
        .pointer("/backend/name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown backend");
    WatchState {
        here: false,
        present: true,
        alive: idle.is_some_and(|secs| secs < HEARTBEAT_LIVE_SECS),
        summary: format!(
            "{phase} · {backend} · beat {}",
            idle.map_or("unknown".to_owned(), ambient::ago)
        ),
        age: idle.map(|secs| Duration::from_secs(secs as u64)),
    }
}

/// `HEAD`'s branch name, when there is one — the same cheap read `fathom watch` prints its own line from
/// (`fathom_ingest::watch::head_branch`, which exists for exactly this line).
fn branch(root: &Path) -> Option<String> {
    #[cfg(feature = "watch")]
    {
        fathom_ingest::repo::Repository::discover(root)
            .ok()
            .and_then(|repo| fathom_ingest::watch::head_branch(&repo))
    }
    #[cfg(not(feature = "watch"))]
    {
        let _ = root;
        None
    }
}

/// An oid's short form for the pane, with nothing invented when there is no oid.
fn short(oid: &str) -> String {
    if oid.is_empty() {
        return "unknown".to_owned();
    }
    oid.chars().take(7).collect()
}

/// This process's own CPU time, user plus system, from the kernel rather than from an estimate.
///
/// Linux only, because that is where `/proc/self/stat` is; elsewhere `None`, which `--self-check` reports
/// as an unverified budget rather than as a zero that would pass the gate by measuring nothing.
#[must_use]
fn process_cpu() -> Option<Duration> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        // The comm field is parenthesised and may hold spaces and parentheses, so the fields that matter
        // are counted from the closing paren, not from the start of the line.
        let tail = stat.rfind(')')?;
        let fields: Vec<&str> = stat[tail + 1..].split_whitespace().collect();
        // `utime` and `stime` are fields 14 and 15 of the line, which is the 12th and 13th after comm.
        let user: u64 = fields.get(12)?.parse().ok()?;
        let system: u64 = fields.get(13)?.parse().ok()?;
        Some(Duration::from_millis(
            user.saturating_add(system).saturating_mul(10),
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
