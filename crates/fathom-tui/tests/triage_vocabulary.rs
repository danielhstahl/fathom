//! The triage vocabulary and the snooze conditions, taken from the rail (`e4.5`, `reco-wsj`; re-hosted
//! by `e10.2`, re-hosted-again-and-renamed by `e10.3`, `reco-ydek.3`).
//!
//! This file used to be `queue_triage.rs`, and it said so: *"what is under test here is what the queue
//! owns and nothing else: the ranking and the factor it names, the closed dismiss vocabulary and the
//! snooze conditions as the worklist offers them, and the counts of a worklist — numbers that stop
//! meaning anything when the pane goes away in `e10.3`."* The pane went away, and this is the half that
//! outlived it, split from the file that held both halves:
//!
//! * **the acts' proofs** — write-before-state-change, the refusing ledger, the no-ledger run, the count
//!   an act moves — moved to `timeline_acts.rs` in `e10.2`, whole and renamed to what a rail has to
//!   prove, per ADR-006 §5.1.2 §2;
//! * **the ranking** left the build with the ranked pane (`e10.3`, ADR-006 §5.1.2 §5), and what remains
//!   of the score-on-a-row story is `risk_rank.rs`;
//! * **what is here** is the vocabulary a keystroke chooses from and the conditions a snooze is stamped
//!   with: `other` cannot be taken without its note, a `next-index` stamp carries the baseline it will be
//!   checked against, and a shell handed nothing is still honest about writing nothing. Those are
//!     properties of the ledger, which epic 10 kept.
//!
//! The tests still play the composition root's role and install a [`Ledger`] of their own — the shell is
//! not allowed to open a store (ADR-002 §3, lint `E01`), so a fake is the only thing that can stand in
//! for the real one — and they still go through the frame the terminal actually gets, via `feed_key`, so
//! the bindings themselves (`f`, `-`, `z`) are part of the assertion.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::{Arc, Mutex};
use std::time::Instant;

use fathom_analyze::insight::{
    CONDITIONS, Disposition, EpisodeDelta, ReasonVocabulary, RiskComponent, RiskView, TriageRecord,
};
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_lines;
use fathom_tui::review::{Ledger, LedgerHandle, Setup, State};
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

/// Fixed clock: a fixture that reads the clock is a fixture with a flake in it (ADR-002 §4.4).
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const DAY: i64 = 86_400_000_000;
const SIZE: (u16, u16) = (110, 40);

/// The ledger the tests install: it records what it was asked to write.
///
/// `Mutex` rather than `RefCell` because [`Ledger`] is `Send + Sync` — a real ledger may be called from a
/// worker — and a fake that dodged that bound would not be testing the same shape the CLI installs. The
/// refusing ledger is `timeline_acts.rs`'s: the act's host moved, and the case that separates "the
/// reviewer signed it off" from "a key was pressed" belongs beside the row that changes.
#[derive(Debug, Default)]
struct Recorder {
    /// Every act the shell wrote, in order.
    written: Mutex<Vec<TriageRecord>>,
}

impl Recorder {
    fn share(self) -> LedgerHandle {
        LedgerHandle(Arc::new(self))
    }
}

impl Ledger for Recorder {
    fn record(&self, record: &TriageRecord) -> Result<(), String> {
        self.written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(record.clone());
        Ok(())
    }

    fn history(&self, limit: i64) -> Result<Vec<TriageRecord>, String> {
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
        model: "spine-2".to_owned(),
        score: i64::try_from(score).unwrap_or(0),
        vector: format!("spine-2 {score} = surfaces {score}"),
        components: vec![RiskComponent {
            name: "surfaces".to_owned(),
            points: i64::try_from(score).unwrap_or(0),
            cap: 50,
            detail: why.to_owned(),
        }],
        surfaces: vec!["migration".to_owned()],
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
    row.state = ReviewState::Unread;
    row.delta = Some(EpisodeDelta::sparse(
        &row.id,
        &row.summary,
        Some(risk(score, "touches a migration")),
    ));
    row
}

/// Four episodes at four ages, so an act has something to act on in each review state.
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
    app.review = Setup {
        vocabulary: ReasonVocabulary::shipped(),
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(Recorder::default().share()),
        history: Vec::new(),
        notices: Vec::new(),
        total: 4,
    }
    .state();
    app.review.sessions = State::sessions_of(&rows());
    app.set_view(ViewId::Timeline);
    app
}

fn frame(app: &App) -> String {
    frame_lines(app).join("\n")
}

