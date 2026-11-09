//! The episode card's row budget (`e9.2`, ADR-006 §5.1.1 rule 4).
//!
//! The card has nineteen content rows at 80×24 and — after the whole degradation ladder has fired —
//! seven at the timeline's floor, and it had been spending them in the wrong order. Intent, a sentence an
//! agent wrote about its own work, took three; `boundary`, `session` and `members` took five more; and the
//! rows that tell a reviewer what the change *did* fell off the end behind `… N more rows`. This file is
//! the budget, stated as what the reviewer can now see:
//!
//! * at **80×24** every band is on the screen — the code delta and its noise fold, where it landed, the
//!   four reach rows, the ranking with its counted ceiling, and one line of intent;
//! * at the **ladder floor (41×9)** the header, the code delta and the first reach row are what survive,
//!   and the provenance is what gets cut — warnings are never demoted and provenance always is;
//! * intent is **one row** in every state, and the states — recorded, digest-only, nothing recorded, and
//!   no intent on the row — are distinguishable from each other at every width that shows the row at all;
//! * the **complexity row slot** is this band's, so `e8.6` fills a defined row instead of choosing a
//!   place (`ADR-007` §5.3, §12).
//!
//! Everything is rendered through `App` at the pane's real geometry, so the layout arithmetic the shell
//! uses is under test, not a hand-picked width.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use fathom_analyze::insight::{EpisodeDelta, SectionId, brief};
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::timeline::{self, Batch, Episode};
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{NOW, mock, with_delta};

/// The frame the ADR draws the timeline at.
const DESKTOP: (u16, u16) = (80, 24);
/// The view's documented minimum, after the first rungs of the ladder.
const MINIMUM: (u16, u16) = (61, 16);
/// The timeline's ladder floor: everything the ladder can sell has been sold (`ADR-006 §3`).
const FLOOR: (u16, u16) = (41, 9);

/// The shell, on the timeline, with `rows` loaded and the cursor on the newest one.
fn app(size: (u16, u16), rows: Vec<Episode>) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.set_view(ViewId::Timeline);
    app.load_episodes(Batch::new("fixture index", rows));
    app
}

/// The card as the shell is drawing it: the pane's own width and row count, no hand-picked geometry.
fn card(app: &App) -> Vec<String> {
    let width = app.pane_cols("card");
    let rows = app.pane_body_rows();
    timeline::card_lines(
        app.timeline.selected(),
        NOW,
        app.timeline.origin(),
        width,
        rows,
    )
    .iter()
    .map(|line| line.text().trim_end().to_owned())
    .collect()
}

/// The first cell of each row — the label column, which is what this file argues about. The state glyph a
/// `brief()` row leads with (`~ what`, `x tests`) is part of the label, so it comes off here and the
/// assertions below can speak the section's name.
fn labels(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let mut words = line.split_whitespace();
            let first = words.next().unwrap_or_default();
            if matches!(first, "x" | "~" | "?") {
                words.next().unwrap_or(first).to_owned()
            } else {
                first.to_owned()
            }
        })
        .collect()
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

/// The selected episode, with a delta document whose intent is `intent` (`None` = no intent at all).
fn episode_with_intent(intent: Option<serde_json::Value>) -> Episode {
    let mut rows = with_delta(mock());
    {
        let episode = rows.first_mut().unwrap();
        if let Some(delta) = episode.delta.as_mut() {
            delta.episode.intent = intent.and_then(|value| serde_json::from_value(value).ok());
        }
    }
    rows.into_iter().next().unwrap()
}

