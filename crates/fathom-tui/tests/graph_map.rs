//! The architecture map (`e3.4`, `reco-w3w`), tested as the four things ADR-006 §5.5 says it is.
//!
//! The ADR gives the view four jobs and one rule. The jobs: the structure map, the dependency map, the
//! per-episode delta overlay, and the file-and-line citation behind an arrow. The rule is the one that makes
//! them testable: **degrade by aggregation, never by clipping** — a canvas that has more cells than it has
//! room does not drop the extras, it folds them and prints the count.
//!
//! So every test here is an arithmetic claim about a drawing rather than a screenshot: the cells the hand
//! carried are either drawn, folded into a drawn cell, or counted as undrawn *in words the pane prints*.
//! A picture that quietly omits 2 000 modules is the failure this file exists to catch, and it is invisible
//! in a screenshot at 80×24.
//!
//! The second family is the overlay: an episode's additions, removals and reversals have to be *seen* in a
//! monochrome terminal, which means a glyph and a word as well as a colour (§10.2), and an edge has to be
//! able to say which statement produced it — including when it cannot, where the honest answer is a counted
//! "I don't know which line" rather than a guessed one.
//!
//! The third is the boundary: the shell is handed a [`Hand`] and may not go and look (ADR-002 §5.2, lint
//! `E01`), so a missing figure must arrive as a reason and not as a zero.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use fathom_tui::app::{App, Model};
use fathom_tui::graph::hand::{
    Cell as HandCell, CellKind, Delta, Evidence, Hand, Heat, Line, Link as HandLink, LinkKind,
    Mark, Op, Series, Subject, Truncated,
};
use fathom_tui::graph::{CitationSource, Graph, Layout, Level};
use fathom_tui::keymap::Keymap;
use fathom_tui::views::{ViewId, view};

/// The frame the ticket's legibility criterion is written at.
const SIZE: (u16, u16) = (80, 24);

/// The canvas's own size at the shipped frame, which is what the view is asked to draw into.
const CANVAS: (usize, usize) = (54, 14);

// --- the fixtures ------------------------------------------------------------------------------

/// A hand over `n` modules in a chain, with one layer tag per group of ten.
fn chain(n: usize) -> Hand {
    let mut cells = Vec::new();
    let mut links = Vec::new();
    for index in 0..n {
        cells.push(HandCell::module(
            format!("crates/alpha/src/m{index:04}.rs"),
            Some(format!("layer{}", index / 10)),
        ));
        if index > 0 {
            links.push(HandLink::new(
                format!("crates/alpha/src/m{:04}.rs", index - 1),
                format!("crates/alpha/src/m{index:04}.rs"),
                LinkKind::Imports,
                format!("crates/alpha/src/m{index:04}.rs"),
                Line::At(3),
            ));
        }
    }
    let mut hand = Hand::new("abc123def4", cells, links);
    hand.layers = (0..(n / 10 + 1))
        .map(|group| format!("layer{group}"))
        .collect();
    hand.cost = Some("adjacent · 0.19 ms".to_owned());
    hand.heat = Heat {
        churn: Series::measured(
            (0..n)
                .map(|index| {
                    (
                        format!("crates/alpha/src/m{index:04}.rs"),
                        f64::from((index % 7) as u32) / 6.0,
                    )
                })
                .collect(),
            "over the last 30 revisions",
        ),
        complexity: Series::unavailable("complexity is not measured at startup"),
    };
    hand
}

