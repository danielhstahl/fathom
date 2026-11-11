//! The four review acts, taken from the timeline (`e10.2`, ADR-006 §5.1.2 §2 and §4).
//!
//! `queue_triage.rs` owns the queue's own half of `e4.5` — the ranking, the pickers, the counts of a
//! worklist. What lives here is the half that moved: the acts are `global` bindings and always were, so
//! what `e10.2` moved is the **host of the feedback**, and the feedback is now the rail's row and the
//! rail's strip rather than a row leaving a list. Three of these tests are the ones ADR-006 §5.1.2 §2
//! names as moving with the acts rather than being rewritten green — write-before-state-change, the
//! refusing ledger that keeps the row exactly where it was, and the no-ledger run that says it lives only
//! in memory — and a fourth, the timed 100-episode budget, is the same criterion restated with the
//! timeline's filter and motion in it.
//!
//! The assertions are the queue's, raised rather than relaxed. In the worklist a row leaving the list was
//! proof enough; on a chronology nothing leaves, so what has to be proved is finer: the cell that changed,
//! the number the strip prints, the fact that the cursor did *not* move.
//!
//! As in `queue_triage.rs`, the tests play the composition root's role and install their own [`Ledger`]:
//! the shell may not open a store (ADR-002 §3), so a fake is the only thing that can stand in for the real
//! one, and the refusing fake is the case that separates "the reviewer signed it off" from "a key was
//! pressed".
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::{Arc, Mutex};
use std::time::Instant;

use fathom_analyze::insight::{
    CONDITIONS, Disposition, EpisodeDelta, ReasonVocabulary, RiskComponent, RiskView, TriageRecord,
};
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::queue::{Ledger, LedgerHandle, Setup, State};
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::{AuthorKind, Batch, Episode, Filter, GroupBy, ReviewState};
use fathom_tui::views::{self, ViewId};

/// Fixed clock: a fixture that reads the clock is a fixture with a flake in it (ADR-002 §4.4).
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const DAY: i64 = 86_400_000_000;
const SIZE: (u16, u16) = (110, 40);

/// The ledger the tests install: it records what it was asked to write, and can be told to refuse.
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

/// Four episodes at four ages, newest first — the rail's order, which is the only order it has.
fn rows() -> Vec<Episode> {
    vec![
        episode("freshhigh", 0, 80),
        episode("freshlow", 1, 20),
        episode("middling", 1, 44),
        episode("stalemedium", 10, 60),
    ]
}

/// Group nothing, so that one `j` is one episode. The default grouping is by day, and a day header is a
/// slot the cursor can stand on with no episode under it — true of the real rail, and a thing these tests
/// would otherwise be about by accident.
fn no_grouping(app: &mut App) {
    app.timeline.set_filter(Filter {
        group: GroupBy::None,
        ..Filter::default()
    });
}

fn batch(episodes: Vec<Episode>, epoch: u64) -> Batch {
    Batch {
        episodes,
        epoch: Some(epoch),
        ..Batch::new("fixture", Vec::new())
    }
}

/// A timeline frame over an indexed repository: four rows, a ledger that says yes, a named reviewer.
fn shell() -> App {
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    no_grouping(&mut app);
    app.timeline.set_now(NOW);
    app.timeline.load(batch(rows(), 7));
    app.queue = Setup {
        vocabulary: ReasonVocabulary::shipped(),
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(Recorder::default().share()),
        total: 4,
        ..Setup::default()
    }
    .state();
    app.queue.sessions = State::sessions_of(&rows());
    app.set_view(ViewId::Timeline);
    app
}

fn frame(app: &App) -> String {
    frame_lines(app).join("\n")
}

/// The rail pane as the frame draws it — which is not always the rail: `r` and every act's chooser take
/// the pane for a different subject, and a test that went straight to `rail_text` would be asking a
/// different question from the one on screen.
fn rail(app: &App) -> String {
    let panes = views::view(ViewId::Timeline).panes;
    let pane = panes
        .iter()
        .find(|pane| pane.name == "rail")
        .unwrap_or_else(|| panic!("the timeline has a rail pane"));
    app.timeline_text(pane).unwrap_or_default().join("\n")
}

