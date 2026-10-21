//! The producer that feeds an open frame (`reco-bpc`).
//!
//! An open TUI had nothing to notice new commits with. Detection was not the problem — `fathom watch`
//! notices a commit made in another terminal in ~160 ms — the *frame* had no producer: `frame::Options`
//! documented a worker that would push rows, and nothing anywhere constructed one. ADR-001 and ADR-006 §1
//! put fathom in a second terminal while an agent works in the first, and a second terminal showing a
//! frozen picture of a moving repository is worse than none, because a frozen rail reads as a quiet
//! repository.
//!
//! So this module is that producer, in the shape the shell can be handed: a [`fathom_tui::watch::Watcher`]
//! the frame asks on the wake-ups it already takes. Everything here is the composition root's and nothing
//! else's, which is exactly why it is here and not in `fathom-tui` (ADR-002 §5.2, lint `E01`): the probe
//! reads git, the re-read opens the store, and the expensive half runs the same `IndexJob` `fathom watch`
//! runs. The shell is handed rows and numbers, and never learns that either exists.
//!
//! # The three rules
//!
//! * **Ask cheap, read only what moved.** The same probe [`crate::ambient`]'s reader uses — four `stat`s
//!   and one ref read, behind a poll interval — so an idle frame costs nothing it can defend, and the
//!   expensive work happens on the frame's own clock rather than on a timer the loop did not ask for.
//! * **One writer per repository.** When a `fathom watch` heartbeat is alive, or a daemon is serving the
//!   repository, this process reads the index and does not write it, and says who does. Two processes
//!   indexing the same store is not faster, it is a lock fight.
//! * **A frame that cannot watch says so.** Where no producer can run — no index, a store that would not
//!   open, a binary built without the indexer, `--no-watch` — the answer is [`fathom_tui::watch::Source::snapshot`],
//!   which puts the limitation and its remedy in the frame instead of leaving a still picture to be
//!   mistaken for a quiet repository.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use fathom_tui::index::{Facts, Read};
use fathom_tui::watch::{Source, Update, Watcher};

/// How often an open frame looks for new commits.
///
/// The same 400 ms `fathom ambient --interval-ms` defaults to, for the same reason: it is what meets the
/// `watch.detect.ms` promise (~1 s, `benchmarks/budgets.json`) at a probe cost of four `stat`s and a ref
/// read, and asking more often than that buys no latency a reviewer can see. The shell's loop wakes ten
/// times a second for `<C-c>`; this interval is what stops the watcher turning those wake-ups into a
/// second timer.
pub const DEFAULT_POLL_MS: u64 = 400;

/// What one slice of indexing is allowed to cost, per ask — the watcher's own slice budget, enforced here
/// because this process is doing the watcher's job when no watcher is running.
#[cfg(feature = "watch")]
const SLICE_BUDGET: Duration = Duration::from_millis(250);

/// Episodes per slice when this process is the one keeping the index current: `fathom ambient`'s default,
/// for the same reason — a big backlog is taken in bites, one per ask, and the frame keeps drawing between
/// them rather than stopping to index the history of the repository in one keystroke-eating gulp.
#[cfg(feature = "watch")]
const CHUNK_UNITS: usize = 200;

