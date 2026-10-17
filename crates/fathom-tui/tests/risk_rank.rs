//! The ranked worklist, in the shipped shell (`reco-c05`).
//!
//! `reco-glv`'s acceptance step (b) — "find and read the three riskiest episodes" — failed on the
//! 500-commit fixture for two reasons: the queue listed episodes in the order the store returned them
//! with no ordering anyone could defend, and pressing `w` printed `not implemented`. Both live in this
//! shell, so both are tested here, through the renderer the terminal actually gets (as in `shell.rs`
//! and `timeline_behaviour.rs`: a frame is only worth asserting if it is the real frame).
//!
//! The shell may not *rank* anything (ADR-006 §2.1; ADR-002 §5.2, lint `E01`): a risk arrives as a
//! number with its parts attached, computed by `fathom-analyze::risk` and handed over by the
//! composition root. So these tests play the composition root's role — they build the rows an index
//! would hand over, in the chronological order a store read returns, with scores deliberately out of
//! order — and then assert the shell's three obligations: **order** them, **say why**, and **say what
//! the number cannot see**.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::time::Instant;

use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState, Risk};
use fathom_tui::views::ViewId;

/// Fixed clock, as every timeline fixture uses: a fixture that reads the clock is a fixture with a
/// flake in it (ADR-002 §4.4).
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const MINUTE: i64 = 60_000_000;
const SIZE: (u16, u16) = (110, 34);

/// A risk in the shape the composition root hands over: the model that made it, its arithmetic, its
/// reasons and its ceiling. `rank`/`ranked` are the caller's — a place in a set is not a fact about
/// one episode — so a fixture that wants the printed rank has to say what set it came from.
fn risk(score: u64, rank: Option<u64>, ranked: u64, why: &str) -> Risk {
    Risk {
        model: "spine-1".to_owned(),
        score,
        vector: format!("spine-1 {score} = surfaces {score}"),
        reasons: vec![format!("surfaces {score}/50 — {why}")],
        badge: "migration".to_owned(),
        rank,
        ranked: Some(ranked),
        blind_spots: vec![
            "whether a schema migration can be rolled back".to_owned(),
            "whether a new environment variable is ever read by the program".to_owned(),
        ],
    }
}

/// Four episodes, in the order a chronological read returns them, scored out of order so that a queue
/// which sorted by time cannot pass. The fourth carries no risk at all, which is every row the shipped
/// build had.
fn rows() -> Vec<Episode> {
    let make = |id: &str, ago: i64, score: u64, rank: Option<u64>, why: &str| {
        let created = Timestamp::from_unix_micros(NOW.as_unix_micros() - ago);
        let mut episode = Episode::new(id, created, format!("episode {id}"));
        episode.last = created;
        episode.author = AuthorKind::Agent;
        episode.author_name = "pi".to_owned();
        episode.author_confidence = Some(0.94);
        episode.n_add = 10;
        episode.n_del = 2;
        episode.n_files = 3;
        episode.paths = vec![format!("db/migrations/{id}.sql")];
        episode.state = ReviewState::Unread;
        episode.risk = (score > 0).then(|| risk(score, rank, 3, why));
        episode
    };
    vec![
        make(
            "e-aaaa",
            4 * MINUTE,
            41,
            Some(3),
            "a workflow change travels with the artefact",
        ),
        make(
            "e-bbbb",
            3 * MINUTE,
            88,
            Some(1),
            "a schema migration that may not roll back",
        ),
        make("e-cccc", 2 * MINUTE, 0, None, "nothing ranked this episode at all"),
        make(
            "e-dddd",
            MINUTE,
            64,
            Some(2),
            "a lockfile the reviewer did not choose",
        ),
    ]
}

fn loaded(rows: Vec<Episode>) -> App {
    let mut app = App::new(Keymap::defaults(), Model::from_facade(), SIZE, "harbour · main");
    app.timeline.set_now(NOW);
    app.set_view(ViewId::Queue);
    app.load_episodes(Batch::new("fixture index", rows));
    app
}

fn keystroke(app: &mut App, chord: &str) {
    app.feed_key(chord, Instant::now());
}

/// The worklist pane's body as the shell draws it.
fn worklist(app: &mut App) -> Vec<String> {
    let pane = app
        .current()
        .panes
        .iter()
        .find(|pane| pane.name == "worklist")
        .expect("the queue view has a worklist pane");
    app.queue_text(pane)
        .expect("the worklist has text once the index has rows")
}