/// A small hand with names a delta can point at, plus the overlay.
fn episode() -> Hand {
    let cells = vec![
        HandCell::module("crates/store/src/spine.rs", Some("persistence".to_owned())),
        HandCell::module("crates/store/src/graph.rs", Some("persistence".to_owned())),
        HandCell::module("crates/analyze/src/risk.rs", Some("analysis".to_owned())),
        HandCell::module("crates/cli/src/main.rs", Some("surface".to_owned())),
    ];
    let links = vec![
        HandLink::new(
            "crates/cli/src/main.rs",
            "crates/analyze/src/risk.rs",
            LinkKind::Imports,
            "crates/cli/src/main.rs",
            Line::At(7),
        ),
        HandLink::new(
            "crates/analyze/src/risk.rs",
            "crates/store/src/spine.rs",
            LinkKind::Uses,
            "crates/analyze/src/risk.rs",
            Line::At(12),
        ),
    ];
    let mut hand = Hand::new("deadbeef", cells, links);
    hand.layers = vec![
        "surface".to_owned(),
        "analysis".to_owned(),
        "persistence".to_owned(),
    ];
    hand.heat = Heat {
        churn: Series::measured(
            vec![
                ("crates/store/src/spine.rs".to_owned(), 1.0),
                ("crates/analyze/src/risk.rs".to_owned(), 0.4),
            ],
            "over the last 30 revisions",
        ),
        complexity: Series::unavailable("complexity is not measured at startup"),
    };
    let mut delta = Delta::empty("old", "deadbeef");
    delta.rung = "adjacent".to_owned();
    delta.counts = vec![("edges added".to_owned(), 1), ("nodes added".to_owned(), 1)];
    delta.ops = vec![
        Op {
            finding: "edges_added".to_owned(),
            subject: Subject::Edge {
                src: "crates/cli/src/main.rs".to_owned(),
                dst: "crates/analyze/src/risk.rs".to_owned(),
                kind: Some(LinkKind::Imports),
            },
            mark: Mark::Added,
            why: Some("new".to_owned()),
            evidence: Some(Evidence::at("crates/cli/src/main.rs", 7)),
        },
        Op {
            finding: "node_added".to_owned(),
            subject: Subject::Node {
                key: "crates/analyze/src/risk.rs".to_owned(),
            },
            mark: Mark::Added,
            why: None,
            evidence: None,
        },
        Op {
            finding: "edges_removed".to_owned(),
            subject: Subject::Edge {
                src: "crates/analyze/src/risk.rs".to_owned(),
                dst: "crates/store/src/gone.rs".to_owned(),
                kind: Some(LinkKind::Uses),
            },
            mark: Mark::Removed,
            why: Some("unwired".to_owned()),
            evidence: Some(Evidence::file("crates/analyze/src/risk.rs", Line::At(19))),
        },
    ];
    delta.violations = 1;
    hand.delta = delta;
    hand
}

fn shell(hand: Option<Hand>) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        SIZE,
        "fathom/pilot · main",
    );
    app.set_view(ViewId::Graph);
    if let Some(hand) = hand {
        app.install_graph(Some(hand));
    }
    app
}

fn pane(name: &str) -> fathom_tui::views::Pane {
    *view(ViewId::Graph)
        .panes
        .iter()
        .find(|pane| pane.name == name)
        .expect("the graph view declares that pane")
}

fn canvas(app: &App) -> Vec<String> {
    app.graph_text(&pane("canvas")).unwrap_or_default()
}

fn joined(lines: &[String]) -> String {
    lines.join("\n")
}

/// Fold the map back out to one cell per module.
///
/// The canvas opens at whatever aggregation fits its width — that is the rule under test elsewhere — and
/// these tests are about what a *cell* says, so they ask for the deepest level first. `]`/`[` are the
/// reviewer's own keys for the same move, which is why the test drives the key and not a field.
fn unfold(graph: &mut Graph) {
    while graph.dispatch("graph.level_down") {}
}

/// The same, for a test that drives the shell rather than the view. The shell's `dispatch` returns nothing
/// (a keystroke is not a question), so this asks a bounded number of times — past the top the view refuses
/// and the level simply stops moving.
fn unfold_app(app: &mut App) {
    for _ in 0..12 {
        app.dispatch("graph.level_down");
    }
}

/// A styled drawing as plain text, which is what these assertions read: the glyphs and the words, with the
/// colour deliberately thrown away, because §10.2's rule is that the facts survive without it.
fn flat(lines: &[fathom_tui::style::StyledLine]) -> String {
    lines
        .iter()
        .map(|line| line.text())
        .collect::<Vec<&str>>()
        .join("\n")
}

// --- 1. degrade by aggregation, never by clipping ----------------------------------------------