/// What the frame should do about freshness, given what the caller knows about the index.
///
/// The decision is here rather than in `crate::tui` so that the sentence naming the limitation is written
/// next to the code that hits it, and so `--no-watch` has one meaning in one place.
#[must_use]
pub fn source(root: &Path, facts: &Facts, enabled: bool, poll: Duration) -> Source {
    // A zero poll is not "as fast as possible": that is a probe on every wake-up the loop takes, which is
    // the one thing an idle frame may not become. A caller that does not name one gets the documented one.
    let poll = if poll.is_zero() {
        Duration::from_millis(DEFAULT_POLL_MS)
    } else {
        poll
    };
    if !enabled {
        return Source::snapshot(
            "`--no-watch`",
            "reopen without the flag and this frame watches the repository itself, or run \
             `fathom watch` in a second terminal",
        );
    }
    match facts.read {
        // The cold case, and the one case this frame will not fix itself: building an epoch is `fathom
        // index`'s, and starting a cold index of a whole repository from a pane that only meant to keep up
        // with it is a surprise nobody asked for. Say which it is and what to type.
        Read::Absent => Source::snapshot(
            "nothing is indexed yet",
            "run `fathom index` here (or `fathom watch`) and this frame has rows to keep current",
        ),
        Read::Unreadable => Source::snapshot(
            "the index would not open",
            "`fathom doctor` says what is wrong with it",
        ),
        Read::NoBackend => Source::snapshot(
            "this binary has no SQLite backend",
            "`fathom --version` names the feature that is missing",
        ),
        Read::Unknown | Read::Present => feed(root, poll),
    }
}

/// A live watcher.
#[cfg(feature = "sqlite-bundled")]
fn feed(root: &Path, poll: Duration) -> Source {
    Source::live(Feed::open(root, poll))
}

/// No store to read, so nothing to keep current — the same limitation `reco-1nu`'s empty states name.
#[cfg(not(feature = "sqlite-bundled"))]
fn feed(_root: &Path, _poll: Duration) -> Source {
    Source::snapshot(
        "this binary has no SQLite backend",
        "`fathom --version` names the feature that is missing",
    )
}

/// The producer `fathom tui` installs.
///
/// `Send + Sync` with a [`Mutex`] inside rather than `&mut self`, because the shell holds every provider it
/// was given behind an `Arc` and hands it out immutably (`reco-bpc`); the lock is never contended, since
/// only the frame's own loop asks.
pub struct Feed {
    root: PathBuf,
    store_path: PathBuf,
    heartbeat: PathBuf,
    poll: Duration,
    /// Whether this process is the one allowed to index what it notices, as opposed to reading whatever a
    /// watcher left. Said on the frame when it is `false`, never hidden in a flag.
    index_here: bool,
    state: Mutex<State>,
}

/// The things a cheap probe cannot answer without paying for them again.
struct State {
    /// When the frame last asked; the poll interval is enforced here, not in the loop.
    asked: Option<Instant>,
    /// The last probe, which is the whole basis for "nothing moved".
    seen: Option<crate::ambient::Signature>,
    /// The watcher's own job, re-armed per ask.
    #[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
    job: crate::watch::IndexJob,
}

impl Feed {
    fn open(root: &Path, poll: Duration) -> Feed {
        let store_path = crate::session::store_path(root);
        let heartbeat = crate::ambient::heartbeat_path(&store_path);
        // Two questions before deciding to write to the store, in the order `fathom ambient` asks them:
        // is a watcher beating, and is a daemon serving? A daemon started with `--no-status-file` writes no
        // heartbeat, and a frame that began indexing behind it would be a second writer of one store.
        let watcher = crate::ambient::heartbeat_is_live(&heartbeat);
        let daemon = daemon_serving(root);
        Feed {
            root: root.to_path_buf(),
            index_here: !watcher && !daemon && can_index(),
            store_path,
            heartbeat,
            poll,
            state: Mutex::new(State {
                asked: None,
                seen: None,
                #[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
                job: crate::watch::IndexJob::for_root(
                    root,
                    crate::session::store_path(root),
                    CHUNK_UNITS,
                ),
            }),
        }
    }