/// The strip under the rail, which is where the counts live.
fn strip(app: &App) -> String {
    let panes = views::view(ViewId::Timeline).panes;
    let pane = panes
        .iter()
        .find(|pane| pane.name == "rail")
        .unwrap_or_else(|| panic!("the timeline has a rail pane"));
    app.timeline_strip(pane).unwrap_or_default()
}

/// What the shell last said out loud.
fn said(app: &App) -> String {
    app.toast.clone().unwrap_or_default()
}

fn dispatch(app: &mut App, key: &str) {
    app.feed_key(key, Instant::now());
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.feed_key(&ch.to_string(), Instant::now());
    }
}

/// The rail row for one episode, as drawn — the row whose first cell is the episode's short id, once
/// the cursor marker (`▸` or two spaces) is off the front of it.
fn row_of(app: &App, id: &str) -> String {
    let short = short(id);
    rail(app)
        .lines()
        .map(|line| line.trim_start().trim_start_matches(['\u{25b8}', ' ']))
        .find(|line| line.starts_with(&short))
        .unwrap_or_else(|| panic!("no row for `{id}` on the rail:\n{}", rail(app)))
        .trim_end()
        .to_owned()
}

/// The rail's six-cell spelling of an id: what the first column of a row prints.
fn short(id: &str) -> String {
    id.chars().take(6).collect()
}

/// The review-state glyph of one row — the last cell of the row, which is what the column ladder keeps
/// until last precisely because it is the cell that carries the state (`e5.6`).
fn glyph_of(app: &App, id: &str) -> String {
    row_of(app, id)
        .chars()
        .next_back()
        .map(String::from)
        .unwrap_or_default()
}

/// The `N unread` the strip prints — the open-work count, read the way a reviewer reads it.
fn unread_count(app: &App) -> usize {
    let text = strip(app);
    number_before(&text, "unread")
        .unwrap_or_else(|| panic!("the strip prints no unread count: {text}"))
}

/// The `N snoozed` the strip prints, or zero when it says none — a conditional part, said only when true.
fn snoozed_count(app: &App) -> usize {
    number_before(&strip(app), "snoozed").unwrap_or(0)
}

/// The number that sits before `word` in a ` · `-separated strip.
fn number_before(text: &str, word: &str) -> Option<usize> {
    text.split(" · ").find_map(|part| {
        part.ends_with(word)
            .then(|| part.split_whitespace().next())
            .flatten()
            .and_then(|number| number.replace(',', "").parse::<usize>().ok())
    })
}

/// Every episode id whose row still reads `⚑` — the rows the rail is still asking about.
fn unread_rows(app: &App) -> usize {
    rail(app)
        .lines()
        .filter(|line| line.trim_end().ends_with(ReviewState::Unread.glyph()))
        .count()
}

// --- the acts, on the rail ------------------------------------------------------------------------

/// The moved test (`queue_triage.rs::accept_writes_before_it_moves_and_the_row_leaves_the_queue`), with
/// the queue's proof replaced by the rail's: nothing leaves a chronology, so the row itself has to say
/// `✔`, in the same frame, with no second view to confirm it (ADR-006 §5.1.2 §2). The write still happens
/// before anything on screen changes, and that half of the name is the half that matters.
#[test]
fn accept_writes_before_it_marks_and_the_row_reads_accepted_in_the_same_frame() {
    let mut app = shell();
    let before = glyph_of(&app, "freshhigh");
    assert_eq!(
        before,
        ReviewState::Unread.glyph(),
        "the fixture starts unread, or there is nothing to change"
    );

    dispatch(&mut app, "x");

    let text = frame(&app);
    let said = said(&app);
    assert!(
        said.contains("reviewed") && !said.contains("stalem"),
        "the toast names the act and the row it was taken on: `{said}`"
    );
    assert!(
        said.contains("risk 80") && said.contains("ada <ada@example.com>"),
        "the same sentence the ledger will print later: `{said}`"
    );
    assert_eq!(
        glyph_of(&app, "freshhigh"),
        ReviewState::Accepted.glyph(),
        "the row reads as accepted in the frame the keystroke caused, with no second view to confirm it:\n{text}"
    );
    assert_eq!(
        unread_count(&app),
        3,
        "and the strip's open-work count dropped by one in the same frame:\n{}",
        strip(&app)
    );
    assert_eq!(
        unread_rows(&app),
        3,
        "the count and the rows agree — four rows, three of them still asking:\n{}",
        rail(&app)
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
        record.episode, "freshhigh",
        "the act is about the row under the cursor"
    );
    assert_eq!(
        record.actor, "ada <ada@example.com>",
        "signed by who the root said"
    );
    assert_eq!(
        record.risk,
        Some(80),
        "and with the risk at the moment of the act, never recomputed later"
    );
    assert_eq!(
        record.fingerprint, "sha256:freshhigh",
        "against a fingerprint, so `e4.6` can re-link the sign-off"
    );
}

