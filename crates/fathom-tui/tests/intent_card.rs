//! The card's intent row, and the keystroke that used to own it (`e4.2`, `reco-c54`,
//! `reco-5js3.2`, `reco-5js3.3`).
//!
//! `fathom-analyze/tests/intent.rs` proves the store column and the delta document agree; `fathom-cli`'s
//! `intent.rs` proves `fathom show` prints them. What only the TUI can prove is the third surface — the one
//! a reviewer actually sits in — and the rule the ticket cares most about there: **an absence has to be
//! drawn**. A card that shows nothing when there is no intent is indistinguishable from a card whose intent
//! was never read, and those are the difference between "this agent worked unbriefed" and "this build is
//! old".
//!
//! `e9.1` spent that rule on a row instead of a mode, `e9.2` moved the assertions onto the row, and `e9.3`
//! deleted the mode: the `i` pane is gone, `intent.show` is out of the registry, and the letter is in
//! ADR-006 §8.2's reserved-for-user set. So this file asserts the same three states on the same frame, and
//! adds the half the pane used to answer badly — what `i` says now, which is where the answer went rather
//! than a shrug (ADR-006 §7.6, `reco-xf8`'s shape).
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

/// The card as the app is drawing it — through `App`, so the pane's own size maths is under test too.
fn pane(app: &mut App) -> String {
    let width = app.pane_cols("card").max(40);
    let rows = app.pane_body_rows();
    text(&app.timeline.preview_lines(width, rows, app.card_window()))
        .iter()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The card at a width, padded to the pane's own width and trimmed of that padding.
fn card(app: &App, width: usize) -> Vec<String> {
    timeline::card_lines(
        app.timeline.selected(),
        NOW,
        "fixture index",
        width,
        40,
        timeline::CardWindow::BRIEF,
    )
    .iter()
    .map(|line| line.text().trim_end().to_owned())
    .collect()
}

/// The one row the card spends on intent, with its value.
fn intent_row(lines: &[String]) -> Option<&str> {
    lines
        .iter()
        .find(|line| line.starts_with("intent"))
        .map(String::as_str)
}

/// The rows whose label is `label`, with the value beside it.
fn rows_labelled<'a>(lines: &'a [String], label: &str) -> Vec<&'a str> {
    lines
        .iter()
        .filter(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|first| first == label)
        })
        .map(String::as_str)
        .collect()
}

// ---------------------------------------------------------------------------
// The keystroke: `i` is unbound, and says where the answer went
// ---------------------------------------------------------------------------

#[test]
fn i_is_unbound_and_says_where_the_intent_went() {
    // A retired key has to answer, not shrug (ADR-006 §7.6): the absence, then the surface that holds
    // the answer now, in the same shape every other refusal in this shell takes.
    let mut app = app_with(Some(recorded()));
    let before = pane(&mut app);
    keystroke(&mut app, "i");
    assert_eq!(
        pane(&mut app),
        before,
        "`i` swaps nothing any more: the card is the one episode surface"
    );
    let said = app.toast.clone().unwrap_or_default();
    assert!(
        said.contains("`i` is unbound"),
        "the key says it is unbound: {said:?}"
    );
    assert!(
        said.contains("intent pane is retired"),
        "and why, rather than pretending it was never a key: {said:?}"
    );
    assert!(
        said.contains("fathom show"),
        "and names the verb that prints the plan and the scope: {said:?}"
    );
    assert!(
        said.contains(" — "),
        "in the house shape: the absence, then the way out: {said:?}"
    );
}

#[test]
fn the_card_is_the_same_card_whatever_is_pressed() {
    // The assertion `e9.3` owes in its own file: no keystroke swaps the card's content any more. `i` is
    // unbound; `w` grows one band in place, which is a different thing from a pane being replaced.
    let mut app = app_with(Some(recorded()));
    let before = card(&app, 78);
    keystroke(&mut app, "i");
    assert_eq!(card(&app, 78), before, "`i` changes nothing");
    keystroke(&mut app, "<Esc>");
    assert_eq!(
        card(&app, 78),
        before,
        "`<Esc>` has no mode to pop, so the card is byte-identical"
    );
}

// ---------------------------------------------------------------------------
// The row: what it prints, and what it refuses to
// ---------------------------------------------------------------------------

#[test]
fn the_row_prints_the_request_and_names_the_verb_for_the_rest() {
    // What the retired pane added over the row was the plan and the declared scope, and both are evidence
    // for a finding rather than a finding (ADR-006 §5.1.1): the row prints the sentence and names
    // `fathom show`, which prints the brief whole.
    let app = app_with(Some(recorded()));
    // Wide enough for every part the row has: the point of this test is what the row *carries*, and a
    // width where the sentence and its provenance compete is the next test's business.
    let lines = card(&app, 132);
    let row = intent_row(&lines).unwrap_or_else(|| panic!("intent is a row:\n{lines:?}"));
    assert!(
        row.contains("make the card pane scroll without dropping the selection"),
        "the request in the asker's words: {row}"
    );
    assert!(row.contains("hook-prompt"), "and which rung decided: {row}");
    assert!(
        row.contains("`fathom show"),
        "and where the plan and the declared scope live: {row}"
    );
    assert!(
        !row.contains("pin the cursor") && !row.contains("crates/fathom-tui/**"),
        "but the plan items and the declared glob are not rows: {row}"
    );
    assert!(
        !row.contains("`i`"),
        "and the row does not point a reviewer at a key that is gone: {row}"
    );
}