/// The three states `insight` defines, plus the absence the pane has to draw as its own fact.
fn states() -> Vec<(&'static str, Option<serde_json::Value>)> {
    vec![
        (
            "recorded",
            Some(serde_json::json!({
                "schema": "fathom-intent/1", "state": "recorded", "source": "hook-prompt",
                "text": "make the store epoch-safe without dropping the reader's cursor",
                "confidence": 0.95, "plan": [], "scope": [], "evidence": []
            })),
        ),
        (
            "digest-only",
            Some(serde_json::json!({
                "schema": "fathom-intent/1", "state": "digest-only", "source": "prompt-digest",
                "text": "", "confidence": 0.3,
                "reason": "the harness recorded a prompt digest of 812 chars, not its text",
                "prompt": {"sha256": "aaaaaaaaaaaaaaaa", "text_captured": false},
                "plan": [], "scope": [], "evidence": []
            })),
        ),
        (
            "none",
            Some(serde_json::json!({
                "schema": "fathom-intent/1", "state": "none", "source": "none", "text": "",
                "confidence": 0.0,
                "reason": "no harness events for this repository, and no `Intent:` trailer",
                "plan": [], "scope": [], "evidence": []
            })),
        ),
        ("absent", None),
    ]
}

/// Every documented width: the ADR's drawn size, the view's minimum, and the ladder floor.
fn widths() -> Vec<(&'static str, (u16, u16))> {
    vec![
        ("80×24 (drawn at)", DESKTOP),
        ("61×16 (min)", MINIMUM),
        ("41×9 (ladder floor)", FLOOR),
    ]
}

// ---------------------------------------------------------------------------
// What the reviewer sees that they did not see before
// ---------------------------------------------------------------------------

/// The rows the card had no room for: the code delta and the reach rows, now on the first screen at every
/// width the shell will draw the pane at.
#[test]
fn the_code_delta_is_on_the_card_at_every_documented_width() {
    for (name, size) in widths() {
        let app = app(size, with_delta(mock()));
        let lines = card(&app);
        let body = lines.join("\n");
        let seen = labels(&lines);
        // Band 1: the churn and the noise fold. Before `e9.2` the churn was the pane's own `size` line
        // reformatted from the spine columns, and the fold was a figure inside `+N more`; now the
        // document's own row leads and the fold is a row.
        assert!(
            seen.iter().any(|label| label == "what"),
            "{name}: the document's `what` row is on the screen:\n{body}"
        );
        assert!(
            seen.iter().any(|label| label == "noise"),
            "{name}: the noise/reformat figure has its own row:\n{body}"
        );
        // Band 2: where it landed, with the rollup grown into the rows intent used to take.
        assert!(
            seen.iter().any(|label| label == "where"),
            "{name}: `where` is on the screen:\n{body}"
        );
        // The first reach row, on every width whose pane can spell the label.
        if size.0 >= MINIMUM.0 {
            assert!(
                seen.iter().any(|label| label == "dependencies"),
                "{name}: the first reach row is on the screen:\n{body}"
            );
        }
    }
}

/// At 80×24 the whole card fits, so the ranking and the one line of intent are on the same screen as the
/// change they are about — which is the whole point of one episode surface.
#[test]
fn at_80x24_every_band_reaches_the_screen() {
    let app = app(DESKTOP, with_delta(mock()));
    let lines = card(&app);
    let body = lines.join("\n");
    let seen = labels(&lines);
    for band in [
        "what",
        "noise",
        "where",
        "dependencies",
        "surface",
        "tests",
        "blast",
        "attention",
        "blind",
        "intent",
    ] {
        assert!(
            seen.iter().any(|label| label == band),
            "band row `{band}` is on the first screen at 80×24:\n{body}"
        );
    }
    assert!(
        !body.contains("more rows —"),
        "nothing falls off the end of the pane any more:\n{body}"
    );
}

