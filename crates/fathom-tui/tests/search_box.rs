//! The `/` box (`e5.2`, `reco-mkc`): what it scores, what its rows say, where `<Enter>` goes, and what
//! survives a restart.
//!
//! The scorer itself is tested in `fathom-analyze` (`tests/search_scoring.rs`: the golden ordering, the
//! total order, the 50 ms budget). What is tested here is the *box*: the corpus is handed in rather than
//! read (ADR-002 §5.2 — the shell may not open a store, and a test that could not tell would not catch a
//! regression into it), every keystroke re-scores, and the row the reviewer opens is the row they were
//! shown.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::Arc;

use fathom_analyze::search::{Corpus, Entry, Index};
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::views::ViewId;

/// A corpus in the shapes `search::corpus` builds from a real index: an episode with its intent text, a
/// path with its episode count, a dependency with its manifest, a finding with its subject.
fn corpus() -> Corpus {
    let day = |ago: i64| Some(Timestamp::from_unix_micros(1_760_000_000_000_000 - ago * 86_400_000_000));
    let mut corpus = Corpus::new();
    corpus.documents = 5;
    // The same gap `search::corpus` declares for a real index: symbols are a kind the scorer can rank and
    // nothing has filled since `e3.1`, and a box that searches a missing kind silently returns nothing.
    corpus.gaps.push(fathom_analyze::search::Gap {
        kind: fathom_analyze::search::Kind::Symbol,
        state: "not indexed".to_owned(),
        because: "no extractor has indexed symbol names since e3.1".to_owned(),
        owner: "e3.1",
    });
    corpus.push(Entry::episode(
        "e8f1a2c3d4",
        "epoch-safe store reads",
        "pin the epoch before the read so the rail cannot see a half-written epoch",
        Some("Dana Stahl".to_owned()),
        day(1),
        Some(82),
    ));
    corpus.push(Entry::path(
        "crates/fathom-store/src/epoch.rs",
        Some("e8f1a2c3d4".to_owned()),
        "3 episodes",
        day(1),
    ));
    corpus.push(Entry::dependency("serde", "cargo", Some("Cargo.toml".to_owned()), None));
    corpus.push(Entry::finding(
        "new-cycle",
        "store→core",
        "a new cycle between the store and the core",
        "high",
        None,
    ));
    corpus.push(Entry::episode(
        "77aa0c99bb",
        "the delta model speaks for itself",
        "one versioned delta document for the TUI, the CLI and the export",
        Some("Priya Raman".to_owned()),
        day(3),
        Some(61),
    ));
    corpus
}

fn shell(size: (u16, u16)) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        size,
        "fathom/pilot · main",
    );
    app.search = Some(Arc::new(Index::build(&corpus())));
    app
}

/// What the results pane is showing.
fn results(app: &App) -> Vec<String> {
    app.model
        .rows
        .get(&(ViewId::Search, "results".to_owned()))
        .cloned()
        .unwrap_or_default()
}

/// What the preview pane is showing.
fn preview(app: &App) -> Vec<String> {
    app.model
        .rows
        .get(&(ViewId::Search, "preview".to_owned()))
        .cloned()
        .unwrap_or_default()
}

#[test]
fn an_empty_box_answers_with_the_grammar_and_the_last_searches() {
    let mut app = shell((110, 40));
    app.recent_searches = vec!["risk>60".to_owned(), "epoch store".to_owned()];
    app.dispatch("search.open");
    let preview = preview(&app);
    assert!(
        preview.iter().any(|line| line.starts_with("author:")),
        "the filter grammar is printed where the query is typed: {preview:?}"
    );
    for token in ["kind:", "path:", "dep:", "finding:", "since:", "until:", "risk>", "risk<"] {
        assert!(
            preview.iter().any(|line| line.starts_with(token)),
            "`{token}` is a filter the parser honours and the box does not list it: {preview:?}"
        );
    }
    let rows = results(&app);
    assert!(
        rows.iter().any(|line| line.contains("5 rows")),
        "the box says how big the corpus is: {rows:?}"
    );
    assert!(
        rows.iter().any(|line| line.contains("epoch store")),
        "the last searches are in the box, not buried in a menu: {rows:?}"
    );
}

#[test]
fn a_box_with_no_corpus_says_what_would_make_it_answer() {
    let mut app = shell((110, 40));
    app.search = None;
    app.dispatch("search.open");
    assert!(
        results(&app)
            .iter()
            .any(|line| line.contains("no index") && line.contains("fathom index")),
        "an unindexed repository is told, not drawn as an empty list: {:?}",
        results(&app)
    );
}

#[test]
fn typing_scores_the_corpus_and_every_row_says_why_it_is_there() {
    let mut app = shell((110, 40));
    app.dispatch("search.open");
    for ch in "epoch".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    let rows = results(&app);
    let joined = rows.join("\n");
    assert!(
        joined.contains("ep ") && joined.contains("epoch-safe store reads"),
        "the episode is in the list: {joined}"
    );
    assert!(
        joined.contains("pa ") && joined.contains("epoch.rs"),
        "the path is in the list too, and a fuzzy box finds it by a substring of a filename: {joined}"
    );
    assert!(
        joined.contains("risk 82"),
        "a row carries the number the reviewer sorts by: {joined}"
    );
    let why = preview(&app).join("\n");
    assert!(
        why.contains("why this matched") && why.contains("epoch"),
        "the preview answers *why*, which is what stops a fuzzy list being a guess: {why}"
    );
    assert!(
        why.contains("= ") || why.contains("~ "),
        "the explanation names the rung that matched: {why}"
    );
}