    /// The one question the loop asks, and the one `<C-r>` asks louder.
    fn ask(&self, force: bool) -> Option<Update> {
        let Ok(mut state) = self.state.lock() else {
            // A poisoned lock means a watcher pass panicked on another thread; the frame keeps what it
            // drew, which is what `ADR-001 §7.2` asks of a partial.
            return None;
        };
        if !force && state.asked.is_some_and(|asked| asked.elapsed() < self.poll) {
            return None;
        }
        state.asked = Some(Instant::now());
        let now = crate::ambient::probe(&self.root, &self.store_path, &self.heartbeat);
        let Some(seen) = state.seen.clone() else {
            // The first look only records where the picture started: a frame that announced "new" about the
            // rows it is drawing for the first time would be reading out its own startup.
            state.seen = Some(now);
            return None;
        };
        if !force && !now.worth_reading(&seen) {
            state.seen = Some(now);
            return None;
        }
        let head_moved = now.head_moved(&seen);
        let mut update = Update::default();
        let mut note = None;
        if head_moved && self.index_here {
            match self.index_slice(&mut state, now.head()) {
                Ok(commits) => update.new_commits = Some(commits),
                Err(why) => note = Some(format!("the frame's own index run failed: {why}")),
            }
        } else if head_moved {
            // The honest lag: `HEAD` has moved, somebody else owns indexing it, and the picture will catch
            // up when they write. Saying it is the difference between a pane that is slow and a pane that
            // is lying.
            note = Some(format!(
                "`HEAD` moved to {} — a watcher owns indexing here, so the new rows land as soon as it \
                 writes them",
                short(now.head())
            ));
        }
        // Both halves of the re-read are keyed off the same index read, so the strip's numbers and the
        // rail's rows cannot disagree about the epoch they came from (`reco-wzt`'s rule, unchanged).
        let facts = crate::tui::index_facts(&self.root);
        update.episodes = crate::episodes::load(&self.root, &facts);
        update.index = Some(facts);
        update.note = note;
        // Re-probe after the work: indexing moved the store, and storing the *old* stamp here would make
        // the next ask see a change it had already accounted for and re-read the whole page again.
        state.seen = Some(crate::ambient::probe(
            &self.root,
            &self.store_path,
            &self.heartbeat,
        ));
        Some(update)
    }

    /// Take what is new, in one bounded slice, with the watcher's own budget re-armed for this ask.
    #[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
    fn index_slice(&self, state: &mut State, head: &str) -> Result<u64, String> {
        use fathom_ingest::watch::{Interest, InterestReason, Job as _, Progress};
        state.job.start_slice(SLICE_BUDGET);
        let interest = Interest {
            reason: InterestReason::Commits,
            head: head.to_owned(),
            changes: Vec::new(),
            index_touched: false,
            replaced: false,
            signals: Vec::new(),
        };
        // The per-chunk progress the watcher prints is not this frame's to print: the row the shell would
        // draw it in is `index.progress`, and a frame that redrew itself every 250 ms while indexing a
        // backlog is a frame that owns the terminal, which ADR-006 §0.1 forbids outright.
        let mut quiet = |_: Progress| {};
        state
            .job
            .refresh(&interest, &mut quiet)
            .map(|progress| progress.commits)
            .map_err(|err| err.message().to_owned())
    }

    /// A build without the watcher's job cannot index what it notices; it reads what somebody else wrote.
    /// `can_index` keeps this from being reached, and the compiler still wants it to exist.
    #[cfg(not(all(feature = "watch", feature = "sqlite-bundled")))]
    fn index_slice(&self, _state: &mut State, _head: &str) -> Result<u64, String> {
        Err("this build cannot index from the shell".to_owned())
    }
}

impl Watcher for Feed {
    fn poll(&self) -> Option<Update> {
        self.ask(false)
    }

    fn refresh(&self) -> Option<Update> {
        self.ask(true)
    }
}

/// Can this build index at all? `watch` owns the job, `sqlite-bundled` owns the store it writes.
fn can_index() -> bool {
    cfg!(all(feature = "watch", feature = "sqlite-bundled"))
}

/// Is a daemon serving this repository? Only a build with the daemon can ask, and only a daemon can answer.
#[cfg(feature = "watch")]
fn daemon_serving(root: &Path) -> bool {
    crate::daemon::serving(root)
}

#[cfg(not(feature = "watch"))]
fn daemon_serving(_root: &Path) -> bool {
    false
}

/// An oid's short form, with nothing invented when there is no oid.
fn short(oid: &str) -> String {
    if oid.is_empty() {
        return "unknown".to_owned();
    }
    oid.chars().take(7).collect()
}
