//! An open frame notices the commit made next door (`reco-bpc`), and re-indexes when it is asked to
//! (`reco-j7t`).
//!
//! The defect was that nothing ever told the frame anything: `frame::Options` documented a worker that
//! would push rows, and no producer existed, so an ambient terminal showed a still picture of a moving
//! repository for as long as it was open. Two promises are tested here, and they are the two the ticket
//! allows:
//!
//! * when something *is* feeding the frame, a change arrives on its own — no keypress, no restart, and no
//!   second process the reviewer had to remember to start;
//! * when nothing is, the frame says so, in the frame, and says what would move it. A frozen rail that
//!   admits it is frozen is a limitation; one that does not is a lie about the repository.
//!
//! The second defect was the key that narrated its own absence: `<C-r>` wrote a progress row for a run it
//! never started and named a ticket that had closed weeks ago as the party that would one day own the
//! worker. What is asserted here is the shape of a real answer — a producer's counters turn into a row on
//! the status strip while the run goes, the row goes when the run lands, and no answer the key gives
//! points a reviewer at work that is already finished.
//!
//! No git, no store and no pty: the producer is scripted, which is the point — the shell's half of the
//! contract is "do the right thing with what you are handed", and the composition root's half is tested
//! where the store is (`crates/fathom-cli/tests`, and `tools/mvp_acceptance.py`'s pty measurement).

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::event::{Bus, Message};
use fathom_tui::frame::{HeadlessSink, Options, run_loop};
use fathom_tui::index::Facts;
use fathom_tui::keymap::Keymap;
use fathom_tui::timeline::{Batch, Episode};
use fathom_tui::watch::{Source, Update, Watcher, Work};

/// The instant the frames are drawn for; a fixture that read a clock would be a fixture with a flake.
const NOW: Timestamp = Timestamp::from_unix_micros(1_700_000_000_000_000);
const SIZE: (u16, u16) = (100, 30);
/// One day, for ages that make two rows distinguishable.
const DAY: i64 = 86_400_000_000;

fn episode(id: &str, age_days: i64) -> Episode {
    let created = Timestamp::from_unix_micros(NOW.as_unix_micros() - age_days * DAY);
    let mut row = Episode::new(id, created, format!("episode {id}"));
    row.last = created;
    row.author_name = "pi".to_owned();
    row.n_add = 10;
    row.n_del = 2;
    row.n_files = 1;
    row.paths = vec![format!("crates/{id}/mod.rs")];
    row.fingerprint = format!("sha256:{id}");
    row
}

fn batch(ids: &[&str]) -> Batch {
    Batch::new(
        "fixture index",
        ids.iter().map(|id| episode(id, 1)).collect::<Vec<_>>(),
    )
}

/// A watcher scripted by a count: it has nothing to say until asked `n` times, then says it once.
///
/// That is the shape of a real one — a cheap probe that answers `None` until a commit lands — minus the
/// git and the store, which is what makes the frame's half testable at all.
struct Scripted {
    /// The polls after which the update arrives.
    after: usize,
    /// How many times it was asked, so a test can prove the loop asked at all.
    polls: AtomicUsize,
}

impl Scripted {
    fn new(after: usize) -> Self {
        Scripted {
            after,
            polls: AtomicUsize::new(0),
        }
    }

    fn polls(&self) -> usize {
        self.polls.load(Ordering::Relaxed)
    }
}

impl Watcher for Scripted {
    fn poll(&self) -> Option<Update> {
        let seen = self.polls.fetch_add(1, Ordering::Relaxed) + 1;
        (seen == self.after).then(|| Update {
            episodes: Some(batch(&["ep-old", "ep-new"])),
            index: Some(Facts::present(
                Some(1),
                Some("ready".to_owned()),
                Some(2),
                Some(2),
            )),
            new_commits: Some(1),
            note: None,
            work: None,
        })
    }

    fn refresh(&self) -> Option<Update> {
        // `<C-r>` says "look now", and a scripted watcher that only ever answers on its own schedule
        // could not tell the difference — so it answers this one every time it is asked.
        self.polls.fetch_add(1, Ordering::Relaxed);
        Some(Update {
            episodes: Some(batch(&["ep-old", "ep-new", "ep-forced"])),
            index: Some(Facts::present(
                Some(1),
                Some("ready".to_owned()),
                Some(3),
                Some(3),
            )),
            new_commits: Some(1),
            note: None,
            work: None,
        })
    }
}