#[test]
fn enter_opens_the_view_that_owns_the_hit() {
    // An episode hit belongs to the timeline.
    let mut app = shell((110, 40));
    app.dispatch("search.open");
    for ch in "epoch-safe".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    assert_eq!(
        app.search_report
            .as_ref()
            .and_then(|report| report.hits.first())
            .map(|hit| hit.kind),
        Some(fathom_analyze::search::Kind::Episode),
        "the top row is the episode"
    );
    app.dispatch("prompt.submit");
    assert_eq!(app.view, ViewId::Timeline, "an episode opens where episodes are read");
    assert_eq!(app.overlay, None, "and the box is gone");

    // A path hit belongs to the structure view, not the timeline.
    let mut path = shell((110, 40));
    app.dispatch("search.open");
    for ch in "epoch.rs".chars() {
        path.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    path.dispatch("prompt.submit");
    assert_eq!(
        path.view,
        ViewId::Structure,
        "a path goes to the view that lists what touched it"
    );

    // A dependency belongs to the dependency view.
    let mut dep = shell((110, 40));
    dep.dispatch("search.open");
    for ch in "serde".chars() {
        dep.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    dep.dispatch("prompt.submit");
    assert_eq!(dep.view, ViewId::Deps, "a dependency opens in the deps view");
}

#[test]
fn a_hit_with_no_view_says_so_instead_of_jumping_somewhere_nearby() {
    let mut app = shell((110, 40));
    app.dispatch("search.open");
    for ch in "new-cycle".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    app.dispatch("prompt.submit");
    let toast = app.toast.unwrap_or_default();
    assert!(
        toast.contains("finding") && toast.contains("fathom why"),
        "a finding with no episode to open names the command that does: {toast}"
    );
    assert_eq!(
        app.view,
        ViewId::Queue,
        "and the reviewer is not moved somewhere that merely looks related"
    );
}

#[test]
fn nothing_that_matches_says_why_in_the_box_that_asked() {
    let mut app = shell((110, 40));
    app.dispatch("search.open");
    for ch in "zzzznothing".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    let joined = results(&app).join("\n");
    assert!(
        joined.contains("nothing in a corpus of 5 matches"),
        "{joined}"
    );
    let report = app.search_report.clone().unwrap();
    assert_eq!(report.hits.len(), 0);
    assert!(
        report
            .gaps
            .iter()
            .any(|gap| gap.kind == fathom_analyze::search::Kind::Symbol),
        "a corpus that cannot search symbols says so, and the box prints its gaps: {:?}",
        report.gaps
    );
    assert!(
        preview(&app)
            .iter()
            .any(|line| line.contains("symbol")),
        "the gap reaches the preview pane too: {:?}",
        preview(&app)
    );
}

#[test]
fn the_arrow_keys_walk_the_last_searches_and_the_session_carries_them() {
    let mut app = shell((110, 40));
    for query in ["epoch store", "risk>60"] {
        app.dispatch("search.open");
        // The field opens with the last query in it (`e1.11`), so a test that means to ask a *new* question
        // has to clear the field the way a reviewer would.
        app.dispatch("prompt.clear_line");
        for ch in query.chars() {
            app.feed_key(&ch.to_string(), std::time::Instant::now());
        }
        app.dispatch("prompt.submit");
    }
    app.dispatch("search.open");
    app.dispatch("prompt.result_up");
    assert_eq!(
        app.prompt, "risk>60",
        "`↑` in an empty box walks the recent searches, newest first"
    );

    // And they survive the restart: the snapshot carries them, and a fresh shell reads them back.
    let rows = fathom_tui::session::snapshot(&app);
    let mut fresh = shell((110, 40));
    fresh.recent_searches.clear();
    let restored = fathom_tui::session::restore(&mut fresh, &rows);
    assert_eq!(
        fresh.recent_searches,
        vec!["risk>60".to_owned(), "epoch store".to_owned()],
        "the history round-trips through the session rows: {:?}",
        restored.summary()
    );
}

#[test]
fn a_query_with_a_pipe_in_it_is_one_search_and_not_two() {
    // The session stores the history as one `|`-joined row, so the delimiter has to be escaped on the way
    // in — otherwise a reviewer searching a path with a pipe in it gets a history full of fragments.
    let mut app = shell((110, 40));
    app.dispatch("search.open");
    for ch in "store|core".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    app.dispatch("prompt.submit");
    let rows = fathom_tui::session::snapshot(&app);
    let mut fresh = shell((110, 40));
    let restored = fathom_tui::session::restore(&mut fresh, &rows);
    let _ = restored;
    assert_eq!(
        fresh.recent_searches,
        vec!["store|core".to_owned()],
        "a query round-trips whole"
    );
}

#[test]
fn the_help_overlay_lists_the_filters_because_a_box_cannot_teach_its_own_grammar() {
    // A reviewer cannot discover `risk>` by typing into a box they are not typing into yet. The grammar has
    // to be somewhere they read when they give up, and that somewhere is `?` (`reco-mkc`'s third criterion).
    let mut app = shell((120, 44));
    app.overlay = Some(fathom_tui::app::Overlay::Help);
    let lines = fathom_tui::render::frame_lines(&app).join("\n");
    for (token, meaning) in fathom_tui::search::FILTERS {
        assert!(
            lines.contains(token) && lines.contains(meaning.split_whitespace().next().unwrap()),
            "`{token}` ({meaning}) is missing from the help overlay:\n{lines}"
        );
    }
}
