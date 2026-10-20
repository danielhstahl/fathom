//! The glance pane (`reco-8bx`): what it draws, what it refuses, and what it costs to keep it open.
//!
//! The four acceptance criteria are about shape and cost, and both are testable without a terminal: the
//! renderer is a pure function of handed-over facts at a requested size (ADR-002 §3, ADR-006 §6.5), and the
//! loop is written against [`GlanceSink`], so a `Vec<Vec<String>>` in a test drives exactly the code path
//! the real screen drives.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fathom_analyze::insight::{QueueCounts, RiskView};
use fathom_core::time::Timestamp;
use fathom_tui::ambient::{
    self, End, FLOOR, Feed, Glance, HeadlessSink, IDEAL, IndexState, Latest, Mode, Need,
    QueueState, Reason, RunOptions,
};
use fathom_tui::event::{Bus, Message};
use fathom_tui::index::Read;
use fathom_tui::style::Face;

const NOW: Timestamp = Timestamp::from_unix_micros(1_700_000_000_000_000);

/// A feed that hands back whatever a test last put in it, and counts how often it was asked.
///
/// It stops on a wall-clock deadline rather than a poll count, because a paused pane asks for nothing and a
/// test that only stopped on polls would then wait forever — which is the same reason `fathom ambient
/// --deadline` is a duration and not a number of ticks.
#[derive(Clone)]
struct Scripted {
    /// The facts, replaced between polls.
    facts: Arc<Mutex<Glance>>,
    asked: Arc<Mutex<u64>>,
    deadline: Arc<Mutex<Instant>>,
}

impl Scripted {
    fn new(glance: Glance) -> Scripted {
        Scripted {
            facts: Arc::new(Mutex::new(glance)),
            asked: Arc::new(Mutex::new(0)),
            deadline: Arc::new(Mutex::new(Instant::now() + Duration::from_millis(80))),
        }
    }

    fn set(&self, glance: Glance) {
        *self.facts.lock().expect("the test holds the only lock") = glance;
    }

    fn asked(&self) -> u64 {
        *self.asked.lock().expect("the test holds the only lock")
    }
}

impl Feed for Scripted {
    fn glance(&mut self, _force: bool) -> Glance {
        let mut asked = self.asked.lock().expect("the test holds the only lock");
        *asked += 1;
        self.facts
            .lock()
            .expect("the test holds the only lock")
            .clone()
    }

    fn expired(&self) -> bool {
        Instant::now() >= *self.deadline.lock().expect("the test holds the only lock")
    }
}

/// The facts a busy repository would hand the pane: a new agent episode, a queue with something high in
/// it, an index that is current, and a watcher doing its job.
fn busy() -> Glance {
    Glance {
        now: NOW,
        root: ".".to_owned(),
        branch: Some("main".to_owned()),
        latest: Some(Latest {
            id: "e8f1a2b3c4d5e6f7".to_owned(),
            summary: "adds a cycle and 288 untested source lines".to_owned(),
            author: "agent".to_owned(),
            created: Some(Timestamp::from_unix_micros(
                NOW.as_unix_micros() - 12_000_000,
            )),
            state: "unread".to_owned(),
            provisional: false,
            risk: Some(RiskView::scored(82)),
        }),
        queue: QueueState {
            known: true,
            counts: Some(QueueCounts {
                total: 12,
                open: 7,
                high: 2,
                medium: 3,
                low: 2,
                snoozed: 2,
                ..QueueCounts::default()
            }),
            over_day: Some(1),
        },
        index: IndexState {
            read: Read::Present,
            epoch: Some(41),
            status: Some("ready".to_owned()),
            episodes: Some(3412),
            last_utc: Some(NOW.as_unix_micros() / 1_000 - 3),
            head_indexed: Some(true),
        },
        watch: fathom_tui::ambient::WatchState {
            here: false,
            present: true,
            alive: true,
            summary: "idle · polling · beat 0s ago".to_owned(),
            age: Some(Duration::from_secs(0)),
        },
        reasons: vec![Reason::new(
            Need::HighRisk,
            "e8f1a2 adds a cycle and 288 untested source lines",
        )],
        notes: Vec::new(),
        drill: Some("e8f1a2b3c4d5e6f7".to_owned()),
    }
}

