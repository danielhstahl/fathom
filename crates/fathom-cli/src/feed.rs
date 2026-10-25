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
//!
//! `<C-r>` (`index.reindex`) asks this producer *now* (`reco-j7t`). A forced ask goes to the index job
//! whether or not `HEAD` moved — "nothing moved since I looked" is not the answer to "is this index up to
//! date" — and the run reports its own counters back through [`fathom_tui::watch::Update::work`], so the
//! frame shows `indexing 40 % · ~2 s left · 200 / 500 commits` from the indexer's numbers rather than
//! narrating that it has no store handle. A run that stops for its slice budget keeps being asked, on the
//! wake-ups the loop already takes; the loop gains no timer, no thread and no second poller.

use std::path::{Path, PathBuf};
#[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use fathom_tui::index::{Facts, Read};
use fathom_tui::watch::{Source, Update, Watcher, Work};

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
    /// What this process's own index run has said about itself, while it is still running (`reco-j7t`).
    ///
    /// `None` between runs. A run that stopped for its slice budget is not a finished run, and the numbers
    /// the next slice reports have to be read against the ones this one already reported or the progress
    /// row restarts from `0 %` every quarter second — which is a row that reads as a run that keeps
    /// starting over, and a reviewer who watches that presses `<C-c>`.
    run: Option<Run>,
}

/// One in-flight index run this process is running on the frame's behalf (`reco-j7t`).
///
/// Every number here came out of the index run's own ticks — nothing is counted from outside, because an
/// outside count of somebody else's work is a guess wearing a percentage (`ADR-006 §10.3`).
#[derive(Default)]
struct Run {
    /// Commits the spine took, summed over the slices so far.
    commits: u64,
    /// Commits that owed derivation when the run stated its denominator for the first time. Frozen for the
    /// length of the run: each slice restarts the run's own count, and the row that restarts with it is a
    /// lie about how far the reviewer has to go.
    of: Option<u64>,
    /// Commits still owing derivation, as the last slice's last tick reported them.
    left: Option<u64>,
    /// Analyses computed, summed over the slices so far.
    units: u64,
    /// The word the run used for the phase it was in when it last reported, and what it calls one unit of
    /// it — so the row says `analysing 40 % · … · 200 / 500 commits` in the indexer's own vocabulary and
    /// not one the frame invented.
    phase: Option<Words>,
}

/// What the run is called, and what its units are called, in the indexer's own words.
type Words = (&'static str, &'static str);

/// What one slice of a run reported, before the frame folds it into the run's totals (`reco-j7t`).
#[derive(Default)]
struct Report {
    /// Commits this slice took into the spine.
    commits: u64,
    /// Analyses this slice computed.
    units: u64,
    /// Did the run say it was finished, or did it stop for its slice budget and wait to be asked again?
    complete: bool,
    /// The phase the slice ended in, as words.
    phase: Option<Words>,
    /// The derivation denominator the run last stated: the words it belongs to, the commits derived, and
    /// the commits that owed derivation when the run counted them.
    deriving: Option<(Words, u64, u64)>,
}

impl Run {
    /// Fold one slice's report into the run's totals and say what the row should read (`reco-j7t`).
    ///
    /// The denominator belongs to the run and not to the slice. Each slice calls `index::run` again, and
    /// each one re-asks the store what still owes derivation and starts its own count from there — so a
    /// row printed straight from a slice restarts at `0 %` every quarter second, which reads as a run
    /// that keeps starting over. The frame keeps the total it was first given and counts the rest off what
    /// the run says is *left*, which is the one figure every slice agrees on.
    fn fold(&mut self, slice: Report) -> Work {
        self.commits += slice.commits;
        self.units += slice.units;
        if let Some((words, done, of)) = slice.deriving {
            self.of = Some(self.of.map_or(of, |seen| seen.max(of)));
            self.left = Some(of.saturating_sub(done));
            self.phase = Some(words);
        } else if self.of.is_none() {
            // Nothing has stated a denominator yet, so the row borrows the run's own word for what it is
            // doing and shows the counts it has — with no percentage, because there is nothing yet to
            // take a percentage of (`ADR-001 §7.7`).
            self.phase = slice.phase;
        }
        Work {
            task: self.phase.map_or("indexing", |(word, _)| word),
            unit: self.phase.map_or("commit", |(_, unit)| unit),
            done: self
                .of
                .map_or(self.commits, |of| of.saturating_sub(self.left.unwrap_or(0))),
            of: self.of,
            units: self.units,
            complete: slice.complete,
        }
    }
}

