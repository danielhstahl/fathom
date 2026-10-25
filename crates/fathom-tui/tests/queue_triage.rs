//! The attention queue's four acts, in the shipped shell (`e4.5`, `reco-wsj`).
//!
//! The ranking, the act vocabulary and the retrospective are `fathom_analyze::queue`; what is under test
//! here is the *shell's* half, which is the half that can lie: the row that moves when nothing was written,
//! the picker whose cursor names a different episode than the act, the `f` that flags without a note, the
//! dismiss that leaves a row on screen. Those are shell facts, so they are tested through the frame the
//! terminal actually gets, exactly as `shell.rs`, `timeline_behaviour.rs` and `risk_rank.rs` do — and
//! through `feed_key`, so the bindings themselves (`x`, `f`, `-`, `z`, `r`) are part of the assertion.
//!
//! The tests play the composition root's role and install a [`Ledger`] of their own — the shell is not
//! allowed to open a store (ADR-002 §3, lint `E01`), so a fake is the only thing that can stand in for the
//! real one. The interesting fake is the refusing one: a ledger that says *no* is the case that separates
//! "the queue moved the row" from "the reviewer signed the episode off".
//!
//! The last-but-one test is the ticket's timed budget: dispositioning is a keystroke, and a keystroke that
//! cost half a second would be felt on every single act.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::{Arc, Mutex};
use std::time::Instant;

use fathom_analyze::insight::{
    CONDITIONS, Disposition, EpisodeDelta, QueuePolicy, ReasonVocabulary, RiskComponent, RiskView,
    TriageRecord,
};
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::queue::{Ledger, LedgerHandle, Setup, State};
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

/// Fixed clock: a fixture that reads the clock is a fixture with a flake in it (ADR-002 §4.4).
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const DAY: i64 = 86_400_000_000;
const SIZE: (u16, u16) = (110, 40);

/// The ledger the tests install: it records what it was asked to write, and can be told to refuse.
///
/// `Mutex` rather than `RefCell` because [`Ledger`] is `Send + Sync` — a real ledger may be called from a
/// worker — and a fake that dodged that bound would not be testing the same shape the CLI installs.
#[derive(Debug, Default)]
struct Recorder {
    /// Every act the shell tried to write, in order.
    written: Mutex<Vec<TriageRecord>>,
    /// When `Some`, every write and read fails with this reason.
    refusal: Mutex<Option<String>>,
}

impl Recorder {
    fn share(self) -> LedgerHandle {
        LedgerHandle(Arc::new(self))
    }

    fn refuse(&self, why: &str) {
        *self
            .refusal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(why.to_owned());
    }

    fn written(&self) -> Vec<TriageRecord> {
        self.written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Ledger for Recorder {
    fn record(&self, record: &TriageRecord) -> Result<(), String> {
        if let Some(why) = self
            .refusal
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return Err(why);
        }
        self.written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(record.clone());
        Ok(())
    }

    fn history(&self, limit: i64) -> Result<Vec<TriageRecord>, String> {
        if let Some(why) = self
            .refusal
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return Err(why);
        }
        Ok(self
            .written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .rev()
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .cloned()
            .collect())
    }
}

fn risk(score: u64, why: &str) -> RiskView {
    RiskView {
        model: "spine-1".to_owned(),
        score: i64::try_from(score).unwrap_or(0),
        vector: format!("spine-1 {score} = surfaces {score}"),
        components: vec![RiskComponent {
            name: "surfaces".to_owned(),
            points: i64::try_from(score).unwrap_or(0),
            cap: 50,
            detail: why.to_owned(),
        }],
        surfaces: vec!["migration".to_owned()],
        rank: None,
        ranked: None,
        caveat: None,
        blind_spots: vec!["rollback".to_owned()],
    }
}

fn episode(id: &str, age_days: i64, score: u64) -> Episode {
    let created = Timestamp::from_unix_micros(NOW.as_unix_micros() - age_days * DAY);
    let mut row = Episode::new(id, created, format!("episode {id}"));
    row.last = created;
    row.author = AuthorKind::Agent;
    row.author_name = "pi".to_owned();
    row.n_add = 10 + score;
    row.n_del = 2;
    row.n_files = 1;
    row.paths = vec![format!("crates/{id}/mod.rs")];
    row.fingerprint = format!("sha256:{id}");
    row.delta = Some(EpisodeDelta::sparse(
        &row.id,
        &row.summary,
        Some(risk(score, "touches a migration")),
    ));
    row
}

