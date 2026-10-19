//! Every pane's empty state, audited as a set (`e5.9`, reco-2uo).
//!
//! ADR-006 §10.3 says *"a shrug is not a remedy"*, and for nine views' worth of panes the only way to hold
//! that line is to enumerate them: a per-pane rule that is checked one pane at a time is a rule checked
//! never, because the pane nobody thought to look at is the one that prints `` `manifest` — nothing to show
//! yet`` for two more releases. This file walks the shipped view registry — every view, every pane it
//! declares, in every index state a caller can hand it — and fails on any of four things:
//!
//! 1. an empty pane whose copy is the generic fallback, in a view that has a specific thing to say;
//! 2. copy that states a cause and offers no action;
//! 3. an action that cannot be taken: a keystroke the shipped keymap does not bind in that view, or a
//!    `fathom` verb the CLI does not have (the published list, checked against the binary by
//!    `crates/fathom-cli/tests/copy.rs`);
//! 4. two views saying the same nothing about panes that are empty for different reasons.
//!
//! The copy itself lives in `fathom_tui::empty`, one module, reviewed as one batch — this file is what makes
//! that claim checkable rather than aspirational.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_tui::empty::{Reason, words};
use fathom_tui::index::Facts;
use fathom_tui::keymap::{Keymap, Mode};
use fathom_tui::views::{Pane, Role, ViewId};

/// Every verb `fathom --help` lists in this build, and every global flag.
///
/// Duplicated on purpose, and checked against the binary in `crates/fathom-cli/tests/copy.rs`: the TUI may
/// not depend on the CLI to learn what the CLI can do (ADR-002 §5.2), so the promise is written twice and
/// the join test is what keeps the two copies honest.
const VERBS: &[&str] = &[
    "index",
    "show",
    "attribution",
    "analyze",
    "stats",
    "check",
    "doctor",
    "export",
    "mcp",
    "watch",
    "hooks",
    "tui",
];
const FLAGS: &[&str] = &[
    "--demo",
    "--plain",
    "--no-alt-screen",
    "--fresh",
    "--mouse",
    "--lines",
    "--theme",
    "--version",
    "--help",
    "--json",
    "--stats",
    "--rebuild",
    "--refresh",
    "--watch",
    "--committed-only",
    "--progress",
    "--no-progress",
    "--show-keys",
    "--show-session",
    "--show-theme",
    "--list-themes",
    "--strict",
    "--deep",
    "--privacy",
    "--format",
    "--episodes",
    "--out",
    "--schema",
    "--root",
    "--quiet",
    "--once",
];

/// An index that finished and found things: the ordinary state, in which every pane's emptiness is that
/// view's own business.
fn ready() -> Facts {
    Facts::present(Some(7), Some("ready".to_owned()), Some(500), Some(412))
}

/// Every (view, pane) pair the shipped registry declares.
fn panes() -> Vec<(ViewId, &'static fathom_tui::views::Pane)> {
    let mut out = Vec::new();
    for id in ViewId::ALL {
        for pane in fathom_tui::views::view(id).panes {
            out.push((id, pane));
        }
    }
    out
}

/// The action test: does this sentence offer something a reviewer can *do*?
///
/// Either a keystroke the shipped keymap really binds, in a mode this view reaches, or a `fathom` verb. A
/// sentence with neither is a description of the pane, which is half an empty state: it tells the reviewer
/// what the pane is for and not how to fill it.
fn assert_has_action(what: &str, text: &str, keymap: &Keymap, view: ViewId, pane: &Pane) {
    // The chain a keystroke resolves through from this pane: the pane's own interaction mode, then the
    // view's, then global (ADR-006 §7.1's precedence). Checking only the view mode would call `j` unbound,
    // which is exactly the kind of wrong-but-plausible claim an empty state can make.
    let names_key = backticked(text)
        .iter()
        .any(|token| bound(keymap, view, pane, token));
    let names_verb = text.contains("fathom ");
    assert!(
        names_key || names_verb,
        "{what}: an empty state must offer a keystroke or a verb: {text:?}"
    );
}

