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
    let day = |ago: i64| {
        Some(Timestamp::from_unix_micros(
            1_760_000_000_000_000 - ago * 86_400_000_000,
        ))
    };
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
    corpus.push(Entry::dependency(
        "serde",
        "cargo",
        Some("Cargo.toml".to_owned()),
        None,
    ));
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

/// The same pane folded into one string. The pane wraps at its own width and bullets its notes, so a claim
/// about a sentence has to be read the way a reviewer reads it — as a stream of words, not as one line.
fn prose(app: &App) -> String {
    preview(app)
        .iter()
        .map(|line| line.trim_start_matches('!').trim())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
    for token in [
        "kind:", "path:", "dep:", "finding:", "since:", "until:", "risk>", "risk<",
    ] {
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
    assert_eq!(
        app.view,
        ViewId::Timeline,
        "an episode opens where episodes are read"
    );
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
    assert_eq!(
        dep.view,
        ViewId::Deps,
        "a dependency opens in the deps view"
    );
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
        preview(&app).iter().any(|line| line.contains("symbol")),
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

/// Type into the box the way the terminal sends it: one key event per character, so a space is
/// the chord `<Space>` and not a shortcut only the test suite can use.
fn type_query(app: &mut App, text: &str) {
    use crossterm::event::{KeyCode, KeyModifiers};
    for ch in text.chars() {
        app.feed_key_event(
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            std::time::Instant::now(),
        );
    }
}

#[test]
fn a_documented_query_can_actually_be_typed() {
    // The grammar (§5.2) is free text plus filter tokens, and the thing between two tokens is a
    // space. A dispatch path that reads the terminal's `<Space>` as "a chord, therefore not text"
    // makes every documented query untypeable — `author:mira path:src/harbour` arrives as one
    // mashed word, which the session then remembers as though the reviewer had written it.
    let mut app = shell((110, 40));
    app.dispatch("search.open");
    app.dispatch("prompt.clear_line");
    type_query(&mut app, "author:mira path:src/harbour");
    assert_eq!(
        app.prompt, "author:mira path:src/harbour",
        "the field holds what was typed, both spaces included"
    );
    let report = app.search_report.clone().unwrap();
    assert_eq!(report.query, "author:mira path:src/harbour");
    assert_eq!(
        report.filters,
        vec!["author:mira".to_owned(), "path:src/harbour".to_owned()],
        "two tokens were parsed, not one word: {:?}",
        report.filters
    );

    // Free text and a filter token compose, which is the whole point of the grammar.
    app.dispatch("prompt.clear_line");
    type_query(&mut app, "epoch store author:dana");
    let report = app.search_report.clone().unwrap();
    assert!(
        report
            .hits
            .iter()
            .any(|hit| hit.label.contains("epoch-safe")),
        "the Dana episode is what the corpus has for that question: {:?}",
        report
            .hits
            .iter()
            .map(|hit| hit.label.clone())
            .collect::<Vec<_>>()
    );

    // And the session remembers the query the reviewer actually asked, spaces and all.
    app.dispatch("prompt.submit");
    assert_eq!(app.last_search.as_deref(), Some("epoch store author:dana"));
    let rows = fathom_tui::session::snapshot(&app);
    let mut fresh = shell((110, 40));
    let _ = fathom_tui::session::restore(&mut fresh, &rows);
    assert_eq!(
        fresh.last_search.as_deref(),
        Some("epoch store author:dana"),
        "the spaces survive the restart too"
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

// ---------------------------------------------------------------------------
// the symbol section (`reco-6sw`)
// ---------------------------------------------------------------------------

/// A corpus in the shape `search::corpus` produces now that the extractor of `e3.1` feeds it: a working
/// tree's declarations beside the episodes, each naming the file it was read out of, and a stamp for the
/// tree and the store epoch they came from.
fn symbol_corpus() -> Corpus {
    let mut corpus = Corpus::new();
    corpus.documents = 1;
    corpus.epoch = Some(4);
    corpus
        .identity
        .set(fathom_analyze::search::SOURCE_INDEX, "epoch=4");
    corpus.identity.set(
        fathom_analyze::search::SOURCE_SYMBOLS,
        "aabbccddeeff · the working tree `pilot` · 2 files",
    );
    corpus.push(Entry::episode(
        "e8f1a2c3d4",
        "epoch-safe store reads",
        "pin the epoch before the read so the rail cannot see a half-written epoch",
        Some("Dana Stahl".to_owned()),
        Some(Timestamp::from_unix_micros(1_760_000_000_000_000)),
        Some(82),
    ));
    let mut symbol = Entry::symbol(
        "parse_lockfile",
        "fn",
        "src/parse.rs",
        4,
        Some(Timestamp::from_unix_micros(1_760_000_000_000_000)),
    );
    symbol.prior = 0;
    corpus.push(symbol);
    corpus
}

fn symbol_shell() -> App {
    let mut app = shell((110, 40));
    app.search = Some(Arc::new(symbol_corpus().index()));
    app
}

#[test]
fn a_symbol_is_listed_as_one_and_names_where_it_lives() {
    let mut app = symbol_shell();
    app.dispatch("search.open");
    for ch in "parse_lockfile".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    let joined = results(&app).join("\n");
    assert!(
        joined.contains("sy ") && joined.contains("parse_lockfile"),
        "the row carries the two-character kind and the name: {joined}"
    );
    assert!(
        joined.contains("src/parse.rs"),
        "and the file it was read out of: {joined}"
    );
    assert_eq!(
        app.search_report
            .as_ref()
            .and_then(|report| report.hits.first())
            .map(|hit| hit.kind),
        Some(fathom_analyze::search::Kind::Symbol)
    );
}

#[test]
fn enter_on_a_symbol_goes_to_the_file_that_declares_it() {
    let mut app = symbol_shell();
    app.dispatch("search.open");
    for ch in "parse_lockfile".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    app.dispatch("prompt.submit");
    assert_eq!(
        app.view,
        ViewId::Diff,
        "a symbol opens the diff view, which is the pane that can select a file"
    );
    let toast = app.toast.unwrap_or_default();
    assert!(
        toast.contains("src/parse.rs"),
        "and the toast names the file, because nothing was selected to prove it: {toast}"
    );
    assert!(
        toast.contains("not in the patch on screen"),
        "with no patch loaded there is nothing to select in, and the box says that rather than \
         pretending: {toast}"
    );
}

#[test]
fn a_corpus_older_than_the_index_says_so_in_the_box() {
    // The corpus is read once, at startup; the watcher keeps the frame's index facts current. When the two
    // disagree the reviewer is told which epoch `/` is answering from instead of being handed a confident
    // list of rows a rebuild has already replaced.
    let mut app = symbol_shell();
    app.model.index_facts =
        fathom_tui::index::Facts::present(Some(9), Some("ready".to_owned()), Some(3), Some(3));
    app.dispatch("search.open");
    for ch in "parse_lockfile".chars() {
        app.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    let shown = prose(&app);
    assert!(
        shown.contains("older than the repository")
            && shown.contains("the index epoch came from `epoch=4`")
            && shown.contains("is now `epoch=9`"),
        "the sentence names the epoch the rows came from and the one they are being compared with: {shown}"
    );

    // Same epoch, no warning: the check is a comparison, not a mood.
    let mut current = symbol_shell();
    current.model.index_facts =
        fathom_tui::index::Facts::present(Some(4), Some("ready".to_owned()), Some(3), Some(3));
    current.dispatch("search.open");
    for ch in "parse_lockfile".chars() {
        current.feed_key(&ch.to_string(), std::time::Instant::now());
    }
    assert!(
        !preview(&current)
            .iter()
            .any(|line| line.contains("older than the repository")),
        "a corpus that is current is not apologising for it: {:?}",
        preview(&current)
    );
}

#[test]
fn an_empty_box_names_the_symbol_section() {
    let mut app = symbol_shell();
    app.dispatch("search.open");
    assert!(
        results(&app)
            .iter()
            .any(|line| line.contains("1 symbol names from the working tree")),
        "the box says which half of its corpus is empty: {:?}",
        results(&app)
    );
}
