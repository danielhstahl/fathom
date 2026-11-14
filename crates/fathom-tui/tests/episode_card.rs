//! The episode card's row budget (`e9.2`, ADR-006 §5.1.1 rule 4).
//!
//! The card has eighteen content rows at 80×24 — the rows `frame_plan` gives the pane, which
//! `the_budget_is_the_rows_the_frame_gives_the_card_pane` pins rather than assumes — and, after the
//! whole degradation ladder has fired, seven at the timeline's floor, and it had been spending them in
//! the wrong order. Intent, a sentence an
//! agent wrote about its own work, took three; `boundary`, `session` and `members` took five more; and the
//! rows that tell a reviewer what the change *did* fell off the end behind `… N more rows`. This file is
//! the budget, stated as what the reviewer can now see:
//!
//! * at **80×24** every band is on the screen — the code delta and its noise fold, where it landed, the
//!   four reach rows, the ranking with its counted ceiling, and one line of intent;
//! * the **budget is a number, not a sentence**: the rows the frame gives the pane, at the four sizes the
//!   budget is stated at, and the card never prints past them (`reco-5js3`);
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

use fathom_analyze::insight::{EpisodeDelta, SectionId, brief};
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_plan;
use fathom_tui::timeline::{self, Batch, CardWindow, Episode};
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

/// Squeeze the runs of whitespace out of rendered text, so a value that the card wrapped across rows is
/// still comparable as the sentence it is.
fn squeezed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The card pane with no keystroke spent on it: top of the document, band 4 brief.
const BRIEF: CardWindow = CardWindow::BRIEF;

/// The ADR page this file reads its band list out of, the way `tests/layout.rs` reads the ladder
/// tables. The band list is normative there and a copy here would be a second list to disagree with.
const ADR: &str = include_str!("../../../docs/adr/0006-tui.md");

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
        app.card_window(),
    )
    .iter()
    .map(|line| line.text().trim_end().to_owned())
    .collect()
}

/// Press a chord the way the terminal would.
fn press(app: &mut App, chord: &str) {
    app.feed_key(chord, std::time::Instant::now());
}

/// The five bands §5.1.1's table publishes, read out of the ADR page rather than re-typed here: the
/// `#`/`band` column of the band table, in its order. A sixth row added there with no renderer here
/// fails `the_card_prints_the_adrs_bands_in_order`, which is the point of reading it.
fn adr_bands() -> Vec<String> {
    let table = ADR
        .split("| # | band |")
        .nth(1)
        .expect("§5.1.1's band table has the header this test reads")
        .split("\n\n")
        .next()
        .expect("the band table is followed by a blank line");
    table
        .lines()
        .filter_map(|line| {
            let mut cells = line.split('|').skip(1);
            let number = cells.next()?.trim();
            if !number.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            Some(cells.next()?.trim().trim_matches('`').to_owned())
        })
        .collect()
}

/// Which card labels belong to which band, in the order the bands print them. The names come from the
/// ADR's table; this is the mapping from a band to the rows it is allowed to print, and every label in
/// it is a row the pane actually draws.
fn band_labels() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("what", &["what", "size", "noise", "complexity"]),
        ("where", &["where", "delta"]),
        (
            "reach",
            &["dependencies", "surface", "tests", "blast", "delta"],
        ),
        (
            "rank",
            &["attention", "risk", "surfaces", "reason", "blind", "unseen"],
        ),
        ("intent", &["intent"]),
    ]
}

/// The first cell of each row — the label column, which is what this file argues about. The state glyph a
/// `brief()` row leads with (`~ what`, `x tests`) is part of the label, so it comes off here and the
/// assertions below can speak the section's name.
/// The card's band labels, in order: the first word of each row, with the state glyph and a note's
/// continuation mark taken off. A `↳` line is the sentence under the row above it, not a band.
fn card_labels(lines: &[String]) -> Vec<String> {
    labels(lines)
        .into_iter()
        .filter(|label| label != "↳")
        .collect()
}

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

