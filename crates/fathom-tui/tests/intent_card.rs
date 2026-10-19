//! The `i` pane and the card's intent line (`e4.2`, `reco-c54`).
//!
//! `fathom-analyze/tests/intent.rs` proves the store column and the delta document agree; `fathom-cli`'s
//! `intent.rs` proves `fathom show` prints them. What only the TUI can prove is the third surface — the one
//! a reviewer actually sits in — and the rule the ticket cares most about there: **an absence has to be
//! drawn**. A card that shows nothing when there is no intent is indistinguishable from a card whose intent
//! was never read, and those are the difference between "this agent worked unbriefed" and "this build is
//! old".
//!
//! Every assertion goes through the real renderer and the real keystroke, as in `timeline_behaviour.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use fathom_analyze::insight::EpisodeDelta;
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::style::StyledLine;
use fathom_tui::timeline::{self, AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

/// The instant the rail measures "now" from, as in the timeline corpus. Fixed, not `now`.
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
/// One minute in microseconds.
const MINUTE: i64 = 60_000_000;

/// Flatten rendered lines. Faces are asserted only where the face *is* the message.
fn text(lines: &[StyledLine]) -> Vec<String> {
    lines.iter().map(|line| line.text().to_owned()).collect()
}

/// Two rows: one with `intent` planted on it, one without anything at all. The second row exists so a test
/// can move the cursor out from under the pane.
fn app_with(intent: Option<serde_json::Value>) -> App {
    let mut first = Episode::new(
        "e8f1a2b3c4d5e6f7",
        Timestamp::from_unix_micros(NOW.as_unix_micros() - 12 * MINUTE),
        "make the card pane scroll",
    );
    first.author = AuthorKind::Agent;
    first.author_name = "pi".to_owned();
    first.n_files = 4;
    first.n_add = 412;
    first.n_del = 90;
    first.state = ReviewState::Unread;
    first.paths = vec!["crates/fathom-tui/src/timeline/card.rs".to_owned()];

    let mut delta = EpisodeDelta::sparse(&first.id, &first.summary, None);
    // The intent arrives as the store wrote it, so the fixture *is* the store's JSON rather than a
    // reconstruction of it. The document's type belongs to `fathom-ingest`, which this crate may not name
    // (ADR-002 §5.2, lint `M02`), so it is deserialized into the field rather than constructed here.
    delta.episode.intent = intent.and_then(|value| serde_json::from_value(value).ok());
    // The rail's session and turn columns come from the store row, which since `e4.2` falls back to the
    // session the *stream* named when no trailer did (`episodes::to_row`). The fixture copies the intent's
    // ids the same way rather than inventing a second pair.
    first.session = delta
        .episode
        .intent
        .as_ref()
        .and_then(|intent| intent.session_id.clone());
    first.turn = delta
        .episode
        .intent
        .as_ref()
        .and_then(|intent| intent.turn_id.clone());
    first.delta = Some(delta);

    let mut second = Episode::new(
        "9c4d10aabbccddee",
        Timestamp::from_unix_micros(NOW.as_unix_micros() - 41 * MINUTE),
        "cache the epoch probe",
    );
    second.author = AuthorKind::Agent;
    second.author_name = "pi".to_owned();
    second.paths = vec!["crates/fathom-store/src/spine.rs".to_owned()];

    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        (100, 32),
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.set_view(ViewId::Timeline);
    app.load_episodes(Batch::new("fixture index", vec![first, second]));
    app
}

/// A prompt the harness handed over, in the shape `fathom index` writes.
fn recorded() -> serde_json::Value {
    serde_json::json!({
        "schema": "fathom-intent/1",
        "state": "recorded",
        "source": "hook-prompt",
        "text": "make the card pane scroll without dropping the selection",
        "confidence": 0.95,
        "session_id": "s1",
        "turn_id": "s1:1",
        "plan": [{"text": "pin the cursor", "status": "in_progress"}, {"text": "golden-test it"}],
        "scope": ["crates/fathom-tui/**"],
        "evidence": [{"source": "hook-prompt", "confidence": 0.95, "detail": "prompt from the harness"}]
    })
}

/// The stream recorded a digest and nothing else, which is what every fixture in the corpus does.
fn digest_only() -> serde_json::Value {
    serde_json::json!({
        "schema": "fathom-intent/1",
        "state": "digest-only",
        "source": "prompt-digest",
        "text": "",
        "confidence": 0.3,
        "reason": "the harness recorded a prompt digest of 812 chars, not its text",
        "prompt": {"sha256": "aa", "text_captured": false},
        "plan": [],
        "scope": [],
        "evidence": []
    })
}

/// Every source was read and none of them said anything.
fn none_recorded() -> serde_json::Value {
    serde_json::json!({
        "schema": "fathom-intent/1",
        "state": "none",
        "source": "none",
        "text": "",
        "confidence": 0.0,
        "reason": "no harness events for this repository, and no `Intent:` or `Task:` trailer on the commit",
        "plan": [],
        "scope": [],
        "evidence": []
    })
}

fn keystroke(app: &mut App, chord: &str) {
    app.feed_key(chord, Instant::now());
}

/// The card pane as the app is drawing it — through `App`, so the pane's own size maths is under test too.
fn pane(app: &mut App) -> String {
    let width = app.pane_cols("card").max(40);
    let rows = app.pane_body_rows();
    text(&timeline::intent_lines(
        app.timeline.selected(),
        width,
        rows,
    ))
    .iter()
    .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
    .collect::<Vec<_>>()
    .join(" ")
}

// ---------------------------------------------------------------------------
// The keystroke
// ---------------------------------------------------------------------------

#[test]
fn i_raises_the_intent_pane_and_esc_puts_the_card_back() {
    let mut app = app_with(Some(recorded()));
    assert!(!app.showing_intent(app.timeline.selected()));
    keystroke(&mut app, "i");
    assert!(
        app.showing_intent(app.timeline.selected()),
        "`i` (`intent.show`) is registered and raises the pane"
    );
    keystroke(&mut app, "<Esc>");
    assert!(
        !app.showing_intent(app.timeline.selected()),
        "and `<Esc>` closes the topmost transient thing it owns"
    );
}

#[test]
fn i_toggles_on_the_same_episode() {
    let mut app = app_with(Some(recorded()));
    keystroke(&mut app, "i");
    keystroke(&mut app, "i");
    assert!(
        !app.showing_intent(app.timeline.selected()),
        "the same key that opened it closes it, rather than stacking a second pane"
    );
}

#[test]
fn the_pane_prints_the_request_the_plan_and_the_declared_scope() {
    let mut app = app_with(Some(recorded()));
    keystroke(&mut app, "i");
    let body = pane(&mut app);
    assert!(
        body.contains("make the card pane scroll without dropping the selection"),
        "the request in the asker's words:\n{body}"
    );
    assert!(
        body.contains("hook-prompt"),
        "and which rung decided:\n{body}"
    );
    assert!(
        body.contains("pin the cursor") && body.contains("golden-test it"),
        "the whole plan, in the harness's order:\n{body}"
    );
    assert!(
        body.contains("crates/fathom-tui/**"),
        "the scope that was declared, as a glob:\n{body}"
    );
    assert!(
        body.contains("declared"),
        "and the word that says this is a declaration, not a measurement:\n{body}"
    );
}

#[test]
fn the_card_carries_the_intent_line_and_the_session_it_arrived_on() {
    // The card, not the `i` pane: the session and the turn are the rail's columns and the card's own field,
    // while the pane is the plan and the scope. Both have to be right, and they are filled from the same row
    // (`episodes::to_row`'s fallback to the stream's session id).
    let app = app_with(Some(recorded()));
    let body = text(&timeline::card_lines(
        app.timeline.selected(),
        NOW,
        "fixture index",
        72,
        40,
    ))
    .iter()
    .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
    .collect::<Vec<_>>()
    .join(" ");
    assert!(
        body.contains("make the card pane scroll"),
        "the card prints the intent without anyone pressing `i`:\n{body}"
    );
    assert!(body.contains("s1"), "and the session:\n{body}");
    assert!(body.contains("s1:1"), "and the turn:\n{body}");
}

#[test]
fn the_pane_names_what_it_did_not_find() {
    // Three absences, three different sentences, because they mean three different things to the reviewer.
    let mut digest = app_with(Some(digest_only()));
    keystroke(&mut digest, "i");
    let body = pane(&mut digest);
    assert!(
        body.contains("no text captured"),
        "a digest is not a blank:\n{body}"
    );
    assert!(
        body.contains("812 chars"),
        "and the reason is the harness's own:\n{body}"
    );

    let mut silent = app_with(Some(none_recorded()));
    keystroke(&mut silent, "i");
    let body = pane(&mut silent);
    assert!(
        body.contains("no recorded intent"),
        "an absence is printed as an absence:\n{body}"
    );
    assert!(
        body.contains("no harness events"),
        "with what was looked for:\n{body}"
    );

    let mut legacy = app_with(None);
    keystroke(&mut legacy, "i");
    let body = pane(&mut legacy);
    assert!(
        body.contains("never looked"),
        "and a row this build wrote no intent for is a third fact, said as one:\n{body}"
    );
    assert!(
        !body.contains("no recorded intent"),
        "which must not borrow the second one's sentence:\n{body}"
    );
}

#[test]
fn the_toast_says_which_state_it_found() {
    // `i` is also the cheap question — "did this agent have a brief?" — answered before the pane is read.
    let mut app = app_with(Some(recorded()));
    keystroke(&mut app, "i");
    assert!(
        app.toast
            .clone()
            .unwrap_or_default()
            .starts_with("intent recorded"),
        "{:?}",
        app.toast
    );

    let mut digest = app_with(Some(digest_only()));
    keystroke(&mut digest, "i");
    assert!(
        digest
            .toast
            .clone()
            .unwrap_or_default()
            .contains("digest only"),
        "{:?}",
        digest.toast
    );

    let mut silent = app_with(Some(none_recorded()));
    keystroke(&mut silent, "i");
    assert!(
        silent
            .toast
            .clone()
            .unwrap_or_default()
            .contains("no recorded intent"),
        "{:?}",
        silent.toast
    );

    let mut legacy = app_with(None);
    keystroke(&mut legacy, "i");
    assert!(
        legacy
            .toast
            .clone()
            .unwrap_or_default()
            .contains("no intent document"),
        "the third state, in the toast too: {:?}",
        legacy.toast
    );
}

#[test]
fn the_pane_does_not_follow_the_cursor() {
    // The pane is keyed to an episode, not to a position. Moving the cursor must not leave the previous
    // episode's intent on screen: the card would then be describing a row the reviewer is not looking at.
    let mut app = app_with(Some(recorded()));
    keystroke(&mut app, "i");
    assert!(app.showing_intent(app.timeline.selected()));
    keystroke(&mut app, "j");
    assert!(
        !app.showing_intent(app.timeline.selected()),
        "the cursor moved to a row with no intent document, and the pane knows it"
    );
}

#[test]
fn a_narrow_pane_still_shows_the_state_and_wraps_the_request() {
    // The responsive contract (`e1.6`'s rule): content is wrapped or clipped, never dropped. The state is
    // the one thing that must survive a narrow terminal, because it is the answer to the question.
    let mut app = app_with(Some(recorded()));
    keystroke(&mut app, "i");
    let lines = text(&timeline::intent_lines(app.timeline.selected(), 24, 20))
        .into_iter()
        .map(|line| line.trim_end().to_owned())
        .collect::<Vec<_>>();
    assert!(
        lines.iter().all(|line| line.chars().count() <= 24),
        "nothing overflows the pane: {lines:?}"
    );
    let body = lines.join("\n");
    assert!(body.contains("intent"), "the label survives: {body}");
    assert!(
        body.contains("scroll"),
        "the request is wrapped, not dropped:\n{body}"
    );
    assert!(
        body.contains("hook-prompt"),
        "and so is the tail of the line, not just its start:\n{body}"
    );
}

#[test]
fn an_empty_selection_says_so() {
    let lines = text(&timeline::intent_lines(None, 60, 6)).join("\n");
    assert!(lines.contains("no episode selected"), "{lines}");
}
