//! The diff fixture corpus — the shapes `e1.7`'s acceptance criteria name, as bytes on disk.
//!
//! Every file under `tests/fixtures/` is one deliberate case: the ordinary multi-file patch, and then
//! each degenerate shape the viewer has to have a stated answer for (binary, mode-only,
//! rename-with-edits, deleted, oversize, whole-patch-unavailable, CRLF churn, whitespace-only churn,
//! a submodule, an empty patch, and something that is not a patch at all). They are data files rather
//! than string literals for two reasons: a reviewer can `cat` one, and the CRLF and trailing-space
//! cases cannot survive being embedded in Rust source without an editor "helping" them.
//!
//! The corpus is shared by `diff_golden.rs` (what the panes print) and `diff_behaviour.rs` (what a
//! keystroke does) through this one module, so a fixture cannot be "fixed" to make one test pass.

#![allow(dead_code)] // each test binary uses part of it; the whole thing is used across the two

use fathom_tui::app::{App, Model};
use fathom_tui::diff::Budget;
use fathom_tui::diff::{BytesSource, PatchSource, Unavailable};
use fathom_tui::keymap::Keymap;
use fathom_tui::views::ViewId;

/// One fixture, by file-stem. Listing them here rather than reading the directory keeps a deleted
/// fixture a compile error instead of a test that quietly has nothing to do.
#[must_use]
pub fn patch(name: &str) -> &'static str {
    match name {
        "mixed" => include_str!("../fixtures/mixed.diff"),
        "binary" => include_str!("../fixtures/binary.diff"),
        "binary_git" => include_str!("../fixtures/binary_git.diff"),
        "mode_only" => include_str!("../fixtures/mode_only.diff"),
        "rename_edits" => include_str!("../fixtures/rename_edits.diff"),
        "deleted" => include_str!("../fixtures/deleted.diff"),
        "added" => include_str!("../fixtures/added.diff"),
        "crlf" => include_str!("../fixtures/crlf.diff"),
        "whitespace" => include_str!("../fixtures/whitespace.diff"),
        "trailing" => include_str!("../fixtures/trailing.diff"),
        "oversize" => include_str!("../fixtures/oversize.diff"),
        "long_line" => include_str!("../fixtures/long_line.diff"),
        "unavailable" => include_str!("../fixtures/unavailable.diff"),
        "file_unavailable" => include_str!("../fixtures/file_unavailable.diff"),
        "submodule" => include_str!("../fixtures/submodule.diff"),
        "states" => include_str!("../fixtures/states.diff"),
        "empty" => include_str!("../fixtures/empty.diff"),
        "not_a_diff" => include_str!("../fixtures/not_a_diff.diff"),
        other => panic!("no fixture named {other:?} — add it to corpus::patch and tests/fixtures/"),
    }
}

/// Every fixture, with the name it is filed under.
#[must_use]
pub fn corpus() -> Vec<(&'static str, &'static str)> {
    [
        "mixed",
        "binary",
        "binary_git",
        "mode_only",
        "rename_edits",
        "deleted",
        "added",
        "crlf",
        "whitespace",
        "trailing",
        "oversize",
        "long_line",
        "unavailable",
        "file_unavailable",
        "submodule",
        "states",
        "empty",
        "not_a_diff",
    ]
    .iter()
    .map(|name| (*name, patch(name)))
    .collect()
}

/// A source over a fixture, labelled with the fixture's name so anything the pane prints about where
/// the bytes came from says which fixture it is talking about.
#[must_use]
pub fn source(text: &str) -> Box<dyn PatchSource> {
    let _ = text;
    Box::new(BytesSource::new("fixture", String::from(text)))
}

/// The same, named.
#[must_use]
pub fn named_source(name: &str) -> Box<dyn PatchSource> {
    Box::new(BytesSource::new(format!("{name}.diff"), patch(name)))
}

/// A source that cannot be opened, which is how an episode whose packfile is not local arrives.
#[must_use]
pub fn unreachable(name: &str, why: &str) -> Box<dyn PatchSource> {
    Box::new(BytesSource::unavailable(
        format!("{name}.diff"),
        Unavailable::new(why),
    ))
}

/// An app showing one fixture at a size, in the diff view, with the tree focused.
#[must_use]
pub fn app_with(name: &str, size: (u16, u16)) -> App {
    let mut app = blank(size);
    let loaded = app.load_patch(named_source(name));
    assert!(loaded.is_ok(), "{name}: {}", loaded.unwrap_err());
    assert_eq!(
        app.view,
        ViewId::Diff,
        "loading a patch opens the diff view"
    );
    app
}