/// At the floor the bands still outrank the provenance: the pane cuts `members`, `boundary`, `session`
/// and the stamp before it cuts a figure about the change.
#[test]
fn at_the_ladder_floor_the_provenance_is_what_gets_cut() {
    let app = app(FLOOR, with_delta(mock()));
    let lines = card(&app);
    let body = lines.join("\n");
    let seen = labels(&lines);
    for band in ["what", "noise", "where"] {
        assert!(
            seen.iter().any(|label| label == band),
            "`{band}` survives to the ladder floor:\n{body}"
        );
    }
    for provenance in ["members", "boundary", "session", "read"] {
        assert!(
            !seen.iter().any(|label| label == provenance),
            "`{provenance}` is provenance, and provenance is what gets cut first:\n{body}"
        );
    }
}

/// The band order is the delta document's reading order — `SectionId::ALL`'s, with intent appended — so
/// the pane, `fathom analyze`, the export and an agent's tool result read one reviewer's question in one
/// sequence. Read from the model rather than re-typed here, because a second list of the seven is two
/// chances to disagree about the seven.
#[test]
fn the_card_prints_the_bands_in_the_documents_order() {
    let app = app(DESKTOP, with_delta(mock()));
    let lines = card(&app);
    let seen = labels(&lines);
    let wanted: Vec<String> = SectionId::ALL
        .iter()
        .map(|id| id.as_str().to_owned())
        .chain(std::iter::once("intent".to_owned()))
        .collect();
    let order: Vec<&str> = seen
        .iter()
        .filter(|label| wanted.iter().any(|band| band == *label))
        .map(|label| label.as_str())
        .collect();
    assert_eq!(
        order,
        wanted.iter().map(String::as_str).collect::<Vec<_>>(),
        "the bands, in the document's order, each exactly once:\n{}",
        lines.join("\n")
    );
}

/// One figure, one string, one surface (ADR-006 §5.1.1). The card's duplications were four rows in three
/// vocabularies: the churn as `size` and as the document's `what` row, the ranking as `risk` and `vector`
/// and `attention`, the ceiling as two `blind` rows.
#[test]
fn no_figure_is_printed_twice() {
    // A pane wide enough not to cut anything: a duplicate that fits is a duplicate a reviewer can see.
    let rows = with_delta(mock());
    let episode = rows.first().unwrap();
    let lines: Vec<String> = timeline::card_lines(Some(episode), NOW, "fixture index", 96, 40)
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect();
    let seen = labels(&lines);
    for (label, count, note) in [
        (
            "blind",
            1,
            "the ceiling is counted once — two rows both labelled `blind` was the defect `e9.1` named",
        ),
        (
            "vector",
            0,
            "the arithmetic is the attention row's, not a second row",
        ),
        (
            "risk",
            0,
            "the ranking is the attention row's, not a second row",
        ),
        (
            "size",
            0,
            "the churn is the document's row, not the pane's reformatting of the spine columns",
        ),
    ] {
        let seen_count = seen.iter().filter(|first| *first == label).count();
        assert_eq!(
            seen_count,
            count,
            "{note}: `{label}` appears {seen_count} times:\n{}",
            lines.join("\n")
        );
    }
    let risk = episode.risk().unwrap().headline();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains(risk.as_str()))
            .count(),
        1,
        "the ranking's own bytes appear on exactly one row:\n{}",
        lines.join("\n")
    );
    // The churn, once. The card used to print it twice: as `size` off the spine columns and as the
    // document's `what` row, in two spellings of the same two numbers.
    let churn = format!("+{} \u{2212}{}", episode.n_add, episode.n_del);
    assert_eq!(
        lines.iter().filter(|line| line.contains(&churn)).count(),
        1,
        "the churn `{churn}` is stated once on the card:\n{}",
        lines.join("\n")
    );
}