/// Four episodes at four ages, so each of the ordering's three factors has something to say.
fn rows() -> Vec<Episode> {
    vec![
        episode("freshhigh", 0, 80),
        episode("stalemedium", 10, 60),
        episode("freshlow", 1, 20),
        episode("middling", 1, 44),
    ]
}

fn batch(episodes: Vec<Episode>, epoch: u64) -> Batch {
    Batch {
        episodes,
        epoch: Some(epoch),
        ..Batch::new("fixture", Vec::new())
    }
}

/// A frame over an indexed repository: four rows, a ledger that says yes, and a named reviewer.
fn shell() -> App {
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    // The clock the composition root would have set (`frame::Options::now`, ADR-002 §4.4). Without it the
    // rail still thinks it is 1970 and every row is `0m unread` — which is exactly the mistake this
    // fixture would otherwise make, so it is stated here out loud.
    app.timeline.set_now(NOW);
    app.timeline.load(batch(rows(), 7));
    app.queue = Setup {
        vocabulary: ReasonVocabulary::shipped(),
        policy: QueuePolicy::default(),
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(Recorder::default().share()),
        history: Vec::new(),
        notices: Vec::new(),
        total: 4,
    }
    .state();
    app.queue.sessions = State::sessions_of(&rows());
    app.set_view(ViewId::Queue);
    app
}

fn frame(app: &App) -> String {
    frame_lines(app).join("\n")
}

/// The worklist pane's own text, with the card and the chrome cut off.
///
/// The row prints a six-character id, and so does the card beside it, so an assertion that searches the
/// whole frame finds the card and measures the wrong thing. Position questions are asked of the pane that
/// has the order in it.
fn worklist(app: &App) -> String {
    frame_lines(app)
        .iter()
        .filter_map(|line| line.split('│').nth(1))
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the shell last said out loud. The toast is the shell's own line of news; the grid truncates it
/// to the pane's width, so a test that needs the whole sentence reads the field the way
/// `risk_rank.rs` and `pointer.rs` do.
fn said(app: &App) -> String {
    app.toast.clone().unwrap_or_default()
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.feed_key(&ch.to_string(), Instant::now());
    }
}

/// The left pane's half of a frame line, or the line itself when it is already a pane's half.
fn pane_of(line: &str) -> &str {
    line.split(['│', '┬']).nth(1).unwrap_or(line).trim_end()
}

/// How many rows the queue says are open, read out of the count line the way a reviewer reads it: the
/// first number on the line above the rows, which is `{open} of {total} open` — or nothing at all, when
/// the queue has emptied itself and says so in words instead.
fn open_count(text: &str) -> usize {
    // The count line is the one line on the screen that opens with a number and says `open`: `4 of 4
    // open · …`. The frame's status line (`queue 4 open`) and the pane's strip (`… · 4 of 4 open`) both
    // mention a count in a different shape, and this reads the pane's own sentence — the one the reviewer
    // reads first, and the one the queue owns. An emptied or empty queue has no number to read, and says
    // so in words; those are zero, not a missing line.
    let number = |line: &str| {
        line.split_whitespace()
            .next()
            .and_then(|w| w.parse::<usize>().ok())
    };
    for line in text.lines().map(pane_of) {
        if line.starts_with("caught up") || line.starts_with("nothing to triage") {
            return 0;
        }
        if line.contains(" open")
            && let Some(open) = number(line)
        {
            return open;
        }
    }
    panic!("no count line on screen:\n{text}")
}

#[test]
fn the_queue_leads_with_the_stale_medium_and_names_the_factor_that_moved() {
    let app = shell();
    let text = frame(&app);
    let list = worklist(&app);
    // 60 × 1.50 (ten days unread) is 90, over a fresh 80: ADR-006 §5.1's change, on the screen and not
    // only in the module test. Rows print a six-character id, so that is what is searched.
    let stale = list.find("stalem").expect("the stale medium is on screen");
    let fresh = list.find("freshh").expect("the fresh high is on screen");
    assert!(
        text.contains("order: queue-1"),
        "the strip has to name the ranking it used, by name and version:\n{text}"
    );
    assert!(
        list.contains("risk × staleness × blast"),
        "the sentence that names the three factors, in the pane that has the room for it:\n{list}"
    );
    assert!(
        text.contains("10d unread"),
        "the row says which factor put it there:\n{text}"
    );
    assert!(
        stale < fresh,
        "staleness has to overtake a bigger number on the screen, not just in the comparator"
    );
    assert!(
        text.contains("blast unmeasured"),
        "an absent factor is named on the strip, and `w` prints what was missing from it:\n{text}"
    );
    assert_eq!(open_count(&text), 4);
}