#[test]
fn three_thousand_modules_are_folded_and_say_they_are_folded() {
    let app = shell(Some(chain(3_000)));
    let text = joined(&canvas(&app));

    // Every drawn row is a row of the pane: no more, and nothing dropped off the bottom without a word.
    assert!(!text.is_empty(), "a 3 000-module hand drew nothing at all");
    assert!(
        text.contains("aggregated") || text.contains("folded"),
        "the canvas does not say it aggregated: {text}"
    );
    assert!(
        text.contains("3 000") || text.contains("3000"),
        "the canvas does not print the hand's own total: {text}"
    );
    // Aggregation is at a named level, not "somewhere in here".
    assert!(
        text.contains('L'),
        "the canvas does not name the level it folded at: {text}"
    );
    // The arithmetic itself: nothing is neither drawn nor counted.
    let scene = app
        .graph
        .scene(CANVAS.0, CANVAS.1)
        .expect("a hand has a scene");
    let drawn: usize = scene.cells.iter().map(|cell| cell.members.len()).sum();
    assert_eq!(
        drawn, scene.totals.cells_total,
        "{} modules are drawn and {} folded, out of {} handed — the difference is a module that is \
         neither on screen nor counted",
        drawn, scene.totals.cells_folded, scene.totals.cells_total
    );
}

#[test]
fn a_thousand_modules_draw_inside_a_frame_budget() {
    let mut app = shell(Some(chain(1_000)));
    let started = Instant::now();
    let lines = canvas(&app);
    let first = started.elapsed();
    // A second draw of the same knobs is the cached answer, and that is the number the frame budget is
    // about: the reviewer pressing `j` must not pay for the layout again.
    let again = Instant::now();
    app.dispatch("list.down");
    let lines_again = canvas(&app);
    let second = again.elapsed();

    assert!(
        !lines.is_empty() && lines.len() == lines_again.len(),
        "a re-draw changed the frame's height"
    );
    assert!(
        first.as_millis() < 120,
        "the first draw of 1 000 modules cost {first:?}, over most of a frame budget"
    );
    assert!(
        second.as_millis() < 60,
        "moving the cursor cost {second:?}, which is a re-layout and not a redraw"
    );
}

#[test]
fn folding_counts_every_module_it_merged_and_drops_none() {
    let mut graph = Graph::new();
    graph.install(chain(600));
    let scene = graph.scene(CANVAS.0, CANVAS.1).expect("a scene");
    let drawn: usize = scene.cells.iter().map(|cell| cell.members.len()).sum();
    assert_eq!(
        drawn + scene.totals.cells_folded,
        scene.totals.cells_total,
        "{} drawn + {} folded is not the {} the hand carried",
        drawn,
        scene.totals.cells_folded,
        scene.totals.cells_total
    );
    // And the links are counted too: an arrow that became internal is a counted arrow, not a lost one.
    assert_eq!(
        scene.totals.links_drawn + scene.totals.links_internal + scene.totals.links_undrawn,
        scene.totals.links_total,
        "the arrows do not add up: {}",
        scene.totals.cells_total
    );
}

#[test]
fn a_hand_bigger_than_the_view_refuses_instead_of_quietly_shrinking() {
    let huge = chain(fathom_tui::graph::hand::CELL_CAP + 1);
    let error = huge
        .checked()
        .expect_err("a hand over the cap must be refused, not truncated in place");
    assert!(
        error.contains("truncated"),
        "the refusal does not tell the caller what to do instead: {error}"
    );
}

#[test]
fn a_declared_subset_is_printed_as_a_subset() {
    let mut hand = chain(40);
    hand.truncated = Some(Truncated {
        cells_handed: 40,
        cells_total: 900,
        links_handed: 39,
        links_total: 800,
        reason: "only the modules this episode touched".to_owned(),
    });
    let app = shell(Some(hand));
    let text = joined(&canvas(&app));
    assert!(
        text.contains("40 of 900"),
        "a truncated hand is drawn as though it were the whole graph: {text}"
    );
    assert!(
        text.contains("touched"),
        "the reason for the subset is not printed: {text}"
    );
}

// --- 2. the overlay ----------------------------------------------------------------------------