/// Every `` `token` `` in a sentence, backticks stripped.
fn backticked(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('`') else { break };
        out.push(tail[..end].to_owned());
        rest = &tail[end + 1..];
    }
    out
}

/// Is this token a keystroke this view can press?
///
/// Multi-key phrases (`fathom analyze <episode> --json`) are not keystrokes and answer false; a token that
/// names two keys at once (`j`/`k`) is checked by its caller, which splits on `/`.
fn bound(keymap: &Keymap, view: ViewId, pane: &Pane, token: &str) -> bool {
    let chain = [pane.interaction(), Mode::for_view(view), Mode::Global];
    token.split('/').any(|key| {
        let key = key.trim();
        !key.is_empty() && keymap.resolve(&chain, key).is_some()
    })
}

/// Rule 1 and 2, in the ordinary state: every pane says something specific, with an action in it.
#[test]
fn every_pane_says_why_it_is_empty_and_what_to_do() {
    let keymap = Keymap::defaults();
    let facts = ready();
    let mut audited = 0;
    for (view, pane) in panes() {
        // Nothing selected, nothing searched for: the state a view is in before any input.
        let lines = words(view, pane, &facts, &Reason::none());
        let text = lines.join(" ");
        audited += 1;
        let what = format!("{} · {}", view.name(), pane.name);
        assert!(
            !text.trim().is_empty(),
            "{what}: an empty empty-state is the shrug this test exists to catch"
        );
        assert!(
            !text.contains("nothing to show yet"),
            "{what}: fell through to the generic fallback: {text:?}"
        );
        assert!(
            !text.contains("is not in this build") || text.contains("e5.7"),
            "{what}: a gap must name its owner: {text:?}"
        );
        assert_has_action(&what, &text, &keymap, view, pane);
        // Rule 2 of the batch: the reason comes first. A primary or secondary pane opens with what the pane
        // is, so the sentence still means something after the action is taken.
        if !matches!(pane.role, Role::Primary | Role::Secondary) {
            continue;
        }
        assert!(
            lines.len() >= 2,
            "{what}: a primary or secondary pane owes a reason *and* an action: {lines:?}"
        );
    }
    // A guard against the registry going quiet: the count is the panes ADR-006 §5 documents.
    assert!(
        audited >= 14,
        "only {audited} panes audited; the registry should declare more than that"
    );
}

/// Rule 3: when the index is the problem, no per-view chore competes with it.
///
/// Four states — never indexed, unreadable, still building, finished with nothing in it — and in every one
/// of them the pane's job is to say what the index said and point at the verb that can go further. A treemap
/// that says "press `j`/`k` to pick an episode" on a repository with no index is not wrong exactly, and is
/// useless, which is worse.
#[test]
fn the_index_axis_wins_whenever_the_index_is_the_problem() {
    let keymap = Keymap::defaults();
    let states = [
        ("no index", Facts::absent()),
        ("unreadable store", Facts::unreadable()),
        (
            "still building",
            Facts::present(Some(2), Some("building".to_owned()), Some(500), Some(0)),
        ),
        (
            "finished, nothing to list",
            Facts::present(Some(1), Some("ready".to_owned()), Some(500), Some(0)),
        ),
    ];
    for (state, facts) in states {
        for (view, pane) in panes() {
            let text = words(view, pane, &facts, &Reason::none()).join(" ");
            let what = format!("{} · {} ({state})", view.name(), pane.name);
            assert!(
                text.contains("fathom index")
                    || text.contains("fathom doctor")
                    || text.contains("fathom watch"),
                "{what}: an index-state empty state must name the verb that changes it: {text:?}"
            );
            assert_has_action(&what, &text, &keymap, view, pane);
        }
    }
}