/// Every figure the pane prints is the document's own bytes, placed and wrapped, never re-worded.
#[test]
fn every_figure_is_the_documents_own_string() {
    let rows = with_delta(mock());
    let episode = rows.first().unwrap();
    let delta = episode.delta.as_ref().unwrap();
    // A pane big enough to wrap is a pane that does not have to cut: here the document's row and the
    // card's row are the same bytes, which is what "the pane authors nothing" means in a test.
    let lines: Vec<String> = timeline::card_lines(Some(episode), NOW, "fixture index", 96, 40)
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect();
    let body = lines.join("\n");
    for row in brief(delta) {
        assert!(
            body.contains(&row.value),
            "the `{} {}` row's value is the document's own bytes, verbatim:\n{body}",
            row.glyph,
            row.id
        );
    }
    // And the noise figure — the one the card pulls out of the section rather than relaying the first
    // figure of — is the figure the analysis wrote, with its own listing.
    let noise = delta
        .sections
        .iter()
        .find(|section| section.id == SectionId::What.as_str())
        .unwrap()
        .figures
        .iter()
        .find(|figure| figure.label == "noise folded")
        .unwrap();
    assert!(
        body.contains(&noise.value),
        "the noise row is the model's figure, not a paraphrase of it:\n{body}"
    );
}

// ---------------------------------------------------------------------------
// Intent: one row, three states, at every width
// ---------------------------------------------------------------------------

/// Intent is one row wherever it is drawn, and the states are distinguishable from each other.
#[test]
fn intent_is_one_row_in_every_state_at_every_width() {
    for (name, size) in widths() {
        let mut heads: Vec<String> = Vec::new();
        for (state, intent) in states() {
            let app = app(size, vec![episode_with_intent(intent)]);
            let lines = card(&app);
            let drawn = rows_labelled(&lines, "intent");
            assert!(
                drawn.len() <= 1,
                "`{state}` at {name} prints {drawn:?} — intent is at most one row:\n{}",
                lines.join("\n")
            );
            if let Some(row) = drawn.first() {
                heads.push((*state).to_owned());
                assert!(
                    row.chars().count() <= app.pane_cols("card"),
                    "the intent row never runs off the pane: {row:?}"
                );
            }
        }
        // Every state is drawn when it has a row at all, so the count is the number that fit on screen.
        assert!(
            heads.windows(2).all(|pair| pair[0] != pair[1]),
            "the states are separate facts, stated separately: {heads:?}"
        );
    }
}

/// The four states say four different things, and none of them is silence.
#[test]
fn the_four_intent_states_read_differently() {
    let mut seen: Vec<(String, String)> = Vec::new();
    for (state, intent) in states() {
        let episode = episode_with_intent(intent);
        let lines: Vec<String> = timeline::card_lines(Some(&episode), NOW, "fixture index", 78, 30)
            .iter()
            .map(|line| line.text().trim_end().to_owned())
            .collect();
        let row = rows_labelled(&lines, "intent")
            .first()
            .unwrap_or_else(|| panic!("`{state}` prints a row:\n{}", lines.join("\n")))
            .to_string();
        seen.push((state.to_owned(), row.clone()));
        match state {
            "recorded" => {
                assert!(row.contains("epoch-safe"), "the text itself: {row}");
                assert!(row.contains("hook-prompt"), "and who said it: {row}");
            }
            "digest-only" => assert!(row.contains("no text captured"), "{row}"),
            "none" => assert!(row.contains("no recorded intent"), "{row}"),
            _ => assert!(row.contains("not on this row"), "{row}"),
        }
    }
    let rows: Vec<String> = seen.iter().map(|(_, row)| row.clone()).collect();
    for (index, row) in rows.iter().enumerate() {
        for other in rows.iter().skip(index + 1) {
            assert_ne!(row, other, "two states must not print one row: {rows:?}");
        }
    }
}