#[test]
fn the_overlay_marks_added_and_removed_in_words_not_only_colour() {
    let mut graph = Graph::new();
    graph.install(episode());
    unfold(&mut graph);
    // What the canvas and the detail page say between them. Which layout is compiled in decides whether a
    // mark is a box on the canvas or a row in the detail pane (a layered canvas draws arrows, not rows), so
    // the claim this test holds is §10.2's: the fact is on screen with or without colour.
    assert!(graph.focus("crates/analyze/src/risk.rs"));
    let text = format!(
        "{}\n{}",
        flat(&graph.canvas(CANVAS.0, 20)),
        flat(&graph.detail(CANVAS.0, 22))
    );
    assert!(
        text.contains('+') || text.contains("added"),
        "an added dependency is not marked: {text}"
    );
    assert!(
        text.contains('-') || text.contains("removed"),
        "a removed dependency is not marked: {text}"
    );
    // §10.2: a glyph and a word travel with the colour.
    assert!(
        text.contains("delta") || text.contains("overlay"),
        "the canvas does not say it is showing the overlay: {text}"
    );
}

#[test]
fn the_touched_only_overlay_keeps_only_what_this_episode_touched() {
    let mut graph = Graph::new();
    graph.install(episode());
    unfold(&mut graph);
    let before = graph
        .scene(CANVAS.0, CANVAS.1)
        .expect("a scene")
        .cells
        .len();
    assert!(
        graph.dispatch("graph.overlay_only"),
        "the overlay key did nothing"
    );
    let text = flat(&graph.canvas(CANVAS.0, 20));
    let after = graph
        .scene(CANVAS.0, CANVAS.1)
        .expect("a scene")
        .cells
        .len();
    assert!(
        after < before,
        "the overlay filter hid nothing: {before} cells before, {after} after"
    );
    assert!(
        !text.contains("crates/store/src/graph.rs"),
        "touched-only still shows a module this episode did not touch: {text}"
    );
    // And it is reversible, which is what "filter" means.
    assert!(graph.dispatch("graph.overlay_only"));
    assert_eq!(
        graph
            .scene(CANVAS.0, CANVAS.1)
            .expect("a scene")
            .cells
            .len(),
        before
    );
}

#[test]
fn a_removed_node_still_appears_as_a_ghost() {
    let mut graph = Graph::new();
    graph.install(episode());
    unfold(&mut graph);
    let scene = graph.scene(CANVAS.0, CANVAS.1).expect("a scene");
    let ghost = scene
        .cells
        .iter()
        .find(|cell| cell.seed().contains("gone"))
        .expect("the delta removed `crates/store/src/gone.rs` and the canvas denies it");
    assert_eq!(
        ghost.mark,
        Some(Mark::Removed),
        "the removed module carries no mark, so it reads as a module that exists"
    );
}

#[test]
fn the_detail_page_lists_this_episodes_rows_with_their_reasons() {
    let mut graph = Graph::new();
    graph.install(episode());
    unfold(&mut graph);
    assert!(graph.focus("crates/analyze/src/risk.rs"));
    let text = flat(&graph.detail(60, 22));
    assert!(
        text.contains("unwired"),
        "the delta's own reason is not printed: {text}"
    );
    assert!(text.contains("adjacent"), "the rung is not printed: {text}");
    assert!(
        text.contains("edges added") || text.contains("added"),
        "the delta's counts are not printed: {text}"
    );
}

// --- 3. the citation ---------------------------------------------------------------------------

#[test]
fn an_arrow_names_the_file_and_line_behind_it() {
    let mut graph = Graph::new();
    graph.install(episode());
    unfold(&mut graph);
    // The episode added `main.rs → risk.rs`, and the hand carried that row's statement at line 7. The
    // detail pane must print it whichever end the cursor sits on, because a reviewer arrives from either.
    let mut printed = Vec::new();
    for key in ["crates/cli/src/main.rs", "crates/analyze/src/risk.rs"] {
        assert!(graph.focus(key), "{key} is not a cell");
        for row in 0..graph.detail_rows().max(1) {
            graph.set_detail_row(row);
            printed.push(flat(&graph.detail(CANVAS.0, 22)));
        }
    }
    let all = printed.join("\n");
    assert!(
        all.contains("crates/cli/src/main.rs:7"),
        "no arrow of the episode's added dependency cites its recorded statement:\n{all}"
    );
}