/// An app showing raw bytes.
#[must_use]
pub fn app_with_text(text: &str, size: (u16, u16)) -> App {
    let mut app = blank(size);
    let loaded = app.load_patch(source(text));
    assert!(loaded.is_ok(), "{}", loaded.unwrap_err());
    app
}

/// An app with a patch source that fails to open.
#[must_use]
pub fn app_unreachable(name: &str, why: &str, size: (u16, u16)) -> App {
    let mut app = blank(size);
    assert!(
        app.load_patch(unreachable(name, why)).is_err(),
        "an unreachable source must report it"
    );
    app
}

/// An app with a non-default budget, for proving a cap is a cap.
#[must_use]
pub fn app_budget(name: &str, budget: Budget, size: (u16, u16)) -> App {
    let mut app = blank(size);
    app.diff.budget = budget;
    let loaded = app.load_patch(named_source(name));
    assert!(loaded.is_ok(), "{name}: {}", loaded.unwrap_err());
    app
}

/// An app in the diff view with nothing loaded — the state a session starts in.
#[must_use]
pub fn blank(size: (u16, u16)) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        size,
        "fathom/diff · fixture",
    );
    app.set_view(ViewId::Diff);
    app
}

/// The pane named, focused.
pub fn focus(app: &mut App, pane: &str) {
    for _ in 0..app.ring().len() {
        if app
            .focused_pane()
            .is_some_and(|current| current.name == pane)
        {
            return;
        }
        app.focus_next();
    }
    panic!(
        "no pane named {pane:?} in the diff view (ring is {:?})",
        app.ring()
    );
}

/// Press a key the way the terminal would: through the chord machine, so an unbound or mistyped key
/// fails the test the way it would fail a user.
pub fn press(app: &mut App, chord: &str) {
    let fired = app.feed_key(chord, std::time::Instant::now());
    assert!(
        !fired.is_empty(),
        "{chord:?} dispatched nothing — is it still bound in the diff mode?"
    );
}

/// Press a key and return the actions it resolved to, for the tests that assert on the binding.
#[must_use]
pub fn pressed(app: &mut App, chord: &str) -> Vec<String> {
    app.feed_key(chord, std::time::Instant::now())
}

/// A patch with `files` files and `hunks_per_file` hunks of `lines` lines each, generated rather
/// than committed: a 50 000-line fixture in git would be a 2 MB blob for a test that can build it in
/// a millisecond, and the acceptance criterion is about the line count, not the bytes on disk.
#[must_use]
pub fn generated(files: usize, hunks_per_file: usize, lines: usize) -> String {
    let mut out = String::new();
    let mut total = 0_usize;
    for file in 0..files {
        let path = format!("src/generated/file{file:03}.rs");
        out.push_str(&format!("diff --git a/{path} b/{path}\n"));
        out.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
        for hunk in 0..hunks_per_file {
            let start = 1 + hunk * (lines * 2 + 3);
            out.push_str(&format!("@@ -{start},{lines} +{start},{lines} @@\n"));
            for line in 0..lines {
                out.push_str(&format!("-old {file}-{hunk}-{line}\n"));
                total += 1;
            }
            for line in 0..lines {
                out.push_str(&format!("+new {file}-{hunk}-{line}\n"));
                total += 1;
            }
        }
    }
    assert!(
        total >= files * hunks_per_file * lines,
        "the generator has to generate"
    );
    out
}

/// The text of a drawn frame, as one block, for `contains` assertions.
#[must_use]
pub fn frame_text(app: &App) -> String {
    fathom_tui::render::frame_lines(app).join("\n")
}

/// The rows of one diff pane, as text, at the app's current size and scroll.
#[must_use]
pub fn pane_rows(app: &App, pane: &str) -> Vec<String> {
    let view = fathom_tui::views::view(ViewId::Diff);
    let found = view
        .panes
        .iter()
        .find(|candidate| candidate.name == pane)
        .unwrap_or_else(|| panic!("no pane named {pane:?}"));
    app.pane_rows(found)
}

/// The hunks pane's rows at the app's current size and scroll.
#[must_use]
pub fn hunk_rows(app: &App) -> Vec<String> {
    pane_rows(app, "hunks")
}

/// The tree pane's rows.
#[must_use]
pub fn tree_rows(app: &App) -> Vec<String> {
    pane_rows(app, "tree")
}