/// The four ids in the order the worklist drew them.
fn order(app: &mut App) -> Vec<String> {
    worklist(app)
        .iter()
        .filter_map(|line| {
            ["e-bbbb", "e-dddd", "e-aaaa", "e-cccc"]
                .into_iter()
                .find(|id| line.contains(id))
                .map(str::to_owned)
        })
        .collect()
}

// --- the ordering ---------------------------------------------------------------------------------

#[test]
fn the_queue_is_ranked_and_not_chronological() {
    let app = &mut loaded(rows());
    let lines = worklist(app);
    let text = lines.join("\n");
    assert_eq!(
        order(app),
        vec!["e-bbbb", "e-dddd", "e-aaaa", "e-cccc"],
        "88, then 64, then 41, then the unranked row last:\n{text}"
    );
    // The rank is printed, because "which three" should not make a reviewer count rows.
    let first = lines.first().cloned().unwrap_or_default();
    assert!(first.contains('1'), "the top row is numbered 1: {first}");
    assert!(first.contains("88"), "and carries its score: {first}");
    let frame = frame_lines(app).join("\n");
    assert!(
        frame.contains("rank 1 of 3"),
        "and the card says how many were ranked to produce that number:\n{frame}"
    );
}

#[test]
fn the_strip_names_the_ranking_its_ceiling_and_what_it_is_not() {
    // ADR-006 §5.1: a strip that does not say *how* it ordered asks for trust. The strip is the pane's
    // own line, so it is read off the pane rather than out of a frame that may be too narrow for it.
    let app = &mut loaded(rows());
    let pane = app
        .current()
        .panes
        .iter()
        .find(|pane| pane.name == "worklist")
        .expect("the queue view has a worklist pane");
    let strip = app
        .queue_strip(pane)
        .expect("a worklist with rows always says how it ordered them");
    assert!(strip.contains("spine-1"), "the model: {strip}");
    assert!(
        strip.contains("3 ranked of 4 held"),
        "how many of the held rows carry a ranking: {strip}"
    );
    assert!(
        strip.contains("cannot see 2 things"),
        "and how much of the question it cannot answer: {strip}"
    );
    assert!(strip.contains("e4.5"), "and what this list is not: {strip}");
}

#[test]
fn the_first_three_rows_are_the_answer_to_which_three() {
    // The literal question the ticket exists because nobody could answer it in the shipped build.
    let app = &mut loaded(rows());
    let mut top = order(app);
    top.truncate(3);
    assert_eq!(
        top,
        vec!["e-bbbb", "e-dddd", "e-aaaa"],
        "the three riskiest, in order, on the first screen"
    );
}

#[test]
fn each_row_carries_its_own_reason() {
    let app = &mut loaded(rows());
    let text = worklist(app).join("\n");
    assert!(
        text.contains("may not roll back"),
        "the reason sits under its own row:\n{text}"
    );
    assert!(
        text.contains("lockfile the reviewer did not choose"),
        "and every row prints its own, not the selected one's:\n{text}"
    );
}

#[test]
fn an_unranked_index_still_draws_a_queue_and_says_what_is_missing() {
    // The degraded case is the shipped case until an index runs the ranking: the worklist must stay a
    // worklist, and must never print a missing score as zero (ADR-001 §7.7).
    let mut rows = rows();
    for episode in &mut rows {
        episode.risk = None;
    }
    let app = &mut loaded(rows);
    let text = worklist(app).join("\n");
    for id in ["e-aaaa", "e-bbbb", "e-dddd", "e-cccc"] {
        assert!(
            text.contains(id),
            "no episode is dropped because it has no score: {id}\n{text}"
        );
    }
    assert!(text.contains("unranked"), "the absence is stated:\n{text}");
    assert!(text.contains("e4.4"), "and names who will fill it:\n{text}");
    assert!(!text.contains("risk 0"), "a missing score is never a zero:\n{text}");
    assert!(text.contains("risk ?"), "and is printed as unknown:\n{text}");
    let pane = app
        .current()
        .panes
        .iter()
        .find(|pane| pane.name == "worklist")
        .expect("a pane");
    let strip = app.queue_strip(pane).unwrap_or_default();
    assert!(strip.contains("none ranked"), "{strip}");
}

// --- the explanation ------------------------------------------------------------------------------