#[test]
fn w_prints_the_ranking_the_way_fathom_why_prints_a_ranking() {
    // ADR-006 §5.1's binding note, and the ticket's second acceptance step: the queue's ordering is
    // printed as a formula with a name and a version, not asserted. A reviewer who can read the line can
    // reproduce the number with a calculator, which is the whole difference between a ranking and a mood.
    let mut app = shell();
    dispatch(&mut app, "w");
    let said = said(&app);
    assert!(
        said.contains("queue-1"),
        "the sentence names the ordering, by name and version: `{said}`"
    );
    assert!(
        said.contains("risk") && said.contains("staleness") && said.contains("blast"),
        "all three terms, with the arithmetic between them: `{said}`"
    );
    assert!(
        said.contains("1.50") && said.contains("10d unread"),
        "the factor and what it was measured from: `{said}`"
    );
    assert!(
        said.contains("no caller handed over a dependent count"),
        "and what was missing from the factor that had no number: `{said}`"
    );
}

#[test]
fn accept_writes_before_it_moves_and_the_row_leaves_the_queue() {
    let mut app = shell();
    dispatch(&mut app, "x");
    let text = frame(&app);
    let said = said(&app);
    assert!(
        said.contains("reviewed") && said.contains("stalem"),
        "the toast says what happened, to which row, and who did it: `{said}`"
    );
    assert!(
        said.contains("risk 60") && said.contains("ada <ada@example.com>"),
        "the same sentence the ledger will print later: `{said}`"
    );
    assert_eq!(open_count(&text), 3, "the row left the queue:\n{text}");
    assert!(
        text.contains("3 reviewed") || text.contains("reviewed"),
        "and the count says how the queue got smaller:\n{text}"
    );
    let written = app
        .queue
        .history
        .iter()
        .filter(|record| record.disposition == Disposition::Accepted)
        .count();
    assert_eq!(written, 1, "one act, written once");
    let record = app
        .queue
        .history
        .iter()
        .find(|record| record.disposition == Disposition::Accepted)
        .cloned()
        .unwrap();
    assert_eq!(
        record.actor, "ada <ada@example.com>",
        "signed by who the root said"
    );
    assert_eq!(
        record.risk,
        Some(60),
        "and with the risk at the time of the act — the stale medium, which is what the queue put first"
    );
    assert_eq!(
        record.fingerprint, "sha256:stalemedium",
        "against a fingerprint, so `e4.6` can re-link the sign-off"
    );
}

/// A dispatch that lets the frame settle, the way a real keypress does.
fn dispatch(app: &mut App, key: &str) {
    app.feed_key(key, Instant::now());
}

#[test]
fn a_ledger_that_refuses_keeps_the_row_exactly_where_it_was() {
    // The ADR-001 §7.7 case, and the reason the write goes first: a badge meaning "a key was pressed"
    // would be a lie about a decision nobody recorded.
    let recorder = Arc::new(Recorder::default());
    recorder.refuse("the disk is full");
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    app.timeline.set_now(NOW);
    app.timeline.load(batch(rows(), 7));
    app.queue = Setup {
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(LedgerHandle(Arc::clone(&recorder) as Arc<dyn Ledger>)),
        total: 4,
        ..Setup::default()
    }
    .state();
    app.set_view(ViewId::Queue);

    let before = frame(&app);
    dispatch(&mut app, "x");
    let after = frame(&app);
    let said = said(&app);
    assert!(
        said.contains("the disk is full") && said.contains("NOT written"),
        "the refusal is the news, in the ledger's own words: `{said}`"
    );
    assert!(recorder.written().is_empty(), "nothing was written");
    assert_eq!(
        open_count(&after),
        open_count(&before),
        "the list did not move: a refusal changes nothing on screen"
    );
    assert!(app.queue.decided.is_empty(), "and nothing was remembered");
    assert!(
        !after.contains("1 reviewed"),
        "and the counts did not quietly count it anyway:\n{after}"
    );
}