fn shell(watch: Option<Source>) -> App {
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    app.timeline.set_now(NOW);
    app.load_episodes(batch(&["ep-old"]));
    app.watch = watch;
    app
}

/// How many episodes the rail holds — not `timeline::len()`, which counts the motion slots and so
/// includes the group headers between the rows.
fn held(app: &App) -> usize {
    app.timeline.rail().episodes().len()
}

fn status_line(app: &App) -> String {
    app.status_bits().join(" · ")
}

// ---------------------------------------------------------------------------------------------
// the live half: a commit arrives by itself
// ---------------------------------------------------------------------------------------------

#[test]
fn an_open_frame_notices_a_commit_made_while_it_was_open() {
    let scripted = std::sync::Arc::new(Scripted::new(2));
    let mut app = shell(Some(Source::live(ScriptedRef(std::sync::Arc::clone(
        &scripted,
    )))));
    let bus = Bus::new();
    // One no-op message, to get past the loop's own rule that a scripted run which has gone quiet has
    // reached the end of its script: the second iteration is the one under test, and it is a timeout —
    // which is exactly the iteration a real idle frame spends all its life in.
    bus.send(Message::Repaint);
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: Some(SIZE),
    };
    // Two frames: the first one the shell draws for itself, the second one the watcher causes. Nothing
    // about the second frame was caused by a keystroke, which is the whole ticket.
    let report = run_loop(&mut app, &bus, &mut sink, None, Some(2))
        .expect("the loop runs with one repaint and a producer on the bus");
    assert_eq!(
        sink.frames.len(),
        2,
        "the loop drew the first frame and then the watcher's: {:?}",
        sink.frames.len()
    );
    assert_eq!(report.frames, 2);
    assert_eq!(
        report.end,
        fathom_tui::frame::LoopEnd::FrameLimit,
        "the run ended on its frame limit, not on the watcher going quiet"
    );
    let first = sink.frames[0].join("\n");
    let second = sink.frames[1].join("\n");
    assert_ne!(
        first, second,
        "the frame did not change when the repository did"
    );
    assert!(
        second.contains("ep-new"),
        "the new episode is on the rail:\n{second}"
    );
    assert!(
        !first.contains("ep-new"),
        "and it was not there before:\n{first}"
    );
    // The producer was asked on a wake-up nothing had been pressed for — the loop asks, it is not asked.
    assert!(scripted.polls() >= 2, "polled {} times", scripted.polls());
    // …and the frame said all along that it was live, which is what makes the change an expected one
    // rather than a surprise the reviewer has to reverse-engineer.
    let strip = status_line(&app);
    assert!(
        strip.contains("live"),
        "the strip carries its promise: {strip}"
    );
    assert!(!strip.contains("snapshot"), "{strip}");
}

#[test]
fn the_watchers_update_names_itself_rather_than_arriving_silently() {
    let mut app = shell(None);
    let before = held(&app);
    fathom_tui::frame::apply_update(
        &mut app,
        Update {
            episodes: Some(batch(&["ep-old", "ep-new"])),
            index: Some(Facts::present(
                Some(1),
                Some("ready".to_owned()),
                Some(2),
                Some(2),
            )),
            new_commits: Some(1),
            note: None,
            work: None,
        },
    );
    assert_eq!(before, 1);
    assert_eq!(held(&app), 2);
    let said = app.toast.clone().unwrap_or_default();
    assert!(
        said.starts_with("1 new · "),
        "the toast says what arrived, not merely that something changed: {said}"
    );
    // The numbers the strip prints moved with the rows, because both came from the one re-read.
    assert_eq!(app.model.index.episodes, Some(2));
    assert_eq!(app.model.index_facts.commits, Some(2));
}

#[test]
fn a_cursor_survives_the_rows_moving_under_it() {
    let mut app = shell(None);
    app.feed_key("j", Instant::now());
    let on = app
        .timeline
        .selected()
        .expect("a row is under the cursor")
        .id
        .clone();
    fathom_tui::frame::apply_update(
        &mut app,
        Update {
            episodes: Some(batch(&["ep-newer", "ep-old", "ep-new"])),
            ..Update::default()
        },
    );
    let still = app
        .timeline
        .selected()
        .expect("the rail still has a cursor")
        .id
        .clone();
    assert_eq!(on, still, "the rows moved; the reviewer's place did not");
}