/// The same repository after the reviewer caught up: nothing open, and nothing wrong with the pane.
fn caught_up() -> Glance {
    Glance {
        reasons: Vec::new(),
        latest: None,
        drill: None,
        queue: QueueState {
            known: true,
            counts: Some(QueueCounts {
                total: 12,
                accepted: 9,
                dismissed: 3,
                ..QueueCounts::default()
            }),
            over_day: Some(0),
        },
        ..busy()
    }
}

fn at(glance: &Glance, cols: usize, rows: usize) -> Vec<String> {
    ambient::text(glance, Mode::default(), cols, rows)
}

// ---------------------------------------------------------------------------------------------
// criterion 1: it renders at 80×12, and in a narrow tmux pane
// ---------------------------------------------------------------------------------------------

#[test]
fn the_pane_fits_eighty_columns_and_twelve_rows() {
    let lines = at(&busy(), IDEAL.0, IDEAL.1);
    assert_eq!(
        lines.len(),
        IDEAL.1,
        "the box is the size that was asked for"
    );
    for line in &lines {
        assert!(
            line.chars().count() <= IDEAL.0,
            "a line escaped 80 columns: {line:?}"
        );
    }
    let text = lines.join("\n");
    for fact in ["latest", "queue  ", "index  ", "look now"] {
        assert!(text.contains(fact), "missing the {fact:?} line:\n{text}");
    }
    // The four facts ADR-006 §5.7's mock promises, in the mock's own words.
    assert!(text.contains("risk 82 ✱ high"), "{text}");
    assert!(text.contains("7 of 12 open"), "{text}");
    assert!(text.contains("epoch 41"), "{text}");
    assert!(text.contains("adds a cycle"), "{text}");
    // The keys the pane offers, including the one that drills in.
    assert!(text.contains("<Enter>"), "{text}");
}

#[test]
fn it_fits_the_forty_column_tmux_pane_adr_001_promises() {
    let lines = at(&busy(), FLOOR.0, FLOOR.1);
    assert_eq!(lines.len(), FLOOR.1, "three rows is the promised floor");
    for line in &lines {
        assert!(
            line.chars().count() <= FLOOR.0,
            "a line escaped 40 columns: {line:?}"
        );
    }
    // The ladder keeps the three facts a glance is made of: newest, waiting, now-or-not.
    assert!(lines[0].starts_with("latest e8f1a2"), "{lines:?}");
    assert!(lines[1].starts_with("queue  "), "{lines:?}");
    assert!(lines[2].starts_with("look now"), "{lines:?}");
}

#[test]
fn below_the_floor_it_refuses_instead_of_drawing_a_lie() {
    let lines = at(&busy(), FLOOR.0 - 1, IDEAL.1);
    let text = lines.join("\n");
    assert!(text.contains("AMBIENT REFUSED"), "{text}");
    assert!(text.contains("40×3"), "the remedy names the floor: {text}");
    assert!(
        text.contains("--once"),
        "and the verb that works at any size: {text}"
    );
    assert!(
        !lines[1..].iter().any(|line| line.contains("latest")),
        "a refusal is the whole frame: {text}"
    );
}

#[test]
fn the_ladder_trims_in_the_order_the_layout_document_sells_it() {
    let full = at(&busy(), IDEAL.0, IDEAL.1);
    // Six rows: everything except the keys, which is the first thing a glance pane can live without.
    let six = at(&busy(), IDEAL.0, 6);
    assert_eq!(
        &six[..5],
        &full[..5],
        "the first five lines never move: {six:?}"
    );
    assert!(!six.iter().any(|line| line.contains("<Enter>")), "{six:?}");
    // Four rows: the index line stays, the gap and the reason collapse into it.
    let four = at(&busy(), IDEAL.0, 4);
    assert_eq!(
        four[2], full[2],
        "the index line is the fourth promise: {four:?}"
    );
    assert_eq!(four[3], full[4], "and then the reason: {four:?}");
}

// ---------------------------------------------------------------------------------------------
// criterion 4: the caught-up state is distinct and unambiguous
// ---------------------------------------------------------------------------------------------