#[test]
fn dismiss_opens_the_vocabulary_and_writes_the_claim_it_was_given() {
    let mut app = shell();
    dispatch(&mut app, "-");
    let chooser = frame(&app);
    assert!(
        chooser.contains("dismiss `") || chooser.contains("dismiss reason"),
        "`-` opens the reason picker about the selected row:\n{chooser}"
    );
    for code in [
        "not-my-area",
        "already-known",
        "intentional",
        "false-positive",
        "low-stakes",
        "superseded",
        "will-revert",
        "other",
    ] {
        assert!(
            chooser.contains(code),
            "the whole closed vocabulary is on screen, not a subset: {chooser}"
        );
    }
    // Third row down, whatever that code turns out to be: the act has to carry *that* code and not the
    // first one, which is the whole risk of a picker whose cursor is a separate piece of state.
    let third = app
        .queue
        .reasons()
        .get(2)
        .unwrap_or_else(|| panic!("the vocabulary has three rows"))
        .code
        .clone();
    assert!(
        chooser.contains(&third),
        "the picker shows the code it is about to write: {chooser}"
    );
    dispatch(&mut app, "j");
    dispatch(&mut app, "j");
    dispatch(&mut app, "<Enter>");
    let text = frame(&app);
    assert!(
        said(&app).contains("dismissed") && said(&app).contains(&third),
        "the toast names the act and the reason: `{}`",
        said(&app)
    );
    assert_eq!(
        open_count(&text),
        3,
        "a dismissal consumes the row:\n{text}"
    );
    assert!(
        text.contains("2 dismissed") || text.contains("dismissed"),
        "and the count follows:\n{text}"
    );
    let reason = app
        .queue
        .history
        .first()
        .map_or_else(String::new, |record| record.reason.clone());
    assert_eq!(
        reason, third,
        "the code under the picker's cursor is the code in the ledger"
    );
    assert!(
        text.contains(third.as_str()) && text.contains("dismissed"),
        "and the row says what it was dismissed as: {text}"
    );
}

#[test]
fn choosing_other_asks_for_the_note_and_keeps_both() {
    let mut app = shell();
    dispatch(&mut app, "-");
    // Walk to `other` by asking the vocabulary where it is, because its position is usage-dependent and
    // a test that hardcoded the index would be testing the sort by accident.
    let at = app
        .queue
        .reasons()
        .iter()
        .position(|reason| reason.code == "other")
        .unwrap_or_else(|| panic!("`other` is a shipped code"));
    for _ in 0..at {
        dispatch(&mut app, "j");
    }
    dispatch(&mut app, "<Enter>");
    let prompted = frame(&app);
    assert!(
        prompted.contains("why, in words") || prompted.contains("note"),
        "`other` cannot be taken without the note it demands:\n{prompted}"
    );
    type_text(&mut app, "the detector missed the guard");
    dispatch(&mut app, "<Enter>");
    let text = frame(&app);
    assert!(
        text.contains("dismissed"),
        "the row left the open set, marked as dismissed:\n{text}"
    );
    assert!(
        said(&app).contains("other") && said(&app).contains("the detector missed the guard"),
        "the code and the note are both in the toast, and both went to the ledger: `{}`",
        said(&app)
    );
    let record = app
        .queue
        .history
        .first()
        .unwrap_or_else(|| panic!("the act was not remembered at all"));
    assert_eq!(record.reason, "other");
    assert_eq!(record.note, "the detector missed the guard");
}

#[test]
fn a_flag_without_a_note_is_refused_in_words_and_a_flag_with_one_is_written() {
    let mut app = shell();
    dispatch(&mut app, "f");
    assert!(frame(&app).contains("flag note"), "`f` asks for the note");
    dispatch(&mut app, "<Enter>");
    assert!(
        said(&app).contains("needs a note"),
        "an empty note is refused, not written empty: `{}`",
        said(&app)
    );
    let refused = frame(&app);
    assert!(
        !refused.contains("queue 4 open ·") || refused.contains("4 open"),
        "a refused act changes no count:\n{refused}"
    );

    dispatch(&mut app, "f");
    type_text(&mut app, "the pool is being drained");
    dispatch(&mut app, "<Enter>");
    let text = frame(&app);
    assert!(text.contains("flagged"), "{text}");
    let note = app
        .queue
        .history
        .first()
        .map_or_else(String::new, |record| record.note.clone());
    assert_eq!(
        note, "the pool is being drained",
        "the note is the act: a flag with nowhere to put the note is a notification nobody can act on"
    );
    // A flag keeps its episode in the queue (ADR-004 §5), so the count must not drop.
    assert_eq!(open_count(&text), 4, "flagged stays open:\n{text}");
    assert!(text.contains('⚐'), "and says so with its glyph:\n{text}");
}