#[test]
fn a_producer_that_says_nothing_draws_nothing() {
    // The idle case, which is the one ADR-006 §0.1 is about: the loop may ask its producer on every
    // wake-up it already takes, but a `None` must not become a frame.
    #[derive(Default)]
    struct Quiet;
    impl Watcher for Quiet {
        fn poll(&self) -> Option<Update> {
            None
        }
    }
    let mut app = shell(Some(Source::live(Quiet)));
    let bus = Bus::new();
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: Some(SIZE),
    };
    let report = run_loop(&mut app, &bus, &mut sink, None, Some(1))
        .expect("the loop runs with nothing on the bus");
    assert_eq!(sink.frames.len(), 1, "only the first frame was drawn");
    assert_eq!(report.events, 0, "and nothing arrived to apply");
}

/// A [`Watcher`] that forwards to a shared script, so a test can read the counters after the loop ends.
struct ScriptedRef(std::sync::Arc<Scripted>);

impl Watcher for ScriptedRef {
    fn poll(&self) -> Option<Update> {
        self.0.poll()
    }

    fn refresh(&self) -> Option<Update> {
        self.0.refresh()
    }
}

// ---------------------------------------------------------------------------------------------
// the honest half: a frame that cannot watch says so
// ---------------------------------------------------------------------------------------------

#[test]
fn a_frame_that_cannot_watch_says_it_is_a_snapshot_and_what_moves_it() {
    let options = Options {
        size: Some(SIZE),
        now: NOW,
        watch: Some(Source::snapshot(
            "no watcher is running",
            "run `fathom watch` here and this frame shows what it indexes",
        )),
        ..Options::default()
    };
    let frame = fathom_tui::frame::render_plain(&options).join("\n");
    assert!(
        frame.contains("snapshot · no watcher is running"),
        "the strip says the picture is not moving:\n{frame}"
    );
    // …and the long form, in the notice list the `?` overlay prints: the limitation *and* the remedy,
    // which is the half a status-line bit has no room for.
    let sentence = Source::snapshot(
        "no watcher is running",
        "run `fathom watch` here and this frame shows what it indexes",
    )
    .sentence();
    assert!(
        sentence.contains("snapshot") && sentence.contains("fathom watch"),
        "{sentence}"
    );
}

#[test]
fn a_frame_nobody_asked_about_freshness_says_nothing_about_it() {
    // `None` is a caller with no opinion — a test frame, an embedder — not a third answer. The shipped
    // shell always has one; a headless render must not invent a claim about a repository it never read.
    let options = Options {
        size: Some(SIZE),
        now: NOW,
        ..Options::default()
    };
    let frame = fathom_tui::frame::render_plain(&options).join("\n");
    assert!(
        !frame.contains("snapshot") && !frame.contains("live"),
        "the frame invented a promise nobody made:\n{frame}"
    );
}

#[test]
fn a_live_frame_says_live_and_nothing_more() {
    let options = Options {
        size: Some(SIZE),
        now: NOW,
        watch: Some(Source::live(Scripted::new(usize::MAX))),
        ..Options::default()
    };
    let frame = fathom_tui::frame::render_plain(&options).join("\n");
    assert!(
        frame.contains("live"),
        "the strip says it is live:\n{frame}"
    );
    assert!(
        !frame.contains("snapshot"),
        "and does not hedge about it:\n{frame}"
    );
}

#[test]
fn a_snapshot_polls_nothing_because_nothing_can_answer() {
    let snapshot = Source::snapshot("why", "refresh");
    assert!(!snapshot.is_live());
    assert!(snapshot.poll().is_none());
    assert!(snapshot.refresh().is_none());
    assert_eq!(snapshot.bit(), "snapshot · why");
}

#[test]
fn ctrl_r_asks_the_producer_now_instead_of_waiting_for_its_poll() {
    // `<C-r>` is the one key that must not be honest-about-nothing: with a watcher installed it re-reads,
    // and what comes back is the same update a poll would have delivered later.
    let mut app = shell(Some(Source::live(Scripted::new(usize::MAX))));
    assert_eq!(held(&app), 1);
    app.feed_key("<C-r>", Instant::now());
    assert_eq!(
        held(&app),
        3,
        "the forced re-read replaced the rail with what the repository now holds"
    );
    assert!(
        app.timeline
            .rail()
            .episodes()
            .iter()
            .any(|row| row.id == "ep-forced"),
        "and the rows are the forced ones"
    );
}