#[test]
fn a_line_the_provider_knows_is_cited_and_a_line_it_does_not_is_said_so() {
    let mut graph = Graph::new();
    let mut hand = episode();
    // Strip the citations the hand carried, so the provider is the only source left.
    for link in &mut hand.links {
        link.line = Line::Unrecorded;
    }
    for op in &mut hand.delta.ops {
        op.evidence = None;
    }
    graph.install(hand);
    unfold(&mut graph);
    fn one(src: &str, dst: &str) -> Option<Evidence> {
        if src == "crates/cli/src/main.rs" && dst == "crates/analyze/src/risk.rs" {
            Some(Evidence::at("crates/cli/src/main.rs", 7))
        } else {
            None
        }
    }
    graph.set_citations(Some(CitationSource::new(one)));
    let mut cited = false;
    let mut unclear = false;
    for index in 0..graph.cell_count() {
        graph.set_cell(index);
        for row in 0..graph.detail_rows().max(1) {
            graph.set_detail_row(row);
            let text = flat(&graph.detail(CANVAS.0, 22));
            if text.contains("main.rs:7") {
                cited = true;
            }
            if text.contains("no recorded line") || text.contains("no single citation") {
                unclear = true;
            }
        }
    }
    assert!(cited, "the provider's citation is never printed");
    assert!(
        unclear,
        "an arrow with no recorded line was drawn as though it had one"
    );
}

#[test]
fn an_ambiguous_line_is_counted_rather_than_guessed() {
    let note = Line::Ambiguous(3)
        .note()
        .expect("an ambiguity has a reason");
    assert!(
        note.contains('3'),
        "the reason does not say how many candidates there were: {note}"
    );
    assert!(
        Line::Unrecorded.note().is_some(),
        "an unrecorded line prints no reason, which reads as a bug rather than a fact"
    );
}

// --- 4. the knobs ------------------------------------------------------------------------------

#[test]
fn hop_filtering_walks_the_neighbourhood_and_says_the_radius() {
    let mut graph = Graph::new();
    graph.install(chain(60));
    graph.set_cell(0);
    graph.focus("crates/alpha/src/m0000.rs");
    let close = graph
        .scene(CANVAS.0, CANVAS.1)
        .expect("a scene")
        .cells
        .len();
    assert!(graph.dispatch("graph.hop_more"));
    graph.focus("crates/alpha/src/m0000.rs");
    let far = graph
        .scene(CANVAS.0, CANVAS.1)
        .expect("a scene")
        .cells
        .len();
    assert!(
        far >= close,
        "one hop further drew fewer cells: {close} then {far}"
    );
    let text = flat(&graph.canvas(CANVAS.0, 20));
    assert!(
        text.contains("hop"),
        "the canvas does not print the hop radius: {text}"
    );
    while graph.dispatch("graph.hop_less") {}
    assert_eq!(
        graph.knobs().hops,
        0,
        "`<` did not walk the radius back to the cell itself"
    );
    assert!(
        !graph.dispatch("graph.hop_less"),
        "going below hop 0 must refuse, not wrap"
    );
}

#[test]
fn aggregation_levels_fold_and_unfold() {
    let mut graph = Graph::new();
    graph.install(chain(300));
    let auto = graph
        .scene(CANVAS.0, CANVAS.1)
        .expect("a scene")
        .cells
        .len();
    assert!(graph.dispatch("graph.level_up"));
    let coarse = graph
        .scene(CANVAS.0, CANVAS.1)
        .expect("a scene")
        .cells
        .len();
    assert!(
        coarse <= auto,
        "folding one level up drew more cells: {auto} then {coarse}"
    );
    assert!(graph.dispatch("graph.level_down"));
    assert_eq!(
        graph
            .scene(CANVAS.0, CANVAS.1)
            .expect("a scene")
            .cells
            .len(),
        auto,
        "going back down did not restore the level"
    );
    // Up to the top and past it: the view refuses rather than folding the map out of existence.
    for _ in 0..12 {
        let _ = graph.dispatch("graph.level_up");
    }
    assert!(
        !graph
            .scene(CANVAS.0, CANVAS.1)
            .expect("a scene")
            .cells
            .is_empty(),
        "folding past the top emptied the canvas"
    );
}