#[test]
fn caught_up_says_so_in_words_and_in_ink() {
    let glance = caught_up();
    assert!(glance.caught_up());
    let lines = ambient::lines(&glance, Mode::default(), IDEAL.0, IDEAL.1);
    let text = lines
        .iter()
        .map(|line| line.text().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("caught up"), "{text}");
    assert!(!text.contains("look now"), "{text}");
    assert!(
        text.contains("12 of 12 episodes dealt with"),
        "it counts what it means: {text}"
    );
    // Distinct in the ink as well as the words: nothing on a quiet pane is loud.
    for line in &lines {
        for span in line.spans() {
            assert!(
                matches!(span.face, Face::Faint | Face::Muted),
                "a caught-up pane printed a loud face: {:?}",
                span.face
            );
        }
    }
}

#[test]
fn a_stale_pane_is_never_caught_up() {
    // "Nothing is waiting" over an index that stopped an hour ago is a broken pane, not a resting one, and
    // the two have to be told apart by the same rule every time.
    let mut glance = caught_up();
    glance.reasons.push(Reason::new(
        Need::NothingWatching,
        "nothing has indexed for 1h",
    ));
    assert!(!glance.caught_up());
    let text = at(&glance, IDEAL.0, IDEAL.1).join("\n");
    assert!(text.contains("index stale"), "{text}");
    assert!(!text.contains("caught up"), "{text}");
    assert!(
        text.contains("0 open"),
        "the queue line stays factual: {text}"
    );
}

#[test]
fn the_order_of_reasons_is_the_order_of_now() {
    // A high-risk episode is the loudest fact, but a pane that cannot see HEAD is the one that has to be
    // fixed first — the ranking of `Need` is the argument, and it is stated once, here.
    let mut glance = busy();
    glance.reasons.push(Reason::new(
        Need::HeadNotIndexed,
        "`HEAD` (abc1234) is not in the index",
    ));
    let text = at(&glance, IDEAL.0, IDEAL.1).join("\n");
    assert!(text.contains("index behind"), "{text}");
    assert!(
        !text.contains("adds a cycle"),
        "two reasons, one line: {text}"
    );
}

#[test]
fn an_unread_queue_that_was_never_counted_says_unknown() {
    let glance = Glance {
        queue: QueueState {
            known: false,
            counts: None,
            over_day: None,
        },
        ..busy()
    };
    let text = at(&glance, IDEAL.0, IDEAL.1).join("\n");
    assert!(text.contains("unknown"), "{text}");
    assert!(!text.contains("0 open"), "{text}");
}

// ---------------------------------------------------------------------------------------------
// criterion 2: no redraws without events, and no animation
// ---------------------------------------------------------------------------------------------

#[test]
fn the_same_facts_draw_the_same_bytes() {
    // No animation loops and no colour strobing: the renderer has no clock and no counter, so two renders
    // of one state are one string. This is the property the loop's "redraw only if it changed" rests on.
    let glance = busy();
    let first = ambient::fingerprint(&glance, Mode::default(), IDEAL.0, IDEAL.1);
    let second = ambient::fingerprint(&glance, Mode::default(), IDEAL.0, IDEAL.1);
    assert_eq!(first, second);
}

#[test]
fn ages_are_bucketed_so_a_quiet_minute_redraws_a_handful_of_times() {
    assert_eq!(ambient::age(0), "now");
    assert_eq!(ambient::age(4), "now");
    assert_eq!(ambient::age(9), "<15s");
    assert_eq!(ambient::age(20), "15s");
    assert_eq!(ambient::age(90), "1m");
    assert_eq!(ambient::age(7_200), "2h");
    assert_eq!(ambient::age(200_000), "2d");
    assert_eq!(ambient::ago(3), "now");
    assert_eq!(ambient::ago(90), "1m ago");
}

#[test]
fn a_quiet_repository_draws_one_frame_and_no_more() {
    let mut sink = HeadlessSink {
        size: (80, 12),
        ..Default::default()
    };
    let report = run_with(Scripted::new(busy()), &mut sink);
    assert!(report.polls >= 5, "the loop must poll: {:?}", report);
    assert_eq!(report.draws, 1, "one frame, then nothing: {:?}", report);
    assert_eq!(sink.frames.len(), 1);
    assert!(sink.frame(0).join("\n").contains("look now"));
}