#[test]
fn ctrl_r_on_a_snapshot_says_what_would_move_it_rather_than_claiming_to_refresh() {
    let mut app = shell(Some(Source::snapshot(
        "`--no-watch`",
        "reopen without the flag",
    )));
    app.feed_key("<C-r>", Instant::now());
    let said = app.toast.clone().unwrap_or_default();
    assert!(
        said.contains("snapshot") && said.contains("`--no-watch`") && said.contains("reopen"),
        "the refusal names the reason and the way out: {said}"
    );
}

/// The poll interval the loop is allowed to ask at is the composition root's business, not the loop's;
/// the loop's own bound is the signal poll, and this keeps the two numbers in the same order.
#[test]
fn the_frame_asks_no_faster_than_its_own_wakeup() {
    assert!(
        fathom_tui::terminal::SIGNAL_POLL >= Duration::from_millis(50),
        "the wake-up the producer piggy-backs on is not the one it sets"
    );
}

// ---------------------------------------------------------------------------------------------
// the third answer: `<C-r>` starts a re-index, and says how far it has got (`reco-j7t`)
// ---------------------------------------------------------------------------------------------

/// A producer scripted to be *in the middle of* a run: every ask answers with the next row the indexer
/// would have reported, and the last row repeats forever, which is how a finished run keeps saying so.
///
/// The rows are the indexer's own — the phase word, the unit word, the denominator out of
/// `fathom_analyze::index::Tick` — because the point of the test is the frame's half of the contract:
/// turn the numbers a producer hands over into a row a reviewer can read, and take the row away when the
/// run says it is done.
struct ScriptedRun {
    asks: Vec<Work>,
    asked: AtomicUsize,
}

impl ScriptedRun {
    fn new(asks: Vec<Work>) -> Self {
        ScriptedRun {
            asks,
            asked: AtomicUsize::new(0),
        }
    }
}

impl Watcher for ScriptedRun {
    fn poll(&self) -> Option<Update> {
        let n = self.asked.fetch_add(1, Ordering::Relaxed);
        Some(Update {
            work: Some(self.asks[n.min(self.asks.len() - 1)].clone()),
            ..Update::default()
        })
    }

    fn refresh(&self) -> Option<Update> {
        self.poll()
    }
}

#[test]
fn ctrl_r_reports_the_run_it_started_in_the_indexers_own_numbers() {
    let mut app = shell(Some(Source::live(ScriptedRun::new(vec![
        Work::running("analysing", "commit", 200, Some(500)).with_units(1_204),
    ]))));
    assert!(
        app.model.index.progress.is_none(),
        "nothing is running before the key is pressed"
    );
    app.feed_key("<C-r>", Instant::now());
    let row = app
        .model
        .index
        .progress
        .clone()
        .expect("the re-index the reviewer asked for has a row");
    assert!(
        row.starts_with("analysing ") && row.contains("40 %"),
        "the row is the run's own phase, measured against the run's own denominator: {row}"
    );
    assert!(
        row.contains("200 / 500 commit"),
        "the counts behind the percentage are printed beside it, as everywhere else: {row}"
    );
    assert!(
        row.contains("1 204 analyses computed"),
        "a warm run whose commits are cheap but whose analyses are not says so: {row}"
    );
    // And it is on the strip, which is the half the ticket was about: the row used to live only in an
    // empty pane, so on a rail with 500 episodes in it a re-index showed no progress at all.
    let strip = status_line(&app);
    assert!(
        strip.contains("analysing") && strip.contains("200 / 500"),
        "the status strip carries the row: {strip}"
    );
}

#[test]
fn a_run_that_lands_takes_its_row_with_it() {
    let mut app = shell(Some(Source::live(ScriptedRun::new(vec![Work::running(
        "analysing",
        "commit",
        200,
        Some(500),
    )]))));
    app.feed_key("<C-r>", Instant::now());
    assert!(app.model.index.progress.is_some());
    // The update that says the run is finished: the row goes, the strip's numbers move, and the estimate
    // is dropped rather than left to describe a run that ended.
    fathom_tui::frame::apply_update(
        &mut app,
        Update {
            work: Some(Work::finished("analysing", "commit", 500, Some(500))),
            index: Some(Facts::present(
                Some(2),
                Some("ready".to_owned()),
                Some(503),
                Some(503),
            )),
            ..Update::default()
        },
    );
    assert!(
        app.model.index.progress.is_none(),
        "a row that outlives its run is the placeholder this ticket was filed against: {:?}",
        app.model.index.progress
    );
    assert!(
        app.estimates.is_empty(),
        "the estimate goes with the row it belongs to"
    );
    let strip = status_line(&app);
    assert!(
        !strip.contains("analysing"),
        "the strip stopped saying it: {strip}"
    );
    assert_eq!(app.model.index.episodes, Some(503), "and said what landed");
}