#[test]
fn auto_aggregation_picks_a_level_the_canvas_can_actually_hold() {
    let mut graph = Graph::new();
    graph.install(chain(900));
    let narrow = graph.scene(30, 10).expect("a scene");
    let wide = graph.scene(90, 22).expect("a scene");
    assert!(
        narrow.cells.len() <= wide.cells.len(),
        "the narrow canvas drew more cells than the wide one, which is how a canvas becomes unreadable"
    );
    assert!(
        narrow.cells.len() <= 30,
        "auto left {} cells for a 30-column canvas",
        narrow.cells.len()
    );
    assert!(matches!(graph.knobs().level, Level::Auto));
}

#[test]
fn every_layout_draws_the_same_hand_and_the_cycle_does_not_repeat() {
    let mut graph = Graph::new();
    graph.install(episode());
    let mut seen = Vec::new();
    for _ in 0..Layout::cycle().len() {
        let text = flat(&graph.canvas(CANVAS.0, 20));
        assert!(!text.is_empty(), "{} drew nothing", graph.layout().name());
        seen.push(graph.layout());
        assert!(graph.dispatch("graph.layout_cycle"));
    }
    assert_eq!(
        seen,
        Layout::cycle().to_vec(),
        "the layout cycle is not the published cycle"
    );
}

#[test]
fn heat_is_a_band_and_a_window_and_never_only_a_colour() {
    let mut graph = Graph::new();
    graph.install(episode());
    let text = flat(&graph.canvas(CANVAS.0, 20));
    assert!(
        text.contains("heat") || text.contains("churn"),
        "the canvas does not say what the heat means: {text}"
    );
    assert!(
        text.contains("30 revisions") || text.contains("window"),
        "the heat is printed without its denominator: {text}"
    );
    assert!(graph.dispatch("graph.heat_cycle"));
    let other = flat(&graph.canvas(CANVAS.0, 20));
    assert!(
        other.contains("complexity"),
        "cycling the basis did not change what the canvas says it is measuring: {other}"
    );
    assert!(
        other.contains("not measured"),
        "an unavailable basis was drawn as a cold map instead of a stated gap: {other}"
    );
}

#[test]
fn a_missing_figure_arrives_as_a_reason_and_not_as_a_zero() {
    let mut hand = episode();
    hand.heat = Heat {
        churn: Series::unavailable("the churn window could not be read"),
        complexity: Series::unavailable("complexity is not measured at startup"),
    };
    let mut graph = Graph::new();
    graph.install(hand);
    unfold(&mut graph);
    let text = flat(&graph.canvas(CANVAS.0, 20));
    assert!(
        text.contains("could not be read"),
        "an unmeasured heat series printed a number instead of its reason: {text}"
    );
}

#[test]
fn a_layer_filter_narrows_the_map_and_clearing_it_widens_it_again() {
    let mut graph = Graph::new();
    graph.install(episode());
    unfold(&mut graph);
    graph.filter_layer(Some("persistence".to_owned()));
    let filtered = graph.scene(CANVAS.0, CANVAS.1).expect("a scene");
    assert!(
        filtered.cells.len() <= 2,
        "filtering to one layer left {} cells",
        filtered.cells.len()
    );
    graph.clear_filter();
    assert_eq!(
        graph
            .scene(CANVAS.0, CANVAS.1)
            .expect("a scene")
            .cells
            .len(),
        5,
        "the four handed modules plus the ghost the delta named"
    );
}

// --- 5. the boundary and the shell's wiring ------------------------------------------------------

#[test]
fn nothing_handed_keeps_the_published_placeholder() {
    let app = shell(None);
    assert!(
        app.graph_text(&pane("canvas")).is_none(),
        "an empty map overwrote the published placeholder with its own words"
    );
    assert!(!app.graph.has_data());
}

#[test]
fn a_graph_key_with_no_data_says_what_would_give_it_data() {
    let mut app = shell(None);
    app.dispatch("graph.level_up");
    let text = app.toast.clone().unwrap_or_default();
    assert!(
        text.contains("fathom index") || text.contains("no graph"),
        "a graph key over an empty map was silent: {text:?}"
    );
}