#[test]
fn the_card_carries_the_intent_line_and_the_session_it_arrived_on() {
    // The session and the turn are the rail's columns and the card's own field, while the brief lives in
    // the document. Both have to be right, and they are filled from the same row (`episodes::to_row`'s
    // fallback to the stream's session id).
    let app = app_with(Some(recorded()));
    let lines = card(&app, 72);
    let body = lines
        .iter()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        body.contains("make the card pane scroll"),
        "the card prints the intent without anyone pressing anything:\n{body}"
    );
    assert!(body.contains("s1"), "and the session:\n{body}");
    assert!(body.contains("s1:1"), "and the turn:\n{body}");
    assert!(
        body.contains("via hook-prompt"),
        "and who said it, because a quotation without a source is a guess:\n{body}"
    );
    assert!(
        !body.contains("for the plan and scope"),
        "and the row no longer points at a keystroke for what the pane printed:\n{body}"
    );
    let drawn = lines
        .iter()
        .filter(|line| line.starts_with("intent"))
        .count();
    assert_eq!(
        drawn,
        1,
        "intent is one row of the card, not the three it used to be:\n{}",
        lines.join("\n")
    );
}

/// The four states, on the row, in the frame the shell draws.
#[test]
fn the_card_row_draws_every_intent_state_as_itself() {
    let states: Vec<(&str, Option<serde_json::Value>, &str)> = vec![
        ("recorded", Some(recorded()), "make the card pane scroll"),
        ("digest-only", Some(digest_only()), "no text captured"),
        ("none", Some(none_recorded()), "no recorded intent"),
        ("no document", None, "not on this row"),
    ];
    let mut seen: Vec<String> = Vec::new();
    for (name, intent, expect) in states {
        let app = app_with(intent);
        for width in [35usize, 72] {
            let lines = card(&app, width);
            let drawn: Vec<&String> = lines
                .iter()
                .filter(|line| line.starts_with("intent"))
                .collect();
            assert_eq!(
                drawn.len(),
                1,
                "`{name}` at {width} columns prints exactly one intent row:\n{}",
                lines.join("\n")
            );
            let row = drawn[0].trim_end();
            assert!(
                row.contains(expect),
                "`{name}` at {width} columns is drawn as what it is, not as a blank: {row:?}"
            );
            assert!(
                !seen.contains(&row.to_owned()),
                "and no two states share a row: {seen:?} vs {row:?}"
            );
            seen.push(row.to_owned());
        }
    }
}

#[test]
fn the_row_names_what_it_did_not_find() {
    // Three absences, three different sentences, because they mean three different things to the
    // reviewer — and every one of them is now the row's alone, with no pane to say it differently.
    let digest = card(&app_with(Some(digest_only())), 96);
    let row = intent_row(&digest).unwrap_or_default().to_owned();
    assert!(
        row.contains("no text captured"),
        "a digest is not a blank: {row}"
    );
    assert!(
        row.contains("prompt-digest"),
        "and the row says where the digest came from: {row}"
    );

    let silent = card(&app_with(Some(none_recorded())), 96);
    let row = intent_row(&silent).unwrap_or_default().to_owned();
    assert!(
        row.contains("no recorded intent"),
        "an absence is printed as an absence: {row}"
    );
    assert!(
        row.contains("no harness events"),
        "with what was looked for: {row}"
    );

    let legacy = card(&app_with(None), 96);
    let row = intent_row(&legacy).unwrap_or_default().to_owned();
    assert!(
        row.contains("not on this row"),
        "and a row this build wrote no intent for is a third fact, said as one: {row}"
    );
    assert!(
        !row.contains("no recorded intent"),
        "which must not borrow the second one's sentence: {row}"
    );
}

#[test]
fn a_narrow_card_still_shows_the_state_and_wraps_the_request() {
    // The responsive contract (`e1.6`'s rule): content is wrapped or clipped, never dropped. The state
    // is the one thing that must survive a narrow terminal, because it is the answer to the question.
    let app = app_with(Some(recorded()));
    let lines: Vec<String> = timeline::card_lines(
        app.timeline.selected(),
        NOW,
        "fixture index",
        24,
        40,
        timeline::CardWindow::BRIEF,
    )
    .iter()
    .map(|line| line.text().trim_end().to_owned())
    .collect();
    assert!(
        lines.iter().all(|line| line.chars().count() <= 24),
        "nothing overflows the pane: {lines:?}"
    );
    let body = lines.join("\n");
    assert!(body.contains("intent"), "the label survives: {body}");
    let labelled = rows_labelled(&lines, "intent");
    let row = labelled
        .first()
        .unwrap_or_else(|| panic!("intent is still one row:\n{body}"));
    let value = row.trim_start_matches("intent").trim();
    assert!(
        !value.is_empty() && (value.contains('…') || value.contains("·")),
        "at 24 columns the row is cut, and says so — an empty value is the drop this rule forbids: {row}"
    );
    assert!(
        rows_labelled(&lines, "intent").len() == 1,
        "and it is still one row:\n{body}"
    );
}

#[test]
fn an_empty_selection_says_so() {
    let lines = timeline::card_lines(
        None,
        NOW,
        "fixture index",
        60,
        6,
        timeline::CardWindow::BRIEF,
    )
    .iter()
    .map(|line| line.text().trim_end().to_owned())
    .collect::<Vec<_>>();
    let body = lines.join("\n");
    assert!(body.contains("no episode selected"), "{body}");
    assert!(
        !body.contains("intent"),
        "no row under the cursor is not an intent state:\n{body}"
    );
}