#[test]
fn a_run_that_finishes_inside_one_ask_never_shows_a_row() {
    // The common case: a warm repository with one new commit. A row that appeared and vanished between
    // two frames is worse than no row, because what the reviewer remembers seeing is `0 %`.
    let mut app = shell(Some(Source::live(ScriptedRun::new(vec![Work::finished(
        "analysing",
        "commit",
        1,
        Some(1),
    )]))));
    app.feed_key("<C-r>", Instant::now());
    assert!(
        app.model.index.progress.is_none(),
        "a run that finished in the same breath it started in leaves nothing standing: {:?}",
        app.model.index.progress
    );
}

/// Does this sentence name a ticket? The point of the check is not the format: it is that a closed
/// ticket, read in the present tense, is a reviewer being pointed at work that is already done.
fn names_a_ticket(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        let word = word.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '.'));
        if word.starts_with("reco-") {
            return true;
        }
        // `e1.4`, `e12.3`: letters, digits, a dot, digits — one word, no spaces.
        let Some(dot) = word.find('.') else {
            return false;
        };
        let (before, after) = (word[..dot].to_owned(), word[dot + 1..].to_owned());
        let letters: String = before
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        let digits = &before[letters.len()..];
        let number = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_digit());
        !letters.is_empty() && number(digits) && number(&after)
    })
}

#[test]
fn the_ticket_scanner_catches_the_sentence_this_ticket_was_filed_about() {
    // A scanner that never fires is worse than none, so it is pointed at the sentence the defect left on
    // the screen, and at the row that replaces it.
    assert!(
        names_a_ticket(
            "indexing 0 % · unknown left (no store handle in the shell — e1.4 owns the worker)"
        ),
        "the old row named a closed ticket and this scanner missed it"
    );
    assert!(names_a_ticket("owned by reco-j7t"));
    assert!(!names_a_ticket(
        "indexing 40 % · ~2 s left · 200 / 500 commits"
    ));
    assert!(!names_a_ticket(
        "run `fathom index` here, then reopen fathom"
    ));
}

#[test]
fn no_answer_c_r_gives_names_a_closed_ticket_as_open_work() {
    for (what, source) in [
        ("no producer at all", None),
        (
            "a snapshot",
            Some(Source::snapshot(
                "`--no-watch`",
                "reopen without the flag and this frame watches the repository itself",
            )),
        ),
        (
            "a live producer",
            Some(Source::live(ScriptedRun::new(vec![Work::finished(
                "analysing",
                "commit",
                1,
                Some(1),
            )]))),
        ),
    ] {
        let mut app = shell(source);
        app.feed_key("<C-r>", Instant::now());
        let said = app.toast.clone().unwrap_or_default();
        assert!(!said.is_empty(), "`<C-r>` on {what} said nothing at all");
        assert!(
            !names_a_ticket(&said),
            "the answer about {what} names a ticket as if it were pending work: {said}"
        );
        assert!(
            !said.contains("no store handle") && !said.contains("owns the worker"),
            "the answer about {what} narrates the shell's own limits instead of what to do: {said}"
        );
    }
}

#[test]
fn a_frame_with_no_producer_names_the_command_rather_than_its_own_anatomy() {
    // The shipped shell always installs a producer, so this is the embedder's frame and the test frame —
    // which is exactly why the sentence has to be a remedy and not an excuse.
    let mut app = shell(None);
    app.feed_key("<C-r>", Instant::now());
    let said = app.toast.clone().unwrap_or_default();
    assert!(
        said.contains("fathom index"),
        "a refusal that names no command the reviewer can type is a dead end: {said}"
    );
    assert!(
        app.model.index.progress.is_none(),
        "no run started, so no run may be reported: {:?}",
        app.model.index.progress
    );
}

#[test]
fn an_update_that_carries_only_a_progress_row_is_not_an_empty_one() {
    // The loop draws nothing for an update that says nothing. A run mid-slice says something: the row.
    let mut idle = Update::default();
    assert!(idle.is_empty());
    idle.work = Some(Work::running("analysing", "commit", 1, Some(2)));
    assert!(!idle.is_empty(), "a progress row alone is worth a frame");
}