/// A recorded intent is cut with a `…` the reviewer can see, not a sentence that silently stops; and the
/// cut falls on the sentence, not on the provenance behind it.
#[test]
fn a_long_intent_is_cut_visibly_and_keeps_its_source() {
    let episode = episode_with_intent(Some(serde_json::json!({
        "schema": "fathom-intent/1", "state": "recorded", "source": "hook-prompt",
        "text": "make the store epoch-safe without dropping the reader's cursor while the index rewrites \
                 the rail underneath it, and keep the counters the worklist cites",
        "confidence": 0.95, "plan": [], "scope": [], "evidence": []
    })));
    let lines: Vec<String> = timeline::card_lines(Some(&episode), NOW, "fixture index", 60, 30)
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect();
    let row = rows_labelled(&lines, "intent").first().unwrap().to_owned();
    assert_eq!(
        row.chars().count(),
        60,
        "one row, exactly the pane's width: {row:?}"
    );
    assert!(row.contains('…'), "the cut is visible: {row}");
    assert!(
        row.contains("via hook-prompt"),
        "and it falls on the sentence, so the source survives: {row}"
    );
    assert!(
        !row.contains("underneath it"),
        "the tail of a long sentence is what got cut: {row}"
    );
}

/// The plan and the declared scope are evidence, and evidence has one home: the delta document, printed by
/// `fathom show`. The row names the verb instead of growing to hold them.
#[test]
fn intent_behind_which_there_is_a_plan_names_the_verb_that_prints_it() {
    let episode = episode_with_intent(Some(serde_json::json!({
        "schema": "fathom-intent/1", "state": "recorded", "source": "hook-prompt",
        "text": "pin the cursor", "confidence": 0.95,
        "plan": [{"text": "keep the selection", "status": "in_progress"}],
        "scope": ["crates/fathom-store/**"], "evidence": []
    })));
    let lines: Vec<String> = timeline::card_lines(Some(&episode), NOW, "fixture index", 96, 30)
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect();
    let row = rows_labelled(&lines, "intent").first().unwrap().to_owned();
    assert!(
        row.contains("`fathom show"),
        "the line says where the whole brief lives: {row}"
    );
    assert!(
        !row.contains("crates/fathom-store/**"),
        "but the declared scope is not a row: {row}"
    );
}

// ---------------------------------------------------------------------------
// The complexity row slot, reserved before the figure exists
// ---------------------------------------------------------------------------

/// `e8.6` fills a row that already has a place: the code-delta band, after the noise fold and before
/// `where`, under the word ADR-007 §10.4 fixed. Until the figure exists the row is absent — never a zero.
#[test]
fn the_complexity_row_lands_in_the_code_delta_band() {
    let plain = app(DESKTOP, with_delta(mock()));
    let lines = card(&plain);
    assert!(
        rows_labelled(&lines, "complexity").is_empty(),
        "no figure, no row — an unmeasured figure is never printed as a zero:\n{}",
        lines.join("\n")
    );

    let mut rows = with_delta(mock());
    plant_complexity(&mut rows);
    let planted = app(DESKTOP, rows);
    let lines = card(&planted);
    let seen = labels(&lines);
    let at = seen
        .iter()
        .position(|label| label == "complexity")
        .unwrap_or_else(|| panic!("the planted figure has its row:\n{}", lines.join("\n")));
    let noise = seen.iter().position(|label| label == "noise").unwrap();
    let where_at = seen.iter().position(|label| label == "where").unwrap();
    assert!(
        at > noise && at < where_at,
        "the complexity row sits between the noise fold and `where`:\n{}",
        lines.join("\n")
    );
    let row = rows_labelled(&lines, "complexity")[0];
    assert!(
        row.contains("+7 (4/5 files measured)"),
        "the figure's own bytes, coverage and all (ADR-007 §5.4):\n{row}"
    );
}