/// The moved test, on the rail: the ledger says *no*, and the rail does not change by one cell.
#[test]
fn a_ledger_that_refuses_keeps_the_row_exactly_where_it_was() {
    // The ADR-001 §7.7 case, and the reason the write goes first: a glyph meaning "a key was pressed"
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
    app.set_view(ViewId::Timeline);

    // The toast is the news and is allowed to change; the rail and its strip are not.
    let before = format!("{}\n--\n{}", rail(&app), strip(&app));
    let unread = unread_count(&app);
    let cursor = app
        .timeline
        .selected()
        .map(|episode| episode.id.clone())
        .unwrap_or_default();

    dispatch(&mut app, "x");

    let after = format!("{}\n--\n{}", rail(&app), strip(&app));
    assert!(
        said(&app).contains("the disk is full") && said(&app).contains("NOT written"),
        "the refusal is the news, in the ledger's own words: `{}`",
        said(&app)
    );
    assert!(recorder.written().is_empty(), "nothing was written");
    assert_eq!(
        before, after,
        "a refusal changes nothing on the rail or in its counts:\n{after}"
    );
    assert_eq!(
        unread_count(&app),
        unread,
        "not even the count the act would have moved"
    );
    assert!(app.queue.decided.is_empty(), "and nothing was remembered");
    assert_eq!(
        app.timeline
            .selected()
            .map(|episode| episode.id.clone())
            .unwrap_or_default(),
        cursor,
        "and the cursor did not move either"
    );
    assert!(
        !rail(&app).contains(ReviewState::Accepted.glyph()),
        "and no row claims a decision the ledger refused:\n{}",
        rail(&app)
    );
}

/// The moved test: no ledger is a real configuration, and the rail says so on screen and not only in a
/// toast that fades.
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
    app.set_view(ViewId::Timeline);

    dispatch(&mut app, "x");

    let text = frame(&app);
    assert!(
        said(&app).contains("in memory only"),
        "no ledger is a real configuration and the shell says so: `{}`",
        said(&app)
    );
    assert!(
        rail(&app).contains("in memory only") || text.contains("in memory only"),
        "and the marker is on the screen, not only in a toast that fades:\n{text}"
    );
    assert_eq!(
        glyph_of(&app, "freshhigh"),
        ReviewState::Accepted.glyph(),
        "the act still works for this run, so the rail is still usable"
    );
    assert_eq!(
        app.queue
            .decided
            .values()
            .filter(|disposition| **disposition == Disposition::Accepted)
            .count(),
        1
    );
}

/// ADR-006 §5.1.2 §2, said out loud because it is a decision and not an oversight: the rail is a
/// chronology and the cursor is a place in time. A keystroke that jumped forward would break the next
/// keystroke, which is `d` on *that* episode nine times in ten.
#[test]
fn an_act_leaves_the_cursor_on_the_row_it_just_changed() {
    let mut app = shell();
    let before = app
        .timeline
        .selected()
        .map(|episode| episode.id.clone())
        .unwrap_or_default();

    dispatch(&mut app, "x");
    assert_eq!(
        app.timeline
            .selected()
            .map(|episode| episode.id.clone())
            .unwrap_or_default(),
        before,
        "`x` did not move the cursor down the rail"
    );

    dispatch(&mut app, "-");
    dispatch(&mut app, "<Enter>");
    assert_eq!(
        app.timeline
            .selected()
            .map(|episode| episode.id.clone())
            .unwrap_or_default(),
        before,
        "and neither did a dismissal — the row keeps its place in the chronology"
    );

    // The row it stayed on is the row the next act takes, which is the whole point: the reviewer works a
    // run of rows with `j`, or with the filter that already exists, not with a cursor that moves itself.
    dispatch(&mut app, "f");
    type_text(&mut app, "the pool is being drained");
    dispatch(&mut app, "<Enter>");
    assert_eq!(
        app.timeline
            .selected()
            .map(|episode| episode.id.clone())
            .unwrap_or_default(),
        before,
        "`f` either"
    );
    assert_eq!(
        glyph_of(&app, &before),
        ReviewState::Flagged.glyph(),
        "and all three acts landed on the same row"
    );
}