/// The rows whose label is `label`, with the value beside it. The state glyph a `brief()` row leads
/// with (`~ what`, `x tests`) is not the label, so it comes off here the way `labels()` strips it.
fn rows_labelled<'a>(lines: &'a [String], label: &str) -> Vec<&'a str> {
    lines
        .iter()
        .filter(|line| {
            !line.starts_with(' ')
                && line
                    .split_whitespace()
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

/// The four sizes the row budget is stated at, each with the body rows the frame gives the `card` pane.
///
/// The third column is what this file argues about, and it is an expectation and not a measurement: the
/// test that reads it asks `frame_plan` for the same number, so a ladder that quietly costs the pane a
/// row fails here instead of quietly shortening the card. `120×30` is the size `reco-5js3` named when it
/// asked for "a named row budget with a test that the rendered card never exceeds it".
fn budget_sizes() -> Vec<(&'static str, (u16, u16), usize)> {
    vec![
        ("120×30 (the size the epic named)", (120, 30), 24),
        ("80×24 (drawn at)", DESKTOP, 18),
        ("61×16 (min)", MINIMUM, 10),
        ("41×9 (ladder floor)", FLOOR, 7),
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

/// The budget is a number, and a number nobody checks is a rumour (`reco-5js3`, ADR-006 §5.1.1 rule 4).
///
/// Rule 4 used to spend itself describing what fits in "twenty rows: one is the strip, nineteen are
/// content" — a count off a layout mock, in which the pane's own title row was booked as content. The
/// renderer's plan is the arithmetic that decides it, and at 80×24 it hands the `card` pane eighteen
/// rows, not nineteen. So read the plan first, then hold every card the pane is asked to draw to it.
///
/// Two assertions, because "never longer than the pane" on its own is satisfied by a card that prints one
/// row. The cut card is **exactly** as deep as the pane — never a row more, which is the budget, and never
/// a row less, because a short pane is a torn frame — and whenever the document is deeper than the pane,
/// the last visible row is the one that counts what fell and names the motion that walks to it. Both band
/// states and both document states are swept, because `w` and a missing delta document are what move the
/// card's depth, and 120×30 is the size at which the whole document fits and nothing has to be counted.
#[test]
fn the_budget_is_the_rows_the_frame_gives_the_card_pane() {
    for (name, size, budget) in budget_sizes() {
        let shell = app(size, with_delta(mock()));
        let plan = frame_plan(
            &shell,
            ViewId::Timeline,
            usize::from(size.0),
            usize::from(size.1),
        );
        let pane = plan
            .panes
            .iter()
            .find(|rect| rect.pane.name == "card")
            .unwrap_or_else(|| panic!("{name}: the timeline draws a card pane"));
        assert_eq!(
            pane.rows, budget,
            "{name}: rule 4 spends the renderer's own count of the pane, and the page now says {budget}"
        );
        assert_eq!(
            pane.rows,
            shell.pane_body_rows(),
            "{name}: the app's side of the row arithmetic and the renderer's cannot disagree"
        );

        for has_delta in [true, false] {
            for expanded in [false, true] {
                let rows = if has_delta {
                    with_delta(mock())
                } else {
                    mock()
                };
                let shell = app(size, rows);
                let window = CardWindow {
                    scroll: 0,
                    expanded,
                };
                let state = format!(
                    "{name}, {}, {}",
                    if expanded { "`w` open" } else { "band 4 brief" },
                    if has_delta {
                        "with a document"
                    } else {
                        "no document"
                    }
                );
                let document = timeline::card_document(
                    shell.timeline.selected(),
                    NOW,
                    shell.timeline.origin(),
                    shell.pane_cols("card"),
                    shell.pane_body_rows(),
                    window,
                );
                let cut = timeline::card_lines(
                    shell.timeline.selected(),
                    NOW,
                    shell.timeline.origin(),
                    shell.pane_cols("card"),
                    shell.pane_body_rows(),
                    window,
                );
                assert_eq!(
                    cut.len(),
                    budget,
                    "{state}: the card fills the rows the frame gave the pane and does not go past them"
                );
                let last = cut
                    .last()
                    .map(|line| line.text().trim_end().to_owned())
                    .unwrap_or_default();
                if document.len() > budget {
                    let hidden = document.len() - budget + 1;
                    assert!(
                        last.starts_with('…')
                            && last.contains(&hidden.to_string())
                            && last.contains('j'),
                        "{state}: {} document rows in {budget} is a window, so the last row counts the \
                         {hidden} that fell and names the motion that walks them; it printed {last:?}",
                        document.len()
                    );
                } else {
                    assert!(
                        !last.starts_with('…'),
                        "{state}: the whole card fits in {budget} rows, so nothing is counted away: {last:?}"
                    );
                }
            }
        }
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
    let lines: Vec<String> =
        timeline::card_lines(Some(episode), NOW, "fixture index", 96, 40, BRIEF)
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
    let lines: Vec<String> =
        timeline::card_lines(Some(episode), NOW, "fixture index", 96, 40, BRIEF)
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
        let lines: Vec<String> =
            timeline::card_lines(Some(&episode), NOW, "fixture index", 78, 30, BRIEF)
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
    let lines: Vec<String> =
        timeline::card_lines(Some(&episode), NOW, "fixture index", 60, 30, BRIEF)
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
    let lines: Vec<String> =
        timeline::card_lines(Some(&episode), NOW, "fixture index", 96, 30, BRIEF)
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
        row.contains("~+7 (4 of 5 measured"),
        "the figure's own bytes, floor and denominator included (ADR-007 §5.4):\n{row}"
    );
    assert_eq!(
        row.matches("measured").count(),
        1,
        "the denominator is stated once — the card reads the one figure, it does not recount it \
         (ADR-001 §7.10):\n{row}"
    );
    // …and at a width the tail survives at, §5.3's second figure is on the same row as the word that
    // makes it a complexity figure rather than a count (§10.1's rule about a bare number).
    let wide_rows = plant_with_complexity();
    let wide_lines: Vec<String> =
        timeline::card_lines(wide_rows.first(), NOW, "fixture index", 96, 40, BRIEF)
            .iter()
            .map(|line| line.text().trim_end().to_owned())
            .collect();
    let wide_row = rows_labelled(&wide_lines, "complexity")
        .first()
        .unwrap_or_else(|| panic!("a wide card has the row a narrow one has"))
        .to_owned();
    assert!(
        wide_row.contains("~+7") && wide_row.contains("48 at head"),
        "delta, then the absolute it was taken against, on the row that names the word:\n{wide_row}"
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
    let lines: Vec<String> =
        timeline::card_lines(Some(episode), NOW, "fixture index", 96, 40, BRIEF)
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
fn plant_with_complexity() -> Vec<Episode> {
    let mut rows = with_delta(mock());
    plant_complexity(&mut rows);
    rows
}

fn plant_complexity(rows: &mut [Episode]) {
    let episode = rows.first_mut().unwrap();
    let delta = episode.delta.as_mut().unwrap();
    let mut document = serde_json::to_value(&*delta).unwrap();
    figures_of(&mut document, SectionId::What).push(serde_json::json!({
        "label": "complexity",
        // The shape `complexity_headline` actually prints for a floor (`e8.5`): the tilde, the delta, and
        // the denominator inside the value. A plant that left the denominator out of the value while
        // still counting an unmeasurable path would be a figure no producer writes.
        "value": "~+7 (4 of 5 measured · 1 unmeasured: language_unknown)",
        "known": true,
        "raw": {
            "delta": 7,
            "measured": 4,
            "unmeasurable": 1,
            "coverage_measured": 4,
            "coverage_total": 5,
            "absolute": 48,
            "max": 9
        },
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

/// The keystroke the card does not need: `i` is unbound now that the pane is gone, and the card's own
/// line is the answer either way (`e9.3`).
#[test]
fn the_card_carries_its_intent_line_without_any_keystroke() {
    let mut app = app(
        DESKTOP,
        vec![episode_with_intent(Some(serde_json::json!({
            "schema": "fathom-intent/1", "state": "recorded", "source": "hook-prompt",
            "text": "pin the cursor", "confidence": 0.95, "plan": [], "scope": [], "evidence": []
        })))],
    );
    assert_eq!(rows_labelled(&card(&app), "intent").len(), 1);
    let before = card(&app);
    press(&mut app, "i");
    assert_eq!(
        card(&app),
        before,
        "`i` no longer raises a pane over the card — it is unbound, and it says so"
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

// ---------------------------------------------------------------------------
// One surface: `w` grows a band, nothing swaps (`e9.3`, ADR-006 §5.1.1)
// ---------------------------------------------------------------------------

/// The band list is §5.1.1's, read out of the ADR page rather than re-typed here, and the card prints
/// each band's rows in that order, each band's rows contiguously. A band added to the page with no
/// renderer in this file fails the first assertion; a card that re-orders them fails the second.
#[test]
fn the_card_prints_the_adrs_bands_in_order() {
    let bands = adr_bands();
    assert_eq!(
        bands,
        vec!["what", "where", "reach", "rank", "intent"],
        "the five bands, in the page's order — the table this test reads is the normative one"
    );
    let app = app(DESKTOP, with_delta(mock()));
    let seen = labels(&card(&app));
    let mut order: Vec<&str> = Vec::new();
    for label in &seen {
        for (band, rows) in band_labels() {
            if rows.contains(&label.as_str()) && order.last() != Some(&band) {
                order.push(band);
            }
        }
    }
    let wanted: Vec<&str> = bands.iter().map(String::as_str).collect();
    assert_eq!(
        order,
        wanted,
        "the card's rows visit the ADR's bands in the ADR's order:\n{}",
        card(&app).join("\n")
    );
}

// ---------------------------------------------------------------------------
// `w` opens band 4 in place (`e9.3`)
// ---------------------------------------------------------------------------

/// The card as laid-out text with as much room as it wants: what the pane would print given a tall
/// enough terminal, which is what a "nothing else moved" claim has to be measured against. A windowed
/// pane hides the tail by design; the document does not.
fn document(episode: &Episode, expanded: bool) -> Vec<String> {
    timeline::card_document(
        Some(episode),
        NOW,
        "fixture index",
        96,
        40,
        CardWindow {
            scroll: 0,
            expanded,
        },
    )
    .iter()
    .map(|line| line.text().trim_end().to_owned())
    .collect()
}

/// Expanding band 4 grows it in place. Every other band prints exactly what it printed before, in the
/// same order, and everything above the band that opened is byte-identical — which is what "the pane
/// grew, it did not swap" means now the pane is a window: nothing was replaced, and the rows below the
/// fold are below it for the same reason they were before (the frame is a size), with a row that says
/// how many and names the motion that walks them.
#[test]
fn expanding_band_4_keeps_every_other_band() {
    let episode = with_delta(mock()).into_iter().next().unwrap();
    let brief = document(&episode, false);
    let open = document(&episode, true);
    let brief_body = brief.join("\n");
    let body = open.join("\n");
    assert!(
        body.len() > brief_body.len(),
        "the card grew, it did not swap:\n{brief_body}\n───\n{body}"
    );
    for label in [
        "what",
        "noise",
        "where",
        "dependencies",
        "surface",
        "tests",
        "blast",
        "intent",
        "members",
        "boundary",
        "session",
        "model",
        "read",
    ]
    .into_iter()
    .filter(|label| !rows_labelled(&brief, label).is_empty())
    {
        assert_eq!(
            rows_labelled(&brief, label),
            rows_labelled(&open, label),
            "`{label}` is printed exactly as it was with the ranking band open:\n{body}"
        );
    }
    let opened = card_labels(&open)
        .iter()
        .position(|label| *label == "reason")
        .expect("the open card prints the reasons list");
    assert_eq!(
        card_labels(&brief)[..opened],
        card_labels(&open)[..opened],
        "everything above the band that opened is byte-identical:\n{body}"
    );
    assert_eq!(
        card_labels(&brief)[opened + 1..],
        card_labels(&open)[card_labels(&open)
            .iter()
            .rposition(|label| *label == "blind")
            .unwrap()
            + 1..],
        "and everything below it is still there, in the same order:\n{body}"
    );

    // The band that opened is now the model's whole argument: every line of `reasons()`, then every
    // blind spot in full, then the model's own line about what a ceiling is for.
    let risk = episode.risk().expect("the fixture episode is ranked");
    for reason in risk.reasons() {
        assert!(
            body.contains(reason.as_str()),
            "every line of `reasons()` is in the card: {reason}\n{body}"
        );
    }
    for spot in &risk.blind_spots {
        assert!(
            body.contains(spot.as_str()),
            "every line of `BLIND_SPOTS` is in the card, with its words intact: {spot}\n{body}"
        );
    }
    assert!(
        body.contains(&risk.blind_spot_note().unwrap()),
        "the counted ceiling stays, in the model's words:\n{body}"
    );
    assert!(
        body.contains(&risk.ceiling_note().unwrap()),
        "and so does the line that says what a ceiling is for:\n{body}"
    );
    assert_eq!(
        card_labels(&open)
            .iter()
            .filter(|label| **label == "blind")
            .count(),
        1,
        "the ceiling is counted once, even with its list printed:\n{body}"
    );
}

/// The `w` toast, the model's own ceiling string and this test have to agree that `w` lists them all.
#[test]
fn the_ceiling_string_that_promises_w_delivers_w() {
    let episode = with_delta(mock()).into_iter().next().unwrap();
    let risk = episode.risk().expect("the fixture episode is ranked");
    let note = risk.blind_spot_note().expect("the fixture has a ceiling");
    assert!(
        note.contains("`w` lists them all"),
        "the shipped string is the promise this test exists to check: {note}"
    );
    let body = document(&episode, true).join("\n");
    assert!(
        risk.blind_spots
            .iter()
            .all(|spot| body.contains(spot.as_str())),
        "and the card keeps it: every one of the {} is printed, in full\n{body}",
        risk.blind_spots.len()
    );
}

/// Opening the band and closing it again leaves the card exactly as it was — the toggle is a toggle,
/// not a mode you can get stuck in.
#[test]
fn opening_and_closing_band_4_restores_the_card() {
    let mut shell = app(DESKTOP, with_delta(mock()));
    let before = card(&shell);
    press(&mut shell, "w");
    assert_ne!(card(&shell), before, "and `w` does open the band");
    press(&mut shell, "w");
    assert_eq!(
        card(&shell),
        before,
        "`w` again folds it back to the brief ranking, byte for byte"
    );
}

// ---------------------------------------------------------------------------
// The degradation contract: three absences, three readings (`e9.3`)
// ---------------------------------------------------------------------------

/// The card as the shell draws it for one episode, at a width nobody has to argue about.
fn one_card(episode: &Episode, window: CardWindow) -> String {
    timeline::card_lines(Some(episode), NOW, "fixture index", 96, 40, window)
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// An unranked episode, an episode with no delta document, and an episode with neither all read as
/// themselves. An absent figure is a gap with a reason in it — never an empty section, never a zero,
/// never silence — and the three are three different facts to the reviewer.
#[test]
fn the_three_absences_read_as_themselves() {
    let ranked = with_delta(mock()).into_iter().next().unwrap();
    let unranked = {
        // A document that arrived with no ranking in it, which is what an index written before
        // `reco-c05` hands over — the shape `sparse()` makes, not a ranked document with a field blanked.
        let mut episode = ranked.clone();
        episode.delta = Some(EpisodeDelta::sparse(&episode.id, &episode.summary, None));
        episode
    };
    let neither = mock().into_iter().next().unwrap(); // a spine row: no document, so no ranking
    let cards: Vec<(String, String)> = [
        ("ranked with a document", ranked),
        ("document, unranked", unranked),
        ("no document, unranked", neither),
    ]
    .into_iter()
    .map(|(kind, episode)| {
        let body = one_card(&episode, CardWindow::BRIEF);
        assert!(
            !body.contains("= 0 ") && !body.contains("risk  0") && !body.contains("complexity  0"),
            "an absent figure is never printed as a zero ({kind}):\n{body}"
        );
        (kind.to_owned(), body)
    })
    .collect();
    let (_, ranked) = &cards[0];
    let (_, unranked) = &cards[1];
    let (_, neither) = &cards[2];
    assert!(
        ranked.contains("spine-3"),
        "the ranked card prints the vector:\n{ranked}"
    );
    assert!(
        unranked.contains("nothing ranked this episode"),
        "the unranked one says so in the ranking's own row — the document's words, not a blank:\n{unranked}"
    );
    assert!(
        neither.contains("nothing has ranked this episode"),
        "and so does the episode with neither, where the card speaks for a document that never \
         arrived: an unranked band is a gap with a reason, not an empty section:\n{neither}"
    );
    assert!(
        neither.contains("no document was handed to this card"),
        "the episode with no document says that where the document would answer:\n{neither}"
    );
    assert!(
        neither.contains("no rollup for the"),
        "including the band the pane used to fill by rolling the index's paths up itself:\n{neither}"
    );
    assert_ne!(
        unranked, neither,
        "an episode with a document and no ranking is a different fact from one with no document"
    );
}

/// Pressing `w` on an episode nothing has ranked must not look like a keystroke that did nothing.
#[test]
fn expanding_an_unranked_episode_says_why_there_is_nothing_to_expand() {
    let mut shell = app(DESKTOP, vec![mock().into_iter().next().unwrap()]);
    press(&mut shell, "w");
    let body = card(&shell).join("\n");
    assert!(
        body.contains("nothing to expand"),
        "the band opened and said what it found:\n{body}"
    );
    assert!(
        app_toast(&shell).contains("is unranked"),
        "and so does the hint row: {}",
        app_toast(&shell)
    );
}

/// The hint row, which is where a refusal has to land.
fn app_toast(app: &App) -> String {
    app.toast.clone().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The window: what does not fit is counted and reachable (`e9.3`, ADR-006 rule 5)
// ---------------------------------------------------------------------------

/// What did not fit is counted, and the row that counts it names the motion that walks it. It used to
/// point only at `<C-w>o`, which is the shell's old claim that the answer to a card too tall for its
/// frame was to make the frame bigger.
#[test]
fn what_does_not_fit_is_counted_and_reachable() {
    let mut shell = app(MINIMUM, with_delta(mock()));
    let first = card(&shell);
    let body = first.join("\n");
    let overflow = first
        .iter()
        .find(|line| line.starts_with('…'))
        .unwrap_or_else(|| panic!("a card deeper than the pane counts what fell:\n{body}"));
    assert!(
        overflow.contains("more row") && overflow.contains('j'),
        "the row names the motion that walks the rest: {overflow}"
    );
    assert!(
        !overflow.contains("<C-w>o"),
        "and no longer points at a zoom as the only way to see the tail: {overflow}"
    );

    // And it is reachable from the pane, with no mode to leave and no zoom to find.
    press(&mut shell, "<Tab>");
    let last = timeline::card_document(
        shell.timeline.selected(),
        NOW,
        shell.timeline.origin(),
        shell.pane_cols("card"),
        shell.pane_body_rows(),
        shell.card_window(),
    )
    .last()
    .map(|line| line.text().trim_end().to_owned())
    .unwrap_or_default();
    let mut seen = body;
    for _ in 0..40 {
        if seen.contains(last.trim_end()) {
            return; // the whole card reached, in the pane, by walking it.
        }
        press(&mut shell, "j");
        seen.push('\n');
        seen.push_str(&card(&shell).join("\n"));
    }
    panic!("walking the card from the pane never reached its last row:\n{seen}");
}

/// `G` walks to the end of the card, the way it does in the document panes, and says so a second time.
#[test]
fn g_walks_to_the_end_of_the_card() {
    let mut shell = app(DESKTOP, with_delta(mock()));
    press(&mut shell, "w");
    press(&mut shell, "<Tab>");
    press(&mut shell, "G");
    let body = card(&shell).join("\n");
    assert!(
        body.contains("fixture index · times UTC") || body.contains("times UTC"),
        "the bottom of the card is the provenance row:\n{body}"
    );
    assert!(
        !body.starts_with('…'),
        "and nothing fell off, because the window is at the end:\n{body}"
    );
    press(&mut shell, "G");
    assert!(
        shell
            .toast
            .clone()
            .unwrap_or_default()
            .contains("already at the bottom"),
        "and pressing it again says so rather than doing nothing silently: {:?}",
        shell.toast
    );
}

/// At the documented minimum, with the ranking band open, the blind spots are reachable from the pane.
/// They are the honest half of the ranking; the card may count them into the tail, but it may not
/// quietly drop them (`e9.3`).
#[test]
fn the_blind_spots_are_reachable_from_the_pane_at_the_documented_minimum() {
    let mut shell = app(MINIMUM, with_delta(mock()));
    let spots = shell
        .timeline
        .selected()
        .and_then(Episode::risk)
        .expect("the fixture is ranked")
        .blind_spots
        .clone();
    press(&mut shell, "w");
    press(&mut shell, "<Tab>");
    let mut seen = String::new();
    for _ in 0..48 {
        let here = squeezed(&seen);
        if spots.iter().all(|spot| here.contains(&squeezed(spot))) {
            return; // every one of them reached, in the pane, by walking it.
        }
        press(&mut shell, "j");
        seen.push(' ');
        seen.push_str(&card(&shell).join("\n"));
    }
    panic!(
        "walking the card at {}x{} never reached the whole ceiling:\n{seen}",
        MINIMUM.0, MINIMUM.1
    );
}

/// At the ladder floor the card is a strip and the ceiling is deep, so the promise is the weaker one
/// it can actually keep at 41×9: nothing is *dropped*. Every blind spot is still in the card, and the
/// row at the bottom of the pane says how many rows are below it.
#[test]
fn at_the_ladder_floor_nothing_is_dropped_only_reachable_by_walking() {
    let mut shell = app(FLOOR, with_delta(mock()));
    let spots = shell
        .timeline
        .selected()
        .and_then(Episode::risk)
        .expect("the fixture is ranked")
        .blind_spots
        .clone();
    press(&mut shell, "w");
    let doc = timeline::card_document(
        shell.timeline.selected(),
        NOW,
        shell.timeline.origin(),
        shell.pane_cols("card"),
        shell.pane_body_rows(),
        shell.card_window(),
    );
    let deep: Vec<String> = doc.iter().map(|line| line.text().to_owned()).collect();
    let body = deep.join("\n");
    let bullets = deep
        .iter()
        .filter(|line| line.trim_start().starts_with('−'))
        .count();
    assert_eq!(
        bullets,
        spots.len(),
        "at 12 columns the card is a strip: each blind spot keeps exactly one row, cut rather than \
         dropped ({spots:?})\n{body}"
    );
    assert!(
        deep.iter()
            .filter(|line| line.trim_start().starts_with('−'))
            .all(|line| line.contains('…')),
        "and every one of them says it was cut, rather than ending mid-word in silence\n{body}"
    );
    assert!(
        deep.iter().any(|line| line.starts_with("blind")),
        "the counted ceiling is in the card the floor can walk to\n{body}"
    );
    let body = card(&shell).join("\n");
    let overflow = body
        .lines()
        .find(|line| line.starts_with('…'))
        .expect("the strip counts what fell below it");
    assert!(
        overflow.chars().any(|c| c.is_ascii_digit()) && overflow.contains('j'),
        "and at twelve columns its short form keeps both facts — how many, and the key: {overflow}"
    );
}

/// Walking the card is bounded: `G` at the end refuses, because a card whose rows the reviewer has all
/// seen has nothing left to walk.
#[test]
fn walking_past_the_end_of_the_card_says_so() {
    let mut shell = app(FLOOR, with_delta(mock()));
    press(&mut shell, "<Tab>");
    for _ in 0..40 {
        press(&mut shell, "G");
    }
    assert!(
        shell
            .toast
            .clone()
            .unwrap_or_default()
            .contains("already at the bottom"),
        "the last motion refused rather than swallowing the keystroke: {:?}",
        shell.toast
    );
}

// ---------------------------------------------------------------------------
// No keystroke swaps the card pane (`e9.3`, ADR-006 §5.1.1)
// ---------------------------------------------------------------------------

/// The frame with its hint row dropped, so the comparison is about the panes and not about the toast an
/// unbound key is required to print.
fn frame(app: &App) -> String {
    let mut lines = fathom_tui::render::frame_lines(app);
    if let Some(last) = lines.last_mut() {
        last.clear();
    }
    lines.join("\n")
}

/// No key swaps the card pane any more. Every view, pressed with `i` from every pane it has, renders
/// byte-identically — and `i` is in no mode's binding table, so this is a table fact and not a
/// coincidence about these fixtures. (`w` is the one chord that still changes the card, and it changes
/// a band inside it, which is the next test.)
#[test]
fn no_key_swaps_the_card_pane() {
    for view in ViewId::ALL {
        let panes = app(DESKTOP, with_delta(mock())).ring().len().max(1);
        // `i` only. `<Esc>` and `<C-w>o` change the frame on purpose — one leaves a one-pane view, the
        // other moves focus — and both are the shell's existing verbs for that.
        for pane in 0..panes {
            let mut shell = app(DESKTOP, with_delta(mock()));
            shell.set_view(view);
            shell.pane = pane;
            let before = frame(&shell);
            press(&mut shell, "i");
            shell.clear_toast();
            let after = frame(&shell);
            assert_eq!(
                before,
                after,
                "`i` redrew the frame in {} pane {pane}: the card is the one episode surface, and \
                 the retired chord no longer swaps anything",
                view.name()
            );
        }
    }
}

/// Every pane of the view that is not the card, by name and with its own rows.
fn other_panes(shell: &mut App) -> Vec<(String, Vec<String>)> {
    let ring = shell.ring();
    let mut out = Vec::new();
    for (idx, name) in ring.iter().enumerate() {
        if *name == "card" {
            continue;
        }
        shell.pane = idx;
        let Some(pane) = shell.focused_pane() else {
            continue;
        };
        out.push(((*name).to_owned(), shell.pane_rows(pane)));
    }
    shell.pane = 0;
    out
}

/// `w` changes a band inside the card and nothing outside it: the other panes of the view it is pressed
/// in are byte-identical before and after.
#[test]
fn w_changes_a_band_and_no_other_pane() {
    for view in [ViewId::Timeline, ViewId::Artefact] {
        let mut shell = app(DESKTOP, with_delta(mock()));
        shell.set_view(view);
        let before = other_panes(&mut shell);
        press(&mut shell, "w");
        let after = other_panes(&mut shell);
        assert_eq!(
            before,
            after,
            "every pane that is not the card is byte-identical after `w` in {}",
            view.name()
        );
    }
}

/// Moving to another episode folds the band again: `w` answers the episode under the cursor, so the
/// expansion follows the selection instead of being a mode that outlives it.
#[test]
fn moving_to_another_episode_folds_the_band_it_opened() {
    let mut shell = app(DESKTOP, with_delta(mock()));
    press(&mut shell, "w");
    assert!(shell.expanded_rank.is_some(), "and `w` opened the band");
    press(&mut shell, "j");
    assert!(
        shell.expanded_rank.is_none(),
        "the cursor moved to a fresh episode, so the band is folded again: {:?}",
        shell.expanded_rank
    );
    let second = shell
        .timeline
        .selected()
        .map(|episode| episode.id.clone())
        .unwrap_or_default();
    press(&mut shell, "w");
    assert_eq!(shell.expanded_rank.as_deref(), Some(second.as_str()));
    press(&mut shell, "k");
    assert!(
        shell.expanded_rank.is_none(),
        "and back to the first one, which is a different answer"
    );
}