/// What one ask's slice of indexing did, in the only shapes the frame can show (`reco-j7t`).
struct Slice {
    /// The status row: the run's counters so far, and whether it is finished.
    work: Work,
    /// Commits the run has taken into the spine, summed over its slices — what the toast counts when the
    /// run lands, because the reviewer asked how much of the repository is new, not how much one
    /// quarter-second of it was.
    commits: u64,
    /// Did the run finish? `false` says *ask me again*, and the poll interval steps aside for it.
    complete: bool,
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
                run: None,
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
        // Two things switch the poll interval off. `<C-r>` is one: the reviewer asked *now*. The other is a
        // run this process is in the middle of (`reco-j7t`) — a slice that stopped for its budget is not a
        // run that finished, and making the frame wait a whole poll interval to be told the next quarter of
        // it is done is a slow way of saying *still working* when the work is right here.
        let mid_run = state.run.is_some();
        if !force && !mid_run && state.asked.is_some_and(|asked| asked.elapsed() < self.poll) {
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
        if !force && !mid_run && !now.worth_reading(&seen) {
            state.seen = Some(now);
            return None;
        }
        let head_moved = now.head_moved(&seen);
        let mut update = Update::default();
        let mut note = None;
        // `<C-r>` asks for a re-index, not for a re-probe, so a forced ask puts the question to the index
        // job whether or not `HEAD` moved since the last look: "nothing moved since I looked" is not the
        // answer to "is this index up to date". A poll still runs only on movement — an idle frame may not
        // start a walk of its repository four times a second, whatever its budget allows.
        let index_now = self.index_here && (force || head_moved || mid_run);
        if index_now {
            match self.index_slice(&mut state, now.head()) {
                Ok(slice) => {
                    update.work = Some(slice.work);
                    if slice.complete {
                        update.new_commits = (slice.commits > 0).then_some(slice.commits);
                        state.run = None;
                    }
                }
                Err(why) => {
                    // A run that failed is a run that is over: the row goes away and the sentence says
                    // which of the two happened, rather than leaving a progress line standing over a
                    // repository the frame has stopped reading.
                    state.run = None;
                    note = Some(format!("the frame's own index run failed: {why}"));
                }
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
        // Re-probe after the work: indexing moved the store, and storing the *old* stamp here would make
        // the next ask see a change it had already accounted for and re-read the whole page again.
        let after = crate::ambient::probe(&self.root, &self.store_path, &self.heartbeat);
        // While the frame's own run is still going, that row *is* the update. Re-reading the store's
        // numbers and the rail's page on every one of those asks would charge a repository read to the
        // reviewer four times a second for rows that cannot be complete before the run says so; the
        // numbers and the rows arrive with the update that says it landed (`reco-j7t`).
        if update.work.as_ref().is_some_and(|work| !work.complete) {
            state.seen = Some(after);
            return Some(update);
        }
        // Both halves of the re-read are keyed off the same index read, so the strip's numbers and the
        // rail's rows cannot disagree about the epoch they came from (`reco-wzt`'s rule, unchanged).
        let facts = crate::tui::index_facts(&self.root);
        update.episodes = crate::episodes::load(&self.root, &facts);
        update.index = Some(facts);
        // ...and so is the corpus the `/` box scores against (`reco-6sw`). It is read once at startup and it
        // holds the working tree's symbol names, which are the fastest-ageing rows in it; re-reading it here
        // — on the ask that already decided the repository moved, and on `<C-r>` — is what keeps `/` from
        // answering from a tree the reviewer has since changed. The cost is bounded by the scan's own caps
        // and is charged to the same moment that just re-read the whole rail page.
        #[cfg(feature = "tui")]
        {
            update.search = crate::search::index(&self.root);
        }
        update.note = note;
        state.seen = Some(after);
        Some(update)
    }

    /// Take what is new, in one bounded slice, with the watcher's own budget re-armed for this ask — and
    /// say, in the run's own numbers, how far the run has got (`reco-j7t`).
    #[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
    fn index_slice(&self, state: &mut State, head: &str) -> Result<Slice, String> {
        use fathom_analyze::index::{Phase, ProgressSink, Tick};
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
        // The run's own ticks, caught on their way past. The frame cannot install a printer, but the
        // counters `fathom index` prints are exactly the ones a reviewer watching this frame needs, and
        // they are measured against the one denominator a run can state up front — the commits that owe
        // derivation — rather than against a total the frame would have to invent.
        #[derive(Default)]
        struct Caught {
            /// The last tick of any phase: the word for what the run is doing.
            last: Option<Tick>,
            /// The last derivation tick that came with a denominator, as `(done, of)`.
            deriving: Option<(u64, u64)>,
        }
        let caught = Arc::new(Mutex::new(Caught::default()));
        let sink = {
            let caught = Arc::clone(&caught);
            ProgressSink::new(move |tick: &Tick| {
                if let Ok(mut seen) = caught.lock() {
                    if tick.phase == Phase::Analysing {
                        seen.deriving = tick.of.map(|of| (tick.done, of));
                    }
                    seen.last = Some(*tick);
                }
            })
        };
        state.job.set_ticks(Some(sink));
        // The per-chunk progress the watcher prints is not this frame's to print: the row the shell would
        // draw it in is `index.progress`, and a frame that redrew itself every 250 ms while indexing a
        // backlog is a frame that owns the terminal, which ADR-006 §0.1 forbids outright.
        let mut quiet = |_: Progress| {};
        let outcome = state.job.refresh(&interest, &mut quiet);
        state.job.set_ticks(None);
        let progress = outcome.map_err(|err| err.message().to_owned())?;
        let caught = caught
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Everything the run said, in the frame's own vocabulary: two `&'static str` words rather than a
        // `Phase`, so the arithmetic that turns it into a row is testable without a store behind it.
        // `analysing` is the phase that owns the denominator, but the row is about the re-index the
        // reviewer asked for, and the indexer calls the whole job `indexing` in ADR-006 §10.3's own copy —
        // so the row says `indexing`, measured against the derivation count (`reco-j7t`).
        let words = ("indexing", Phase::Analysing.unit());
        let report = Report {
            commits: progress.commits,
            units: progress.units,
            complete: progress.complete,
            // The derivation phase owns the denominator, so a slice that reached it is labelled with its
            // words; until then the row borrows whatever phase the run last reported — a walk still
            // walking, an episodes pass still rebuilding.
            phase: match (caught.deriving, caught.last) {
                (Some(_), _) => Some(words),
                (None, Some(tick)) => Some((tick.phase.word(), tick.phase.unit())),
                (None, None) => None,
            },
            deriving: caught.deriving.map(|(done, of)| (words, done, of)),
        };
        let run = state.run.get_or_insert_with(Run::default);
        let work = run.fold(report);
        let complete = progress.complete;
        Ok(Slice {
            work,
            commits: run.commits,
            complete,
        })
    }

    /// A build without the watcher's job cannot index what it notices; it reads what somebody else wrote.
    /// `can_index` keeps this from being reached, and the compiler still wants it to exist.
    #[cfg(not(all(feature = "watch", feature = "sqlite-bundled")))]
    fn index_slice(&self, _state: &mut State, _head: &str) -> Result<Slice, String> {
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

#[cfg(test)]
mod tests {
    use super::{Report, Run};

    const WORDS: (&str, &str) = ("indexing", "commit");

    #[test]
    fn a_slice_that_stopped_for_its_budget_reports_the_run_and_not_the_slice() {
        // 500 commits owe derivation; the first slice takes 200 of them and stops for its budget. The run
        // restarts its own count on every slice — each call asks the store what is *left* — so a row built
        // from one slice would read `0 %` four times a second, which is a row that reads as a hang.
        let mut run = Run::default();
        let first = run.fold(Report {
            commits: 200,
            units: 812,
            complete: false,
            phase: Some(WORDS),
            deriving: Some((WORDS, 200, 500)),
        });
        assert_eq!((first.done, first.of, first.units), (200, Some(500), 812));
        assert!(
            !first.complete,
            "a slice that stopped for budget is not a finished run"
        );
        assert_eq!(first.task, "indexing", "the row is in the indexer's words");
        // The second slice sees 300 left and takes all of them. The denominator stays 500 — the total the
        // run first stated — and `done` is that total minus what the run says is left.
        let second = run.fold(Report {
            commits: 300,
            units: 1_204,
            complete: true,
            phase: Some(WORDS),
            deriving: Some((WORDS, 300, 300)),
        });
        assert_eq!(
            (second.done, second.of, second.units),
            (500, Some(500), 2_016),
            "the row counts up to the run's own total, not the last slice's"
        );
        assert!(second.complete, "and it says when the run is over");
    }

    #[test]
    fn a_run_that_never_reached_its_denominator_promises_no_percentage() {
        // A walk still walking has no denominator yet: the run does not know how much derivation it owes
        // until it has walked, and printing `0 %` for "not counted yet" is the lie ADR-001 §7.7 refuses.
        let mut run = Run::default();
        let walking = run.fold(Report {
            commits: 0,
            units: 0,
            complete: false,
            phase: Some(("indexing", "commit")),
            deriving: None,
        });
        assert_eq!(walking.of, None, "no denominator yet, so no percentage");
        assert_eq!(walking.done, 0, "and nothing invented for the count");
        assert_eq!(
            walking.task, "indexing",
            "the run's own word for what it is doing"
        );
    }

    #[test]
    fn a_slice_whose_run_stated_a_smaller_total_than_it_already_said_keeps_the_larger_one() {
        // A crashed run's commits come back into the derivation list on the next pass, so the list a later
        // slice states can be bigger or smaller than the one before it. The row keeps the larger total and
        // never goes backwards past the work it already reported.
        let mut run = Run::default();
        run.fold(Report {
            commits: 100,
            units: 100,
            complete: false,
            phase: Some(WORDS),
            deriving: Some((WORDS, 100, 400)),
        });
        let after = run.fold(Report {
            commits: 50,
            units: 50,
            complete: false,
            phase: Some(WORDS),
            deriving: Some((WORDS, 0, 600)),
        });
        assert_eq!(after.of, Some(600), "the bigger denominator wins");
        assert_eq!(
            after.done, 0,
            "and the row says where the run actually is, not where it was"
        );
        assert_eq!(after.units, 150, "while the counts it owns keep summing");
    }
}