/// The strip's open-work count is the number the four acts are supposed to move. A flag keeps its episode
/// in the open set (ADR-004 §5), a snooze takes it out and puts a `z` on the line, and a dismissal is
/// neither — the counts have to say the same thing the rows do.
#[test]
fn the_strips_open_work_count_is_the_acts_that_move_it() {
    let mut app = shell();
    assert_eq!(unread_count(&app), 4);
    assert_eq!(
        snoozed_count(&app),
        0,
        "nothing is snoozed and the strip says nothing"
    );

    dispatch(&mut app, "f");
    type_text(&mut app, "the pool is being drained");
    dispatch(&mut app, "<Enter>");
    assert_eq!(
        unread_count(&app),
        3,
        "a flag is a decision about the row, so it is no longer unread:\n{}",
        strip(&app)
    );
    assert_eq!(glyph_of(&app, "freshhigh"), ReviewState::Flagged.glyph());

    dispatch(&mut app, "j");
    dispatch(&mut app, "z");
    for _ in 0..CONDITIONS
        .iter()
        .position(|(code, _, _)| *code == "next-index")
        .unwrap_or_else(|| panic!("`next-index` is a shipped condition"))
    {
        dispatch(&mut app, "j");
    }
    dispatch(&mut app, "<Enter>");
    assert_eq!(
        glyph_of(&app, "freshlow"),
        ReviewState::Snoozed.glyph(),
        "`z` picked a condition and stamped it"
    );
    assert_eq!(
        snoozed_count(&app),
        1,
        "and the strip's snoozed count rose in the same frame:\n{}",
        strip(&app)
    );
    assert!(
        said(&app).contains("next-index:7"),
        "the condition carries the baseline it will be checked against: `{}`",
        said(&app)
    );
}

/// A `state=unread` rail may not go on displaying a row that has since been signed off: the filter reads
/// the marker, so the act re-filters. This is the queue's "the row leaves the list", kept honest on the
/// rail for the one filter where leaving is what the filter asked for.
#[test]
fn a_filtered_rail_lets_the_row_go_when_the_act_makes_it_not_match() {
    let mut app = shell();
    app.timeline.set_filter(Filter {
        state: Some(ReviewState::Unread),
        group: GroupBy::None,
        ..Filter::default()
    });
    assert_eq!(
        rail(&app).lines().filter(|line| line.contains('⚑')).count(),
        4,
        "the filter is the reviewer's own way of working a run of rows:\n{}",
        rail(&app)
    );

    dispatch(&mut app, "x");
    let text = rail(&app);
    assert!(
        !text.contains("freshhigh") && !text.contains(&short("freshhigh")),
        "the accepted row left the filtered rail:\n{text}"
    );
    assert_eq!(
        unread_count(&app),
        3,
        "and the count went with it:\n{}",
        strip(&app)
    );
}

// --- the retrospective on the rail ----------------------------------------------------------------