#[test]
fn every_published_graph_action_is_bound_and_refused_or_handled() {
    let keymap = Keymap::defaults();
    let actions = [
        "graph.level_up",
        "graph.level_down",
        "graph.hop_less",
        "graph.hop_more",
        "graph.layout_cycle",
        "graph.overlay_only",
        "graph.heat_cycle",
    ];
    for action in actions {
        assert!(
            !keymap.bindings_of(action).is_empty(),
            "{action} is published in ADR-006 §7.4 and bound by nothing"
        );
        let mut probe = Graph::new();
        probe.install(episode());
        assert!(
            probe.dispatch(action),
            "{action} is in the registry and refused by the view"
        );
    }
}

#[test]
fn a_click_selects_the_cell_the_keyboard_would_have_selected() {
    let mut app = shell(Some(episode()));
    unfold_app(&mut app);
    app.dispatch("list.down");
    let by_key = app.selected_row();
    assert_eq!(
        by_key,
        app.graph.selected().unwrap_or(usize::MAX),
        "the shell's cursor and the view's cursor disagree after a keystroke"
    );
    let drawn = canvas(&app);
    assert!(drawn.len() > 5, "the canvas drew {} rows", drawn.len());
    // A canvas has header rows above the picture: the first row that names a cell is the row a pointer
    // would have to land on, and every row above it is prose.
    let first_hit =
        (0..drawn.len()).find_map(|line| app.row_at_line(&pane("canvas"), line, drawn.len()));
    assert!(
        first_hit.is_some(),
        "no row of the canvas could be hit at all — the picture has no addressable cells"
    );
    // The wheel over a canvas is the same motion, because a canvas has no window to scroll.
    app.scroll_by(1);
    assert_eq!(
        app.selected_row(),
        by_key + 1,
        "the wheel over a canvas did not move the cell"
    );
}

#[test]
fn the_view_keeps_its_knobs_across_a_reinstall_and_drops_its_stale_cursor() {
    let mut app = shell(Some(episode()));
    unfold_app(&mut app);
    app.dispatch("list.down");
    assert_eq!(app.selected_row(), 1, "the canvas cursor did not move");
    // Re-handing the graph is a new revision: the knobs stay, the selection cannot point at an index that
    // may now mean something else.
    app.install_graph(Some(episode()));
    assert_eq!(
        app.graph.knobs().hops,
        2,
        "the hop radius was reset by a re-hand"
    );
    assert!(
        app.graph.selected().is_none(),
        "a re-hand left the cursor pointing at an index into the old graph"
    );
}

#[test]
fn a_layer_tag_the_config_never_declared_is_its_own_band() {
    let mut hand = episode();
    // `e3.3`'s drift: the revision's rows carry a tag `[layers]` does not list.
    hand.cells.push(HandCell::module(
        "crates/stray/src/x.rs",
        Some("undeclared".to_owned()),
    ));
    let mut graph = Graph::new();
    graph.install(hand);
    unfold(&mut graph);
    let scene = graph.scene(CANVAS.0, CANVAS.1).expect("a scene");
    assert!(
        scene
            .cells
            .iter()
            .any(|cell| cell.layer.as_deref() == Some("undeclared")),
        "an undeclared layer tag was sorted away, which is how drift becomes invisible"
    );
}