/// Rule 4: panes that are empty for different reasons must not read the same.
///
/// `tree` is the diff's file tree and the deps manifest's expansion; `detail` is the structure pane's
/// directory and the graph's edge. The same sentence in both would mean the copy was written by pane name
/// rather than by view, which is how one generic string ends up back in nine places.
#[test]
fn panes_that_share_a_name_say_different_things() {
    let facts = ready();
    let pairs = [
        (ViewId::Diff, ViewId::Deps, "tree"),
        (ViewId::Structure, ViewId::Graph, "detail"),
    ];
    for (left, right, pane) in pairs {
        let left_pane = fathom_tui::views::view(left).pane(pane).unwrap();
        let right_pane = fathom_tui::views::view(right).pane(pane).unwrap();
        let before = words(left, left_pane, &facts, &Reason::none()).join(" ");
        let after = words(right, right_pane, &facts, &Reason::none()).join(" ");
        assert_ne!(
            before, after,
            "{pane} says the same thing in {left:?} and {right:?}: {before:?}"
        );
    }
}

/// The copy names only commands this build has — the rule `--demo` was filed against, applied to every pane
/// rather than to the one flag that happened to be noticed.
#[test]
fn no_pane_names_a_command_this_build_does_not_have() {
    let facts = ready();
    for (view, pane) in panes() {
        for reason in [Reason::none(), selected("ep-4a4"), queried("drop column")] {
            for line in words(view, pane, &facts, &reason) {
                for word in line.split_whitespace() {
                    let word = word.trim_matches('`').trim_end_matches([',', ';', '.']);
                    if word.starts_with("--") {
                        assert!(
                            FLAGS.contains(&word),
                            "{} · {} names `{word}`: {line}",
                            view.name(),
                            pane.name
                        );
                    }
                    if let Some(rest) = word.strip_prefix("fathom")
                        && let Some(verb) = rest
                            .trim_start_matches('`')
                            .split(['`', '(', ')'])
                            .next()
                            .filter(|verb| {
                                verb.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                            })
                    {
                        assert!(
                            VERBS.contains(&verb),
                            "{} · {} names the verb `{verb}`: {line}",
                            view.name(),
                            pane.name
                        );
                    }
                }
            }
        }
    }
}

/// Two different reasons must produce two different sentences, or the reason was decorative.
///
/// "Nothing is selected" and "the selected episode touched no manifests" are different states with
/// different actions; a single string covering both means one of them is being lied about.
#[test]
fn a_selection_changes_what_the_pane_says() {
    let facts = ready();
    let selection_views = [
        ViewId::Diff,
        ViewId::Structure,
        ViewId::Graph,
        ViewId::Deps,
        ViewId::Artefact,
    ];
    for (view, pane) in panes() {
        if !selection_views.contains(&view) || !matches!(pane.role, Role::Primary | Role::Secondary)
        {
            // The rail and the worklist *are* the list: no selection changes what an empty one of those
            // means, and pretending otherwise would be the test inventing a dependency the view does not have.
            continue;
        }
        let nothing = words(view, pane, &facts, &Reason::none()).join(" ");
        let selected = words(view, pane, &facts, &selected("ep-4a4")).join(" ");
        assert_ne!(
            nothing,
            selected,
            "{} · {} says the same thing with and without a selection: {nothing:?}",
            view.name(),
            pane.name
        );
    }
}

/// The search pane quotes the reviewer's own query back at them, or says nothing has been asked yet.
///
/// "no results" is a shrug; "nothing matches `drop colum`" is a fact the reviewer can act on — usually by
/// noticing the typo, which is the whole reason the query is repeated.
#[test]
fn the_search_pane_repeats_the_query_or_says_none_was_made() {
    let facts = ready();
    let pane = fathom_tui::views::view(ViewId::Search)
        .pane("results")
        .unwrap_or_else(|| panic!("search has a results pane"));
    let empty_query = words(ViewId::Search, pane, &facts, &Reason::none()).join(" ");
    assert!(empty_query.contains("searched for"), "{empty_query:?}");
    let queried = words(ViewId::Search, pane, &facts, &queried("drop colum")).join(" ");
    assert!(
        queried.contains("drop colum"),
        "the pane did not quote the query back: {queried:?}"
    );
    assert_ne!(empty_query, queried);
}

fn selected(episode: &str) -> Reason {
    Reason {
        episode: Some(episode.to_owned()),
        ..Reason::none()
    }
}

fn queried(query: &str) -> Reason {
    Reason {
        query: Some(query.to_owned()),
        ..Reason::none()
    }
}