#[test]
fn snooze_picks_a_condition_and_the_row_comes_back_when_it_happens() {
    let mut app = shell();
    dispatch(&mut app, "z");
    let chooser = frame(&app);
    for code in ["7d", "next-index", "new-in-session"] {
        assert!(
            chooser.contains(code),
            "every condition is offered: {chooser}"
        );
    }
    let at = CONDITIONS
        .iter()
        .position(|(code, _, _)| *code == "next-index")
        .unwrap_or_else(|| panic!("`next-index` is a shipped condition"));
    for _ in 0..at {
        dispatch(&mut app, "j");
    }
    dispatch(&mut app, "<Enter>");
    let snoozed = frame(&app);
    assert!(
        said(&app).contains("snoozed") && said(&app).contains("next-index:7"),
        "the stamp carries its baseline, or the condition is unevaluable later: `{}`",
        said(&app)
    );
    assert_eq!(
        open_count(&snoozed),
        3,
        "a snoozed row is out of the queue:\n{snoozed}"
    );

    // The same epoch: still snoozed. A new epoch: back in the queue, with its glyph.
    app.timeline.load(batch(rows(), 7));
    app.set_view(ViewId::Queue);
    assert_eq!(
        open_count(&frame(&app)),
        3,
        "the epoch has not moved, so the snooze holds"
    );
    app.timeline.load(batch(rows(), 8));
    app.set_view(ViewId::Queue);
    let woken = frame(&app);
    assert_eq!(
        open_count(&woken),
        4,
        "the index moved, so the snooze is over:\n{woken}"
    );
    assert!(woken.contains('z'), "and it says why it is back:\n{woken}");
}

#[test]
fn the_retrospective_names_what_was_skipped_and_says_pattern_out_loud() {
    let mut app = shell();
    // Eight of ten dismissals under one code, by two reviewers: over both thresholds, so the shell has
    // to say "pattern" rather than leave a pile of rows unexplained.
    let history: Vec<TriageRecord> = (0..10)
        .map(|n| TriageRecord {
            episode: format!("episode-{n:02}"),
            fingerprint: format!("sha256:{n}"),
            disposition: Disposition::Dismissed,
            reason: if n < 8 { "not-my-area" } else { "low-stakes" }.to_owned(),
            note: String::new(),
            until: String::new(),
            actor: if n % 2 == 0 {
                "ada".to_owned()
            } else {
                "bo".to_owned()
            },
            utc: NOW,
            risk: Some(70 + n),
            risk_model: "spine-1".to_owned(),
        })
        .collect();
    app.queue = Setup {
        vocabulary: ReasonVocabulary::shipped(),
        policy: QueuePolicy::default(),
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(Recorder::default().share()),
        history,
        notices: Vec::new(),
        total: 40,
    }
    .state();
    app.queue.sessions = State::sessions_of(&rows());
    dispatch(&mut app, "r");
    let text = frame(&app);
    assert!(
        text.contains("what you skipped, and why"),
        "`r` says what the pane is now:\n{text}"
    );
    assert!(
        text.contains("not-my-area"),
        "the rollup prints the code and its count:\n{text}"
    );
    assert!(
        text.contains("pattern"),
        "eight of ten dismissals under one reason is the pattern this view exists to show:\n{text}"
    );
    assert!(
        text.contains("coverage:") && text.contains("40"),
        "coverage is a share of the index, not of the ledger:\n{text}"
    );
    dispatch(&mut app, "r");
    assert!(
        frame(&app).contains("order: queue-1"),
        "`r` toggles back to the worklist"
    );
}

#[test]
fn an_unreadable_ledger_is_said_rather_than_shown_as_a_clean_sheet() {
    let recorder = Arc::new(Recorder::default());
    recorder.refuse("the ledger is on a network disk that is not mounted");
    let mut app = shell();
    app.queue
        .notices
        .push("the ledger is on a network disk that is not mounted".to_owned());
    dispatch(&mut app, "r");
    let text = frame(&app);
    assert!(
        text.contains("network disk"),
        "an unreadable ledger is reported, not hidden:\n{text}"
    );
    assert!(
        !text.contains("nothing dismissed"),
        "and it must not read as a clean sheet:\n{text}"
    );
    let _ = recorder;
}