#[test]
fn a_symbol_edge_is_folded_onto_its_module_and_counted_as_mixed() {
    let mut hand = Hand::new(
        "abc",
        vec![
            HandCell::module("crates/a/src/deep/lib.rs", None),
            HandCell::module("crates/b/src/deep/lib.rs", None),
            HandCell {
                key: "crates/b/src/deep/lib.rs::Thing".to_owned(),
                kind: CellKind::Symbol,
                layer: None,
                may_have_shrunk: false,
                present: true,
                evidence: None,
            },
        ],
        vec![
            HandLink::new(
                "crates/a/src/deep/lib.rs",
                "crates/b/src/deep/lib.rs::Thing",
                LinkKind::Uses,
                "crates/a/src/deep/lib.rs",
                Line::At(4),
            ),
            HandLink::new(
                "crates/a/src/deep/lib.rs",
                "crates/b/src/deep/lib.rs",
                LinkKind::Imports,
                "crates/a/src/deep/lib.rs",
                Line::At(3),
            ),
        ],
    );
    hand.layers = vec![];
    let mut graph = Graph::new();
    graph.install(hand);
    // Fold at the level where the file and its symbol share a cell, which is where a symbol arrow has to
    // be folded onto its module and *counted*. `Level::Depth` is the same state `]` moves, spelled once.
    graph.set_level(Level::Depth(4));
    let scene = graph.scene(CANVAS.0, CANVAS.1).expect("a scene");
    let arrow = scene
        .links
        .iter()
        .find(|link| link.src.contains("a/src") && link.dst.contains("b/src"))
        .expect("no arrow between the two modules");
    assert!(
        arrow.kind.is_none() && arrow.rows == 2,
        "a `uses` arrow onto a symbol and an `imports` arrow onto the same file were drawn as one kind, \
         which claims a dependency the graph did not declare: {arrow:?}"
    );
    assert!(
        graph.focus("crates/a/src/deep/lib.rs"),
        "the folded cell is not there"
    );
    let text = flat(&graph.detail(CANVAS.0, 22));
    assert!(
        text.contains("mixed"),
        "the merged arrow is not called mixed in words: {text}"
    );
}

#[test]
fn the_published_defaults_are_the_defaults_the_adr_prints() {
    let graph = Graph::new();
    let knobs = graph.knobs();
    assert_eq!(knobs.hops, 2, "the published default hop is not 2");
    assert!(!knobs.only_touched, "the map must not open as an overlay");
    assert!(matches!(knobs.level, Level::Auto));
    assert_eq!(graph.layout(), Layout::preferred());
    assert!(graph.selected().is_none());
    assert_eq!(graph.filter(), &fathom_tui::graph::Filter::default());
}

// --- 6. the frame the ticket's legibility criterion is written at --------------------------------

/// Every line of a drawn frame is exactly the frame's width, which is the only way to know nothing was
/// clipped at the right border without looking at it.
fn widths_are_uniform(lines: &[String], width: usize) -> Option<usize> {
    lines
        .iter()
        .find(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) > width)
        .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
}

#[test]
fn the_map_is_legible_in_the_80x24_frame_and_says_what_the_picture_is() {
    use fathom_tui::render::frame_lines;

    let mut app = shell(Some(episode()));
    unfold_app(&mut app);
    // A reviewer's first move: pick a cell. The citation is a fact about an arrow, and an arrow needs an
    // end — an unselected map prints the whole map's facts, which is the right answer and does not yet
    // contain an `evidence` line.
    assert!(app.graph.focus("crates/analyze/src/risk.rs"));
    let lines = frame_lines(&app);
    assert_eq!(
        lines.len(),
        usize::from(SIZE.1),
        "the graph frame is not 24 rows at 80×24"
    );
    if let Some(wide) = widths_are_uniform(&lines, usize::from(SIZE.0)) {
        panic!("a graph frame line is {wide} cells wide in an 80-column frame");
    }
    let all = lines.join("\n");
    // The four things §5.5 says the view answers, each named in the frame rather than implied by a shape.
    for fact in ["hop", "delta overlay", "nodes", "aggregated", "evidence"] {
        assert!(
            all.contains(fact),
            "the 80×24 graph frame never prints `{fact}`, so the reader has to guess it: {all}"
        );
    }
    // And the pane footer says who owns the pane, which is how a reviewer reports a wrong picture.
    assert!(
        all.contains("e3.4"),
        "the frame does not name its owning ticket: {all}"
    );
}

#[test]
fn a_narrow_frame_folds_harder_and_says_so_instead_of_dropping_cells() {
    use fathom_tui::render::frame_lines;

    let mut app = shell(Some(chain(400)));
    app.resize(40, 12);
    let lines = frame_lines(&app);
    assert_eq!(lines.len(), 12, "a 12-row frame drew {} rows", lines.len());
    if let Some(wide) = widths_are_uniform(&lines, 40) {
        panic!("a 40-column graph frame printed a {wide}-cell line");
    }
    let all = lines.join("\n");
    assert!(
        all.contains("aggregated") || all.contains("folded"),
        "a 40-column frame of a 400-module graph says nothing about how it fitted: {all}"
    );
}