/// The moved test (`the_retrospective_names_what_was_skipped_and_says_pattern_out_loud`), with `r` on the
/// rail: same rollup, same honesty rule, different pane (ADR-006 §5.1.2 §4).
#[test]
fn the_retrospective_names_what_was_skipped_and_says_pattern_out_loud() {
    let mut app = shell();
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
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(Recorder::default().share()),
        history,
        total: 40,
        ..Setup::default()
    }
    .state();
    app.set_view(ViewId::Timeline);

    dispatch(&mut app, "r");
    let text = rail(&app);
    assert!(
        text.contains("what you skipped, and why"),
        "`r` says what the rail is now:\n{text}"
    );
    assert!(
        text.contains("not-my-area"),
        "the rollup prints the code and its count:\n{text}"
    );
    assert!(
        text.contains("pattern"),
        "eight of ten dismissals under one reason is the pattern this state exists to show:\n{text}"
    );
    assert!(
        text.contains("coverage:") && text.contains("40"),
        "coverage is a share of the index, not of the ledger:\n{text}"
    );
    assert!(
        strip(&app).starts_with("ledger: ")
            && strip(&app).contains("10 acts")
            && strip(&app).contains("10 dismissed"),
        "the strip is replaced wholesale and says so:\n{}",
        strip(&app)
    );
    assert!(
        strip(&app).contains("ada <ada@example.com>"),
        "and names who the ledger is being read as:\n{}",
        strip(&app)
    );

    dispatch(&mut app, "r");
    assert!(
        rail(&app).contains(&short("freshhigh")),
        "`r` again puts the episodes back:\n{}",
        rail(&app)
    );
    assert!(
        strip(&app).contains("filter: ") && !strip(&app).starts_with("ledger: "),
        "and the strip goes back to the rail's own — the filter sentence is back and the ledger sentence \
         is gone:\n{}",
        strip(&app)
    );
}