/// A complexity figure that could not be measured prints as `?` with its reason, not as `±0`.
#[test]
fn an_unmeasured_complexity_figure_prints_as_unknown() {
    let mut rows = with_delta(mock());
    {
        let episode = rows.first_mut().unwrap();
        let delta = episode.delta.as_mut().unwrap();
        let mut document = serde_json::to_value(&*delta).unwrap();
        figures_of(&mut document, SectionId::What).push(serde_json::json!({
            "label": "complexity",
            "value": "",
            "known": false,
            "reason": "every path in this episode was unreadable at the base",
            "evidence": {"owner": "e8.4"}
        }));
        *delta = serde_json::from_value(document).unwrap();
    }
    let episode = rows.first().unwrap();
    let lines: Vec<String> = timeline::card_lines(Some(episode), NOW, "fixture index", 96, 40)
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect();
    let row = rows_labelled(&lines, "complexity")
        .first()
        .unwrap_or_else(|| panic!("the unknown is drawn, not dropped:\n{}", lines.join("\n")))
        .to_owned();
    assert!(
        row.contains("unreadable"),
        "the reason travels with the gap: {row}"
    );
    assert!(!row.contains("±0"), "and zero is never the answer: {row}");
}

/// Add a `complexity` figure to the `what` section, the way `e8.6` will: through the document, with its
/// evidence pointer attached, at the unchanged document version.
fn plant_complexity(rows: &mut [Episode]) {
    let episode = rows.first_mut().unwrap();
    let delta = episode.delta.as_mut().unwrap();
    let mut document = serde_json::to_value(&*delta).unwrap();
    figures_of(&mut document, SectionId::What).push(serde_json::json!({
        "label": "complexity",
        "value": "+7 (4/5 files measured)",
        "known": true,
        "raw": {"delta": 7, "coverage_measured": 4, "coverage_total": 5},
        "evidence": {"pointer": "#/files"}
    }));
    *delta = serde_json::from_value(document).unwrap();
}

/// The figure list of one section, as JSON — the shape the store writes and the CLI hands over.
fn figures_of(document: &mut serde_json::Value, section: SectionId) -> &mut Vec<serde_json::Value> {
    let sections = document
        .get_mut("sections")
        .and_then(serde_json::Value::as_array_mut)
        .unwrap();
    let wanted = section.as_str();
    let section = sections
        .iter_mut()
        .find(|row| row.get("id").and_then(serde_json::Value::as_str) == Some(wanted))
        .unwrap();
    section
        .get_mut("figures")
        .and_then(serde_json::Value::as_array_mut)
        .unwrap()
}

/// The keystroke the card does **not** need: `i` still raises the intent pane (until `e9.3` retires it),
/// and the card's own line is unaffected by which states it has to carry.
#[test]
fn the_card_carries_its_intent_line_without_any_keystroke() {
    let mut app = app(
        DESKTOP,
        vec![episode_with_intent(Some(serde_json::json!({
            "schema": "fathom-intent/1", "state": "recorded", "source": "hook-prompt",
            "text": "pin the cursor", "confidence": 0.95, "plan": [], "scope": [], "evidence": []
        })))],
    );
    assert!(rows_labelled(&card(&app), "intent").len() == 1);
    app.feed_key("i", Instant::now());
    assert!(
        app.showing_intent(app.timeline.selected()),
        "`i` is still the intent pane until `e9.3` retires it"
    );
}

/// The card's honesty clause: a document that failed its own self-check says so above the rows, not in a
/// footer nobody scrolls to.
#[test]
fn a_failed_self_check_is_a_warning_above_the_rows() {
    let mut rows = with_delta(mock());
    {
        let episode = rows.first_mut().unwrap();
        let delta: &mut EpisodeDelta = episode.delta.as_mut().unwrap();
        delta.integrity.verified = false;
    }
    let lines = card(&app(DESKTOP, rows));
    let body = lines.join("\n");
    assert!(
        body.contains("self-check FAILED"),
        "the warning is on the screen:\n{body}"
    );
    let at = lines
        .iter()
        .position(|line| line.contains("self-check FAILED"))
        .unwrap();
    let what = lines
        .iter()
        .position(|line| line.starts_with("~ what"))
        .unwrap();
    assert!(
        at < what,
        "and it is read before any of the rows it disowns:\n{body}"
    );
}