#[test]
fn w_explains_the_selected_episode_and_esc_unexplains_it() {
    let app = &mut loaded(rows());
    assert_eq!(app.explain, None, "nothing is explained before it is asked for");
    let before = frame_lines(app).join("\n");
    assert!(
        before.contains("a spine cannot see"),
        "the card already says a ranking has a ceiling, and how many items are hidden:\n{before}"
    );
    assert!(
        !before.contains("environment variable"),
        "but it prints one blind spot and a count, not the list — the card has a size:\n{before}"
    );

    keystroke(app, "w");
    assert_eq!(app.explain.as_deref(), Some("e-bbbb"));
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|toast| toast.contains("spine-1 88")),
        "the toast carries the arithmetic, for a reader who will not open the card: {:?}",
        app.toast
    );

    let frame = frame_lines(app).join("\n");
    assert!(
        frame.contains("spine-1 88 = surfaces 88"),
        "the arithmetic is on the screen, never a bare number:\n{frame}"
    );
    assert!(
        frame.contains("environment variable"),
        "and the whole ceiling, not a `(1 more)`:\n{frame}"
    );

    keystroke(app, "<Esc>");
    assert_eq!(
        app.explain, None,
        "Esc dismisses the explanation before it unselects anything"
    );
    let frame = frame_lines(app).join("\n");
    assert!(
        !frame.contains("environment variable"),
        "and the card is the episode again:\n{frame}"
    );
}

#[test]
fn moving_on_ends_the_explanation_instead_of_letting_it_lie() {
    let app = &mut loaded(rows());
    keystroke(app, "w");
    keystroke(app, "j");
    assert_eq!(
        app.explain, None,
        "the explanation was of the episode that was selected, not of the pane"
    );
    assert_eq!(
        app.queue_selected().map(|episode| episode.id.as_str()),
        Some("e-dddd"),
        "and the cursor moved down the ranked list, not down the calendar"
    );
}

#[test]
fn a_refresh_keeps_the_reviewer_on_the_episode_they_were_reading() {
    // Background refreshes replace every row (ADR-006 §5.3). A worklist that re-resolved the cursor by
    // position, or re-sorted mid-read, would move the "top three" out from under the reviewer.
    let app = &mut loaded(rows());
    keystroke(app, "j");
    let before = app.queue_selected().map(|episode| episode.id.clone());
    assert_eq!(before.as_deref(), Some("e-dddd"));
    app.load_episodes(Batch::new("fixture index", rows()));
    assert_eq!(
        app.queue_selected().map(|episode| episode.id.clone()),
        before,
        "a refreshed worklist keeps the reviewer where they were"
    );
}

// --- the frame ------------------------------------------------------------------------------------

#[test]
fn a_worklist_frame_draws_exactly_at_every_promised_size() {
    // A worklist row is two lines tall and carries a reason beside it, so the ADR-006 §5.5 audit (the
    // selected row reachable at every promised size) has to hold with rows in the pane — not only with
    // the "nothing to rank yet" placeholder this build shipped with.
    for size in [(80u16, 24u16), (100, 30), (200, 60)] {
        let mut app = App::new(
            Keymap::defaults(),
            Model::from_facade(),
            size,
            "harbour · main",
        );
        app.timeline.set_now(NOW);
        app.set_view(ViewId::Queue);
        app.load_episodes(Batch::new("fixture index", rows()));
        let lines = frame_lines(&app);
        assert_eq!(lines.len(), usize::from(size.1), "height at {size:?}");
        for (index, line) in lines.iter().enumerate() {
            assert_eq!(
                fathom_tui::layout::display_width(line),
                usize::from(size.0),
                "row {index} at {size:?}: {line:?}"
            );
        }
        let text = lines.join("\n");
        assert!(
            text.contains("e-bbbb"),
            "the top of the worklist is on screen at {size:?}:\n{text}"
        );
    }
}

#[test]
fn the_status_line_counts_the_queue_the_shell_is_showing() {
    // Before this ticket the status row printed `queue unknown open` forever, because nothing in the
    // shell knew what its own queue held.
    let frame = frame_lines(&loaded(rows())).join("\n");
    assert!(
        frame.contains("queue 4 open"),
        "four unread episodes:\n{frame}"
    );
    assert!(
        !frame.contains("queue unknown"),
        "and not the placeholder this build shipped with:\n{frame}"
    );

    // The count is the rows on screen, so a read episode leaves it — which is also the case a
    // dispositioning key (`x`, `e4.6`) will move through later.
    let mut read = rows();
    read[1].state = ReviewState::Read;
    let frame = frame_lines(&loaded(read)).join("\n");
    assert!(
        frame.contains("queue 3 open"),
        "one already-read episode is not open:\n{frame}"
    );
}