/// The moved honesty rule, verbatim: an unreadable ledger prints its complaint and the "nothing was
/// dismissed" line is withheld, because silence plus a clean sheet is the lie a reviewer would act on.
#[test]
fn an_unreadable_ledger_is_said_rather_than_shown_as_a_clean_sheet() {
    let recorder = Arc::new(Recorder::default());
    recorder.refuse("the ledger is on a network disk that is not mounted");
    let mut app = shell();
    app.queue
        .notices
        .push("the ledger is on a network disk that is not mounted".to_owned());
    dispatch(&mut app, "r");
    let text = rail(&app);
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

/// `r` is a `global` binding now, so the question is answerable from wherever the reviewer is standing —
/// and the answer is on the rail, so that is where the key goes.
#[test]
fn r_from_any_view_puts_the_timeline_on_screen_listing_acts() {
    for view in [ViewId::Queue, ViewId::Diff, ViewId::Structure] {
        let mut app = shell();
        app.set_view(view);
        dispatch(&mut app, "r");
        assert_eq!(
            app.view,
            ViewId::Timeline,
            "`r` from {} lands on the timeline",
            views::view(view).title
        );
        assert!(
            rail(&app).contains("what you skipped, and why"),
            "and the rail is listing acts, not episodes:\n{}",
            rail(&app)
        );
    }
}

/// `<Esc>` pops exactly one thing (§R2). The acts listing is a state of a pane, so it is what `<Esc>` pops
/// when it is on — the reviewer stays in the timeline they were reading.
#[test]
fn esc_leaves_the_acts_listing_without_leaving_the_timeline() {
    let mut app = shell();
    dispatch(&mut app, "r");
    assert_eq!(app.view, ViewId::Timeline);
    dispatch(&mut app, "<Esc>");
    assert_eq!(
        app.view,
        ViewId::Timeline,
        "`<Esc>` did not throw the reviewer out of the view"
    );
    assert!(
        rail(&app).contains(&short("freshhigh")),
        "and put the episodes back:\n{}",
        rail(&app)
    );
}

/// A run with no ledger still answers `r`, and says the answer is not durable. A rollup of in-memory
/// acts above a strip that implied they were recorded is the claim this ticket exists to keep honest.
#[test]
fn an_acts_listing_with_no_ledger_says_nothing_was_written() {
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    app.timeline.set_now(NOW);
    app.timeline.load(batch(rows(), 7));
    app.queue = Setup {
        actor: "ada".to_owned(),
        total: 4,
        ..Setup::default()
    }
    .state();
    app.set_view(ViewId::Timeline);
    dispatch(&mut app, "x");
    dispatch(&mut app, "r");
    assert_eq!(
        strip(&app),
        "no ledger — nothing written this run",
        "the strip says the whole truth about a ledger that is not there"
    );
}

// --- the pickers, on the rail ---------------------------------------------------------------------

/// The pickers are the acts' other half, and their host moved with them: `-` taken from the rail draws
/// its chooser in the rail pane, about the row the rail's cursor is on.
#[test]
fn dismiss_from_the_rail_opens_the_vocabulary_in_the_rail_pane() {
    let mut app = shell();
    dispatch(&mut app, "-");
    let chooser = rail(&app);
    assert!(
        chooser.contains(&format!("dismiss `{}`", short("freshhigh"))),
        "the chooser names the row it is about, in the pane the reviewer is looking at:\n{chooser}"
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
            "the whole closed vocabulary is on screen, not a subset:\n{chooser}"
        );
    }
    // Third row down, whatever that code turns out to be: the picker's cursor is separate state and the
    // act must carry the code under *it*, not the first one.
    let third = app
        .queue
        .reasons()
        .get(2)
        .unwrap_or_else(|| panic!("the vocabulary has three rows"))
        .code
        .clone();
    dispatch(&mut app, "j");
    dispatch(&mut app, "j");
    dispatch(&mut app, "<Enter>");
    assert!(
        said(&app).contains("dismissed") && said(&app).contains(&third),
        "the toast names the act and the reason: `{}`",
        said(&app)
    );
    assert_eq!(
        glyph_of(&app, "freshhigh"),
        ReviewState::Dismissed.glyph(),
        "the row says what it was dismissed as:\n{}",
        rail(&app)
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
}

/// `f` without a note is refused in words, and the refusal changes nothing — the same rule the worklist
/// enforced, on the row the rail is pointing at.
#[test]
fn a_flag_without_a_note_is_refused_in_words_and_a_flag_with_one_is_written() {
    let mut app = shell();
    dispatch(&mut app, "f");
    assert!(rail(&app).contains("flag note"), "`f` asks for the note");
    dispatch(&mut app, "<Enter>");
    assert!(
        said(&app).contains("needs a note"),
        "an empty note is refused, not written empty: `{}`",
        said(&app)
    );
    assert_eq!(
        glyph_of(&app, "freshhigh"),
        ReviewState::Unread.glyph(),
        "and the refusal left the row alone"
    );
    assert_eq!(unread_count(&app), 4, "and left the count alone");

    dispatch(&mut app, "f");
    type_text(&mut app, "the pool is being drained");
    dispatch(&mut app, "<Enter>");
    assert_eq!(
        glyph_of(&app, "freshhigh"),
        ReviewState::Flagged.glyph(),
        "{}",
        rail(&app)
    );
    let note = app
        .queue
        .history
        .first()
        .map_or_else(String::new, |record| record.note.clone());
    assert_eq!(note, "the pool is being drained");
}

// --- the timed budget, on the rail ----------------------------------------------------------------

/// The moved `e4.5` criterion (`reco-wsj`'s five minutes), re-hosted on the rail with its filter and
/// motion included, exactly as ADR-006 §5.1.2 §2 requires — and with the cursor moved by the reviewer's
/// own `j`, never by the act.
#[test]
fn clearing_a_hundred_episode_rail_costs_the_shell_seconds_not_minutes() {
    let many: Vec<Episode> = (0..100u32)
        .map(|n| {
            let n = u64::from(n);
            episode(
                &format!("episode{n:03}"),
                i64::try_from(n).unwrap_or(0),
                n % 34,
            )
        })
        .collect();
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "repo");
    no_grouping(&mut app);
    app.timeline.set_now(NOW);
    app.timeline.load(batch(many, 1));
    app.queue = Setup {
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(Recorder::default().share()),
        total: 100,
        ..Setup::default()
    }
    .state();
    app.set_view(ViewId::Timeline);
    assert_eq!(unread_count(&app), 100, "the rail starts full");

    let started = Instant::now();
    for round in 0..100 {
        // `j` is the reviewer walking the rail; the act never walks it for them.
        if round > 0 {
            dispatch(&mut app, "j");
        }
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

    assert_eq!(
        unread_count(&app),
        0,
        "every row is dispositioned and the strip knows it:\n{}",
        strip(&app)
    );
    assert_eq!(
        app.queue
            .decided
            .values()
            .filter(|disposition| **disposition == Disposition::Dismissed)
            .count(),
        100,
        "one hundred acts, one hundred rows, none of them the same row twice"
    );
    println!("100 acts on a 100-row rail: {elapsed:?}");
    assert!(
        elapsed.as_secs_f64() < 30.0,
        "emptying a 100-episode rail took the shell {elapsed:?}. The reviewer's budget is 300 s and most of it is reading; anything in this neighbourhood would mean the shell, not the reviewer, is what blows the ticket's five minutes"
    );
}