/// The rail pane's own text, with the card and the chrome cut off.
///
/// The row prints a six-character id, and so does the card beside it, so an assertion that searches the
/// whole frame finds the card and measures the wrong thing. Position questions are asked of the pane
/// that has the order in it.
fn rail(app: &App) -> String {
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

/// A dispatch that lets the frame settle, the way a real keypress does.
fn dispatch(app: &mut App, key: &str) {
    app.feed_key(key, Instant::now());
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

/// How many rows the rail says are unread, read out of the strip the way a reviewer reads it: the number
/// immediately before the word `unread` on the strip's line (ADR-006 §5.1.2 §3 part 2).
fn open_count(text: &str) -> usize {
    let number = |line: &str| {
        let words = line.split_whitespace().collect::<Vec<_>>();
        words
            .iter()
            .position(|word| *word == "unread")
            .and_then(|at| at.checked_sub(1))
            .and_then(|before| words[before].parse::<usize>().ok())
    };
    for line in text.lines().map(pane_of) {
        if line.starts_with("order:") && line.contains(" unread") {
            return number(line).unwrap_or_else(|| panic!("no count in the strip: {line}"));
        }
    }
    panic!("no strip line on screen:\n{text}")
}

#[test]
fn dismiss_opens_the_vocabulary_and_writes_the_claim_it_was_given() {
    // The chooser is the closed vocabulary, ordered by what this reviewer uses, and the code under its
    // cursor is the code in the ledger — that identity is the whole reason the picker exists instead of a
    // free-text field.
    let mut app = shell();
    dispatch(&mut app, "-");
    let chooser = frame(&app);
    for code in ["not-my-area", "other"] {
        assert!(chooser.contains(code), "the shipped codes are offered: {chooser}");
    }
    // Read off the list *before* the act, because the order is by usage and the act is usage: asking
    // afterwards would compare the ledger against a list the act itself re-sorted.
    let second = app
        .review
        .reasons()
        .iter()
        .nth(1)
        .map_or_else(String::new, |reason| reason.code.clone());
    dispatch(&mut app, "j");
    dispatch(&mut app, "<Enter>");
    let text = rail(&app);
    let reason = app
        .review
        .history
        .first()
        .map_or_else(String::new, |record| record.reason.clone());
    assert_eq!(
        reason, second,
        "the code under the picker's cursor is the code in the ledger"
    );
    assert!(
        text.contains(ReviewState::Dismissed.glyph()),
        "and the row says it was decided: {text}"
    );
    assert_eq!(
        app.review
            .disposition(app.timeline.selected().expect("a selected row")),
        Disposition::Dismissed,
        "with this claim behind it"
    );
}

#[test]
fn choosing_other_asks_for_the_note_and_keeps_both() {
    let mut app = shell();
    dispatch(&mut app, "-");
    // Walk to `other` by asking the vocabulary where it is, because its position is usage-dependent and
    // a test that hardcoded the index would be testing the sort by accident.
    let at = app
        .review
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
    let text = rail(&app);
    assert!(
        text.contains(ReviewState::Dismissed.glyph()),
        "the row left the open set, marked as dismissed:\n{text}"
    );
    assert!(
        said(&app).contains("other") && said(&app).contains("the detector missed the guard"),
        "the code and the note are both in the toast, and both went to the ledger: `{}`",
        said(&app)
    );
    let record = app
        .review
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
    assert_eq!(
        open_count(&rail(&app)),
        4,
        "a refused act changes no count:\n{}",
        rail(&app)
    );

    dispatch(&mut app, "f");
    type_text(&mut app, "the pool is being drained");
    dispatch(&mut app, "<Enter>");
    let text = rail(&app);
    assert!(text.contains(ReviewState::Flagged.glyph()), "{text}");
    let note = app
        .review
        .history
        .first()
        .map_or_else(String::new, |record| record.note.clone());
    assert_eq!(note, "the pool is being drained");
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
    let snoozed = rail(&app);
    assert!(
        said(&app).contains("snoozed") && said(&app).contains("next-index:7"),
        "the stamp carries its baseline, or the condition is unevaluable later: `{}`",
        said(&app)
    );
    assert_eq!(
        open_count(&snoozed),
        3,
        "a snoozed row is out of the open count:\n{snoozed}"
    );

    // The same epoch: still snoozed. A new epoch: back in the open count, with its glyph. The store's
    // marker is what a load hands over, so the fixture hands over the marker the act wrote — the shell's
    // own job is only to notice that the stamped condition has happened.
    let mut store = rows();
    store[0].state = ReviewState::Snoozed;
    app.load_episodes(batch(store.clone(), 7));
    assert_eq!(
        open_count(&rail(&app)),
        3,
        "the epoch has not moved, so the snooze holds"
    );
    app.load_episodes(batch(store, 8));
    let woken = rail(&app);
    assert_eq!(
        open_count(&woken),
        4,
        "the index moved, so the snooze is over:\n{woken}"
    );
    // The row is back as unread work, and the strip stops claiming a snooze it no longer holds: the
    // `z` marker was the *condition*'s mark, and a condition that has happened is not a condition.
    assert!(
        woken.contains(ReviewState::Unread.glyph()),
        "the released row reads as open work again:\n{woken}"
    );
    assert!(
        !woken.contains("snoozed"),
        "and the strip stops counting a snooze that is over:\n{woken}"
    );
}

#[test]
fn the_state_a_shell_gets_with_nothing_installed_still_refuses_nothing_silently() {
    // `Setup::default()` is what `--plain` over an unindexed directory, a test harness, or an embedder
    // gets. Its acts must still be truthful about having no ledger.
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
    let (line, landed) = state.clone().commit(TriageRecord {
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
    assert!(
        !landed.written(),
        "nothing was written, and the shell knows it"
    );
    assert!(
        landed.taken(),
        "and the act was still taken, honestly, for this run"
    );
    assert!(line.contains("in memory only"), "{line}");
}