#[test]
fn a_change_wakes_the_pane_immediately() {
    let feed = Scripted::new(busy());
    let later = {
        let mut glance = busy();
        glance.reasons = vec![Reason::new(Need::OpenQueue, "the queue moved")];
        glance
    };
    let mut sink = HeadlessSink {
        size: (80, 12),
        ..Default::default()
    };
    let feed2 = feed.clone();
    let bus = Bus::new();
    let bus2 = Arc::clone(&bus);
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        feed2.set(later);
        // A wake-up, the way a worker's message wakes the shell's loop.
        bus2.send(Message::Repaint);
    });
    let mut feed = feed;
    let report = fathom_tui::ambient::run_loop(
        &mut feed,
        &bus,
        &mut sink,
        None,
        &RunOptions {
            poll: Duration::from_millis(5),
            ..RunOptions::default()
        },
    );
    thread.join().expect("the producer ran");
    assert_eq!(report.draws, 2, "{:?}", report);
    assert!(sink.frames[1].join("\n").contains("the queue moved"));
}

#[test]
fn q_leaves_the_pane() {
    let (report, _) = drive(busy(), &[key(KeyCode::Char('q'))]);
    assert_eq!(report.end, End::Quit);
}

#[test]
fn enter_asks_for_the_pane_s_own_episode() {
    let (report, _) = drive(busy(), &[key(KeyCode::Enter)]);
    assert_eq!(
        report.end,
        End::Drill {
            episode: Some("e8f1a2b3c4d5e6f7".to_owned())
        }
    );
}

#[test]
fn a_paused_pane_stops_asking_for_facts() {
    let (report, feed) = drive(busy(), &[key(KeyCode::Char('p'))]);
    assert_eq!(report.end, End::Expired, "the loop runs on after `p`");
    let after_pause = feed.asked();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        feed.asked(),
        after_pause,
        "a paused pane must not keep reading the repository"
    );
}

#[test]
fn r_re_reads_even_when_nothing_moved() {
    let feed = Scripted::new(busy());
    let asked_before = feed.asked();
    let (report, _) = drive_with(feed.clone(), &[key(KeyCode::Char('r'))]);
    assert!(
        feed.asked() > asked_before,
        "r must force a read: {:?}",
        report
    );
}

#[test]
fn the_help_key_replaces_the_key_line() {
    let glance = busy();
    let plain = ambient::text(&glance, Mode::default(), IDEAL.0, IDEAL.1).join("\n");
    let help = ambient::text(
        &glance,
        Mode {
            help: true,
            paused: false,
        },
        IDEAL.0,
        IDEAL.1,
    )
    .join("\n");
    assert!(plain.contains("? keys"), "{plain}");
    assert!(help.contains("this line"), "{help}");
    assert!(!help.contains("? keys"), "{help}");
}

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn key(code: KeyCode) -> Message {
    Message::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Run the loop with a scripted feed and a scripted list of keys, sending them all up front: the loop
/// coalesces them exactly as it does for a person typing faster than it draws.
fn drive(glance: Glance, keys: &[Message]) -> (fathom_tui::ambient::Report, Scripted) {
    drive_with(Scripted::new(glance), keys)
}

fn drive_with(feed: Scripted, keys: &[Message]) -> (fathom_tui::ambient::Report, Scripted) {
    let bus = Bus::new();
    for message in keys {
        bus.send(message.clone());
    }
    let mut sink = HeadlessSink {
        size: (80, 12),
        ..Default::default()
    };
    let mut feed = feed;
    let report = fathom_tui::ambient::run_loop(
        &mut feed,
        &bus,
        &mut sink,
        None,
        &RunOptions {
            poll: Duration::from_millis(5),
            ..RunOptions::default()
        },
    );
    (report, feed)
}

fn run_with(feed: Scripted, sink: &mut HeadlessSink) -> fathom_tui::ambient::Report {
    let mut feed = feed;
    let bus = Bus::new();
    fathom_tui::ambient::run_loop(
        &mut feed,
        &bus,
        sink,
        None,
        &RunOptions {
            poll: Duration::from_millis(2),
            ..RunOptions::default()
        },
    )
}