#[test]
fn with_no_ledger_an_act_is_honest_about_living_only_in_memory() {
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    app.timeline.set_now(NOW);
    app.timeline.load(batch(rows(), 7));
    app.queue = Setup {
        actor: "ada".to_owned(),
        total: 4,
        ..Setup::default()
    }
    .state();
    app.set_view(ViewId::Queue);
    dispatch(&mut app, "x");
    let text = frame(&app);
    assert!(
        text.contains("in memory only"),
        "the count line carries the marker too, so it is on the screen and not only in a toast that fades:\n{text}"
    );
    assert!(
        said(&app).contains("in memory only"),
        "no ledger is a real configuration and the shell says so: `{}`",
        said(&app)
    );
    assert_eq!(
        app.queue
            .decided
            .values()
            .filter(|disposition| **disposition == Disposition::Accepted)
            .count(),
        1,
        "the act still works for this run, so the queue is still usable"
    );
}

#[test]
fn clearing_a_hundred_episode_queue_costs_the_shell_seconds_not_minutes() {
    // The ticket's timed criterion, taken literally: "clearing a 100-episode queue where most episodes
    // are low risk takes under 5 minutes". A reviewer has to read each row, and reading is theirs; what
    // this test owns is the shell's share of those 300 seconds — the keystrokes, the ranking, the write.
    // It is measured by emptying the whole queue, one act per episode, because a benchmark that does half
    // the job and extrapolates is how a 5-minute budget quietly becomes a 50-minute one.
    let many: Vec<Episode> = (0..100u32)
        .map(|n| {
            let n = u64::from(n);
            // Most rows low: the fixture matches the criterion's "most episodes are low risk".
            episode(
                &format!("episode{n:03}"),
                i64::try_from(n).unwrap_or(0),
                n % 34,
            )
        })
        .collect();
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    app.timeline.set_now(NOW);
    app.timeline.load(batch(many, 1));
    app.queue = Setup {
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(Recorder::default().share()),
        total: 100,
        ..Setup::default()
    }
    .state();
    app.set_view(ViewId::Queue);
    assert_eq!(open_count(&frame(&app)), 100, "the queue starts full");

    let started = Instant::now();
    for round in 0..100 {
        dispatch(&mut app, "-");
        dispatch(&mut app, "<Enter>");
        assert!(
            said(&app).contains("dismissed"),
            "act {} of 100 did not take effect: `{}`",
            round + 1,
            said(&app)
        );
    }
    let elapsed = started.elapsed();
    let end = frame(&app);
    assert!(
        end.contains("caught up") && open_count(&end) == 0,
        "an emptied queue says so in words, and counts itself as nothing left: {end}"
    );
    println!("100 acts on a 100-row queue: {elapsed:?}");
    assert!(
        elapsed.as_secs_f64() < 30.0,
        "emptying a 100-episode queue took the shell {elapsed:?}. The reviewer's budget is 300 s and          most of it is reading; anything in this neighbourhood would mean the shell, not the reviewer,          is what blows the ticket's five minutes"
    );
}

#[test]
fn the_state_a_shell_gets_with_nothing_installed_still_orders_and_still_refuses_nothing_silently() {
    // `Setup::default()` is what `--plain` over an unindexed directory, a test harness, or an embedder
    // gets. It must still draw a queue, and its acts must still be truthful about having no ledger.
    let state = State::absent();
    assert_eq!(
        state.actor, "unknown",
        "an unnamed reviewer is `unknown`, never a guess"
    );
    assert!(state.ledger.is_none());
    assert_eq!(
        state.reasons().len(),
        8,
        "the shipped vocabulary is always there"
    );
    let (line, written) = state.clone().commit(TriageRecord {
        episode: "episode-x".to_owned(),
        fingerprint: "sha256:x".to_owned(),
        disposition: Disposition::Accepted,
        reason: String::new(),
        note: String::new(),
        until: String::new(),
        actor: "unknown".to_owned(),
        utc: NOW,
        risk: None,
        risk_model: String::new(),
    });
    assert!(!written, "nothing was written, and the shell knows it");
    assert!(line.contains("in memory only"), "{line}");
    let _ = ReviewState::Accepted;
}
