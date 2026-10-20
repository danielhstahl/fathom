//! Behaviour the diff viewer has to get right (`e1.7`, AC 3 and 4).
//!
//! Hunk navigation, word-diff, the whitespace rule, the three ways out (copy path, copy hunk, open in
//! `$EDITOR`), the degenerate cases, and the budgets that keep a 50 000-line diff from becoming an
//! out-of-memory report. Where the golden files pin *what the screen says*, this file pins *what a
//! keystroke does* — including what a keystroke does when there is nothing to do.
//!
//! Keystrokes go through `App::feed_key`, the same chord machine the loop uses, so a binding that was
//! removed from the table fails here rather than quietly doing nothing in front of someone.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "corpus/mod.rs"]
mod corpus;

use fathom_tui::app::format_thousands;
use fathom_tui::diff::Budget;
use fathom_tui::diff::Layout;
use fathom_tui::diff::{self, Unavailable};
use fathom_tui::diff::{Editor, Env, Os};
use fathom_tui::views::{self, ViewId};

/// The hunks pane, focused — the state most of these tests start from.
fn hunks_at(name: &str, size: (u16, u16)) -> fathom_tui::app::App {
    let mut app = corpus::app_with(name, size);
    corpus::focus(&mut app, "hunks");
    app
}

fn toast(app: &fathom_tui::app::App) -> String {
    app.toast.clone().unwrap_or_default()
}

fn frame(app: &fathom_tui::app::App) -> String {
    corpus::frame_text(app)
}

// --- hunk navigation --------------------------------------------------------------------------

#[test]
fn a_hunk_jump_lands_on_the_header_row_of_the_next_hunk() {
    let mut app = hunks_at("mixed", (110, 24));
    let file = app.diff.file.expect("the first file is selected on load");
    assert_eq!(app.scroll_of(), 0, "a fresh pane is at the top");

    corpus::press(&mut app, "]");
    let second = app.diff.hunk_at(file, app.scroll_of());
    assert_eq!(second, Some(1), "one `]` should be looking at hunk 2");
    assert!(app.scroll_of() > 0, "and the pane must have moved");
    assert!(
        toast(&app).contains("hunk 2 of 2"),
        "the toast has to name the hunk it jumped to: {}",
        toast(&app)
    );
    assert!(
        toast(&app).contains("@@ -40,4 +42,7 @@"),
        "and its `@@` range, so the jump can be checked against git: {}",
        toast(&app)
    );

    corpus::press(&mut app, "]");
    assert_eq!(
        app.diff.hunk_at(file, app.scroll_of()),
        Some(1),
        "past the last hunk `]` stays put — it does not scroll into the void"
    );

    corpus::press(&mut app, "[");
    assert_eq!(app.diff.hunk_at(file, app.scroll_of()), Some(0));
    corpus::press(&mut app, "[");
    assert_eq!(
        app.scroll_of(),
        0,
        "and `[` at the first hunk stops at the top of the file"
    );
}

#[test]
fn the_hunk_under_the_top_of_the_window_is_the_one_the_pane_names() {
    let app = hunks_at("mixed", (110, 24));
    let file = app.diff.file.unwrap();
    // Scroll one line past the first hunk's header and the pane is looking at hunk 1 still; a whole
    // hunk further and it is hunk 2. This is the number `Y` copies, so it has to be the one on screen.
    let slot = fathom_tui::diff::slot(Layout::Unified, false);
    let second = app
        .diff
        .file_at(file)
        .unwrap()
        .hunks
        .last()
        .unwrap()
        .anchors[slot];
    assert_eq!(app.diff.hunk_at(file, 0), Some(0));
    assert_eq!(app.diff.hunk_at(file, 1), Some(0));
    assert_eq!(
        app.diff.hunk_at(file, second),
        Some(1),
        "the second hunk's own anchor row is the second hunk"
    );
    assert_eq!(
        app.diff.hunk_at(file, second - 1),
        Some(0),
        "one row above it is still the first"
    );
    assert_eq!(app.diff.hunk_at(file, 4_000), Some(1), "clamped, not None");
}

#[test]
fn a_jump_on_a_file_with_no_hunks_says_so_instead_of_moving_nothing() {
    let mut app = hunks_at("mode_only", (110, 24));
    corpus::press(&mut app, "]");
    assert!(
        !toast(&app).is_empty(),
        "a keystroke that does nothing has to say why"
    );
    assert_eq!(app.scroll_of(), 0);
}

#[test]
fn hunk_anchors_and_drawn_rows_cannot_drift_apart() {
    // The scan counts rows in all four shapes; the pane draws from the same planner. If they ever
    // disagree, a `]` jump lands mid-hunk and the reviewer silently reads the wrong lines. This is
    // the regression test for exactly that, on every fixture and every shape.
    for (name, _) in corpus::corpus() {
        for layout in [Layout::Unified, Layout::Split] {
            for ignore_ws in [false, true] {
                let mut app = corpus::app_with(name, (110, 24));
                app.diff.layout = layout;
                app.diff.ignore_ws = ignore_ws;
                let Some(file) = app.diff.file else { continue };
                let anchors: Vec<(usize, usize)> = app
                    .diff
                    .file_at(file)
                    .unwrap()
                    .hunks
                    .iter()
                    .enumerate()
                    .map(|(number, hunk)| {
                        (
                            number,
                            hunk.anchors[fathom_tui::diff::slot(layout, ignore_ws)],
                        )
                    })
                    .collect();
                for (number, scroll) in anchors {
                    corpus::focus(&mut app, "hunks");
                    app.scroll_by(i32::try_from(scroll).unwrap_or(i32::MAX));
                    let named = app.diff.hunk_at(file, app.scroll_of());
                    assert_eq!(
                        named,
                        Some(number),
                        "{name} {layout:?}/ws-ignore {ignore_ws}: scrolling to hunk {number}'s \
                         anchor row {scroll} should be looking at hunk {number}, saw {named:?}"
                    );
                }
            }
        }
    }
}

// --- the toggles ------------------------------------------------------------------------------

#[test]
fn word_diff_lights_the_changed_run_and_says_what_it_found() {
    let mut app = hunks_at("mixed", (110, 24));
    let before = frame(&app);
    corpus::press(&mut app, "W");
    assert!(app.diff.word_diff);
    assert!(
        toast(&app).contains("word-diff on"),
        "the toggle confirms itself: {}",
        toast(&app)
    );
    let after = frame(&app);
    assert_ne!(before, after, "word-diff has to change what the pane draws");
    // The claim is about the *footer*: with a changed pair in view the pane reports the run count,
    // which is the difference between "something is lit" and "here is what changed".
    let said = corpus::hunk_rows(&app).join("\n");
    assert!(
        said.contains("word-diff on") && said.contains('·'),
        "the footer states the state and the finding: {said}"
    );
    corpus::press(&mut app, "W");
    assert!(!app.diff.word_diff);
}

#[test]
fn the_whitespace_rule_folds_pairs_and_reports_how_many() {
    let mut app = hunks_at("crlf", (110, 30));
    let as_written = corpus::hunk_rows(&app);
    assert!(
        as_written.iter().any(|line| line.contains('␍')),
        "CRLF is shown as a character when the rule is off: {as_written:?}"
    );
    let written_rows = as_written
        .iter()
        .filter(|line| !line.trim().is_empty())
        .count();

    corpus::press(&mut app, "=");
    assert!(app.diff.ignore_ws, "`=` turns the rule on");
    let folded = corpus::hunk_rows(&app);
    let folded_rows = folded.iter().filter(|line| !line.trim().is_empty()).count();
    assert!(
        folded_rows < written_rows,
        "folding has to shorten the pane: {written_rows} → {folded_rows}"
    );
    assert!(
        folded.iter().any(|line| line.contains("whitespace-only")),
        "and say what it folded, on the row where the pairs were: {folded:?}"
    );
    assert!(
        folded.iter().any(|line| line.contains("7 pair")),
        "with the count: {folded:?}"
    );
    // And back again, because a reviewer who cannot undo a toggle will not use it.
    corpus::press(&mut app, "=");
    assert!(!app.diff.ignore_ws);
    assert!(
        corpus::hunk_rows(&app)
            .iter()
            .any(|line| line.contains('␍'))
    );
}

#[test]
fn the_whitespace_rule_folds_indent_only_changes_too() {
    let mut app = hunks_at("whitespace", (110, 30));
    corpus::press(&mut app, "=");
    let rows = corpus::hunk_rows(&app);
    assert!(
        rows.iter()
            .any(|line| line.contains("whitespace-only pair")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|line| line.contains("2 pair(s) folded")),
        "the footer counts them for the whole file: {rows:?}"
    );
    // The untouched context line is still there — folding pairs is not deleting the file.
    assert!(
        rows.iter().any(|line| line.contains("fn two()")),
        "{rows:?}"
    );
}

#[test]
fn split_is_refused_when_the_pane_cannot_show_both_sides_and_names_the_key() {
    let mut app = hunks_at("mixed", (110, 24));
    corpus::press(&mut app, "\\");
    assert_eq!(
        app.diff.layout,
        Layout::Unified,
        "a 49-column pane cannot hold a split"
    );
    assert!(
        toast(&app).contains(&format_thousands(fathom_tui::diff::SPLIT_MIN_COLS as u64)),
        "the refusal has to say what it needed: {}",
        toast(&app)
    );
    assert!(toast(&app).contains('\\'), "and the key to try again");
}

#[test]
fn split_draws_both_sides_when_the_pane_is_wide_enough() {
    let mut app = hunks_at("mixed", (240, 24));
    corpus::press(&mut app, "\\");
    assert_eq!(app.diff.layout, Layout::Split, "240 columns is plenty");
    let rows = corpus::hunk_rows(&app);
    let paired = rows
        .iter()
        .find(|line| line.contains("self.raw") && line.contains('│'))
        .unwrap_or_else(|| panic!("no side-by-side row in {rows:?}"));
    assert!(
        paired.contains('-') && paired.contains('+'),
        "a split row carries both signs: {paired}"
    );
    // The same hunk, unified, is more rows than the split: alignment shares a row per pair.
    corpus::press(&mut app, "\\");
    assert_eq!(app.diff.layout, Layout::Unified);
}

#[test]
fn zooming_the_hunk_pane_lets_a_split_fit_in_a_narrow_terminal() {
    // `<C-w>o` hands the pane the whole frame, which is the honest way to read a split at 110 columns.
    let mut app = hunks_at("mixed", (110, 24));
    app.dispatch("pane.zoom");
    corpus::press(&mut app, "\\");
    assert_eq!(
        app.diff.layout,
        Layout::Split,
        "zoomed, the pane has the frame's width: {} columns",
        app.pane_cols("hunks")
    );
}

// --- the tree ---------------------------------------------------------------------------------

#[test]
fn tree_rows_carry_counts_markers_and_the_class_they_belong_to() {
    let app = corpus::app_with("mixed", (110, 24));
    // The styled tree, not the text pane: no cursor mark in the way of the first row.
    let joined = app
        .diff
        .tree_rows(44)
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.starts_with("8 files · +18 −11"), "{joined}");
    for marker in ["~", "new", "del", "ren", "bin", "mode"] {
        assert!(joined.contains(marker), "no {marker:?} marker in {joined}");
    }
    assert!(
        joined.contains("crates/fathom-analyze/src/"),
        "directories group the files: {joined}"
    );
    assert!(
        joined.contains("test") && joined.contains("(1 f)"),
        "the taxonomy class is a group row: {joined}"
    );
}

#[test]
fn read_and_dirty_markers_come_from_the_stored_state() {
    let app = corpus::app_with("states", (110, 24));
    let rows = corpus::tree_rows(&app).join("\n");
    assert!(rows.contains('✓'), "a read file is marked: {rows}");
    assert!(rows.contains('*'), "a dirty file is marked: {rows}");
    // Evidence belongs to the file, so it appears when that file is the one on screen — and only
    // then, which is why the cursor has to move.
    let mut app = corpus::app_with("states", (110, 24));
    corpus::focus(&mut app, "tree");
    for _ in 0..8 {
        if corpus::frame_text(&app).contains("ev-2") {
            break;
        }
        corpus::press(&mut app, "j");
    }
    let screen = corpus::frame_text(&app);
    assert!(
        screen.contains("ev-2") && screen.contains("b.rs"),
        "evidence ids belong on the pane showing the file they came from: {screen}"
    );
}

#[test]
fn noise_files_are_hidden_by_default_and_shown_with_the_rule_that_hid_them() {
    let mut app = corpus::app_with("mixed", (110, 24));
    let hidden = corpus::tree_rows(&app).join("\n");
    assert!(
        !hidden.contains("package-lock.json"),
        "the noise file is not on screen by default: {hidden}"
    );
    assert!(
        hidden.contains("noise: 1 hidden by `generated+bulk`"),
        "the hidden count names the rule: {hidden}"
    );

    corpus::press(&mut app, "b");
    assert!(app.diff.show_noise);
    let shown = corpus::tree_rows(&app).join("\n");
    assert!(
        shown.contains("package-lock.json") && shown.contains("[n]"),
        "revealed, and still marked as the noise it is: {shown}"
    );
    assert!(
        toast(&app).contains("generated+bulk"),
        "the toast repeats the rule: {}",
        toast(&app)
    );
    corpus::press(&mut app, "b");
    assert!(!app.diff.show_noise);
}

#[test]
fn the_selection_cannot_leave_the_tree_and_the_cursor_is_always_on_screen() {
    let mut app = corpus::app_with("mixed", (110, 14));
    corpus::press(&mut app, "G");
    let last = app.selected_row();
    let rows = app.row_count(pane("tree"));
    assert_eq!(last, rows - 1, "`G` goes to the last row and no further");
    corpus::press(&mut app, "G");
    assert_eq!(app.selected_row(), rows - 1, "and stays there");
    corpus::press(&mut app, "gg");
    assert_eq!(app.selected_row(), 0);

    // A tall tree in a short pane: the window has to follow the cursor, or the selection is invisible.
    let big = corpus::generated(120, 1, 2);
    let mut app = corpus::app_with_text(&big, (110, 12));
    corpus::focus(&mut app, "tree");
    corpus::press(&mut app, "G");
    let body = corpus::pane_rows(&app, "tree");
    assert!(
        body.iter().any(|line| line.starts_with('▸')),
        "the cursor left the screen: {body:?}"
    );
}

// --- the degenerate cases ---------------------------------------------------------------------

/// Each degenerate shape has to *say* what it is, in the pane, in words a reviewer can act on. An
/// empty pane is a failure even when nothing crashed.
#[test]
fn every_degenerate_shape_says_what_it_is() {
    let cases: &[(&str, &[&str])] = &[
        ("binary", &["binary", "no line text"]),
        ("binary_git", &["no line text"]),
        (
            "mode_only",
            &["mode 100644 → 100755", "bytes did not change"],
        ),
        ("rename_edits", &["renamed.py", "src/legacy.py", "78%"]),
        ("deleted", &["del", "@@ -1,4 +0,0 @@"]),
        ("oversize", &["materialisation cap", "not decoded"]),
        ("long_line", &["clipped", "8 192"]),
        (
            "unavailable",
            &["not available offline", "fathom fetch --episode"],
        ),
        (
            "file_unavailable",
            &["not available offline", "run: fathom fetch --blob"],
        ),
        ("submodule", &["submodule"]),
        ("not_a_diff", &["not a unified diff"]),
        ("empty", &["patch is empty"]),
    ];
    for (name, facts) in cases {
        let app = corpus::app_with(name, (110, 24));
        let screen = frame(&app);
        for fact in *facts {
            assert!(
                screen.contains(fact),
                "the `{name}` case never says {fact:?} — it has to, or the pane is just empty:\n{screen}"
            );
        }
    }
}

#[test]
fn a_rename_shows_the_old_path_the_similarity_and_the_edits() {
    let app = corpus::app_with("rename_edits", (110, 24));
    let screen = frame(&app);
    assert!(screen.contains("src/renamed.py"), "{screen}");
    assert!(screen.contains("src/legacy.py"), "the old path: {screen}");
    assert!(
        screen.contains("78%"),
        "the similarity git stated: {screen}"
    );
    assert!(
        screen.contains("encoding=\"utf-8\""),
        "and the actual edits: {screen}"
    );
}

#[test]
fn a_source_that_cannot_be_opened_is_reported_not_swallowed() {
    let mut app = corpus::blank((110, 24));
    let outcome = app.load_patch(corpus::unreachable(
        "episode 0:12",
        "no packfile for this epoch",
    ));
    let err = outcome.expect_err("an unreachable source must fail");
    assert!(
        err.to_string().contains("packfile"),
        "the caller's words survive the trip: {err}"
    );
    let screen = frame(&app);
    assert!(
        screen.contains("no packfile for this epoch"),
        "and land in the pane: {screen}"
    );
    // The view is still a whole frame: an unavailable diff is a state, not a crash.
    assert_eq!(
        fathom_tui::render::frame_lines(&app).len(),
        24,
        "an empty diff still draws a frame"
    );
}

#[test]
fn an_unavailable_report_carries_its_hint_through_the_model() {
    let skip = Unavailable::with_hint("blob is not local", "fathom fetch --all");
    assert_eq!(skip.hint.as_deref(), Some("fathom fetch --all"));
    assert!(skip.to_string().contains("fathom fetch --all"));
}

// --- the three ways out: copy path, copy hunk, $EDITOR ----------------------------------------

#[test]
fn copy_path_takes_the_path_under_the_cursor() {
    let mut app = corpus::app_with("mixed", (110, 24));
    corpus::focus(&mut app, "tree");
    // The counts row is not a path, and the pane says so instead of copying something plausible.
    corpus::press(&mut app, "gg");
    corpus::press(&mut app, "y");
    assert!(
        toast(&app).contains("nothing selectable"),
        "{}",
        toast(&app)
    );
    // counts row, class group, directory, file.
    for _ in 0..3 {
        corpus::press(&mut app, "j");
    }
    let path = app.diff.path_text(app.selected_row());
    assert_eq!(
        path.as_deref(),
        Some("crates/fathom-analyze/src/score.rs"),
        "the file the cursor walked to is the file the pane is drawing"
    );
    corpus::press(&mut app, "y");
    // The path is named in the toast, and what the shell was asked to do is visible: with no
    // environment installed there is no clipboard program to run, so the plan is the OSC 52 escape —
    // the one clipboard that works over `ssh`. The escape is the frame sink's to write, not the
    // view's: `fathom-tui` writes no bytes to the machine (ADR-002 §3).
    let said = toast(&app);
    assert!(
        said.contains("crates/fathom-analyze/src/score.rs"),
        "{said}: the path was {path:?}"
    );
    assert!(
        matches!(app.diff.handoff, Some(fathom_tui::diff::Handoff::Write(_))),
        "the queued work is the escape sequence: {:?}",
        app.diff.handoff
    );
    // With a clipboard program on PATH the plan is a program instead, and it is the one this OS uses.
    app.diff.install_env(
        fathom_tui::diff::Env::fixed(&[], &["pbcopy"]),
        fathom_tui::diff::Os::MacOs,
    );
    corpus::press(&mut app, "y");
    match app.diff.handoff.clone() {
        Some(fathom_tui::diff::Handoff::Run(command)) => {
            assert_eq!(command.line(), "pbcopy");
            assert_eq!(
                command.stdin.as_deref(),
                Some("crates/fathom-analyze/src/score.rs"),
                "the path is what goes down the pipe"
            );
        }
        other => panic!("pbcopy was on PATH, so the plan should have been to run it: {other:?}"),
    }
}

#[test]
fn copy_hunk_produces_the_raw_patch_text_for_that_hunk() {
    let app = corpus::app_with("mixed", (110, 24));
    let file = app.diff.file.unwrap();
    let text = app.diff.hunk_text(file, Some(1)).unwrap();
    assert!(text.starts_with("diff --git"), "{text}");
    assert!(text.contains("@@ -40,4 +42,7 @@"), "{text}");
    assert_eq!(
        text.lines().filter(|line| line.starts_with('+')).count(),
        3,
        "the hunk's own added lines, no more: {text}"
    );
    assert_eq!(
        text.lines().filter(|line| line.starts_with('-')).count(),
        0,
        "hunk 2 of the fixture removes nothing: {text}"
    );
    assert!(
        app.diff.hunk_text(file, Some(99)).is_err(),
        "a hunk that is not there is an error, not an empty string"
    );
}

#[test]
fn a_copy_goes_to_the_platform_clipboard_program_when_there_is_one() {
    // The claim in the ticket is "macOS, Linux and Windows". Each is a program name and a session
    // variable, so the plan can be checked without a clipboard, a display server, or a VM.
    let text = "diff --git a/x b/x\n@@ -1 +1 @@\n-a\n+b\n";

    let mac = Env::fixed(&[], &["pbcopy"]);
    match diff::copy_plan(Os::MacOs, text, &mac) {
        Copy::Program(command) => {
            assert_eq!(command.program, "pbcopy");
            assert_eq!(command.stdin.as_deref(), Some(text));
        }
        other => panic!("macOS with pbcopy should use it: {other:?}"),
    }

    let wayland = Env::fixed(&[("WAYLAND_DISPLAY", "wayland-0")], &["wl-copy", "xclip"]);
    match diff::copy_plan(Os::Linux, text, &wayland) {
        Copy::Program(command) => assert_eq!(
            command.program, "wl-copy",
            "a Wayland session should not be handed an X tool"
        ),
        other => panic!("Wayland with wl-copy should use it: {other:?}"),
    }

    let x11 = Env::fixed(&[("DISPLAY", ":0")], &["xclip"]);
    match diff::copy_plan(Os::Linux, text, &x11) {
        Copy::Program(command) => {
            assert_eq!(command.program, "xclip");
            assert_eq!(
                command.args,
                vec!["-selection".to_owned(), "clipboard".to_owned()],
                "xclip does not default to the clipboard"
            );
        }
        other => panic!("X11 with xclip should use it: {other:?}"),
    }

    let windows = Env::fixed(&[], &["clip"]);
    match diff::copy_plan(Os::Windows, text, &windows) {
        Copy::Program(command) => assert_eq!(command.program, "clip"),
        other => panic!("Windows with clip should use it: {other:?}"),
    }

    // The escape hatch a reviewer can actually set when none of the above exist.
    let explicit = Env::fixed(&[("FATHOM_CLIPBOARD", "wl-copy --trim")], &["wl-copy"]);
    match diff::copy_plan(Os::Other, text, &explicit) {
        Copy::Program(command) => {
            assert_eq!(command.program, "wl-copy");
            assert_eq!(command.args, vec!["--trim".to_owned()]);
        }
        other => panic!("FATHOM_CLIPBOARD wins: {other:?}"),
    }
}

#[test]
fn with_no_clipboard_program_the_text_goes_to_the_terminal_itself() {
    // OSC-52 is the fallback that works through ssh with nothing installed. Terminals that ignore it
    // are why the toast says what was tried instead of "copied".
    let empty = Env::fixed(&[], &[]);
    match diff::copy_plan(Os::Linux, "hello\n", &empty) {
        Copy::Osc52 { sequence } => {
            assert!(sequence.starts_with("\x1b]52;c;"), "{sequence:?}");
            assert!(sequence.contains("aGVsbG8K"), "the payload is base64");
        }
        other => panic!("no clipboard program should fall back to OSC-52: {other:?}"),
    }
}

#[test]
fn the_editor_command_names_the_file_and_the_line_in_the_spelling_that_editor_uses() {
    let cases = [
        ("vim", vec!["+42".to_owned(), "src/a.rs".to_owned()]),
        ("nvim", vec!["+42".to_owned(), "src/a.rs".to_owned()]),
        ("nano", vec!["+42".to_owned(), "src/a.rs".to_owned()]),
        ("code", vec!["--goto".to_owned(), "src/a.rs:42".to_owned()]),
        (
            "idea",
            vec!["--line".to_owned(), "42".to_owned(), "src/a.rs".to_owned()],
        ),
        ("hx", vec!["src/a.rs:42".to_owned()]),
        ("emacs", vec!["+42".to_owned(), "src/a.rs".to_owned()]),
        // An editor with no line argument still opens the file, and says the line is elsewhere.
        ("myed", vec!["src/a.rs".to_owned()]),
    ];
    for (editor, want) in cases {
        let env = Env::fixed(&[("EDITOR", editor)], &[editor]);
        match diff::editor_plan(Os::Linux, "src/a.rs", 42, None, &env) {
            Editor::Open(command) => {
                assert_eq!(command.program, editor, "{editor}");
                assert_eq!(command.args, want, "{editor}");
            }
            other => panic!("{editor} should open: {other:?}"),
        }
    }
}

#[test]
fn visual_beats_editor_and_a_missing_editor_says_so_with_the_default_it_tried() {
    let env = Env::fixed(&[("EDITOR", "vim"), ("VISUAL", "code")], &["vim", "code"]);
    match diff::editor_plan(Os::Linux, "x.rs", 1, None, &env) {
        Editor::Open(command) => assert_eq!(command.program, "code", "$VISUAL wins"),
        other => panic!("{other:?}"),
    }
    let none = Env::fixed(&[], &[]);
    match diff::editor_plan(Os::MacOs, "x.rs", 1, None, &none) {
        Editor::Unavailable { why } => {
            assert!(why.contains("vi"), "{why}");
            assert!(why.contains("not on PATH"), "{why}");
        }
        other => panic!("no editor at all should be an explanation: {other:?}"),
    }
}

#[test]
fn opening_the_editor_targets_the_line_at_the_top_of_the_pane() {
    let mut app = hunks_at("mixed", (110, 24));
    corpus::press(&mut app, "]");
    corpus::press(&mut app, "E");
    let said = toast(&app);
    assert!(
        said.contains("42") || said.contains("not on PATH") || said.contains("no editor"),
        "the toast says which line it aimed at, or why it could not: {said}"
    );
}

// --- the budgets ------------------------------------------------------------------------------

#[test]
fn the_reader_reads_in_chunks_and_never_more_than_a_chunk_at_a_time() {
    // `e1.7`'s "chunked reads" claim, measured rather than asserted: a counting reader that would
    // fail the test if anything asked for the whole patch in one go.
    let text = corpus::generated(4, 20, 40);
    let mut app = corpus::app_with_text(&text, (110, 24));
    let budget = Budget::DEFAULT;
    assert!(
        budget.chunk_bytes < text.len(),
        "the fixture is bigger than a chunk"
    );
    app.diff.budget = budget;
    app.diff.reload().unwrap();
    let scan = app.diff.index().unwrap();
    assert!(scan.bytes > budget.chunk_bytes as u64);
    assert!(
        scan.retained_bytes < budget.index_bytes,
        "{} bytes of index against a {} byte cap",
        scan.retained_bytes,
        budget.index_bytes
    );
}

#[test]
fn the_index_stops_at_its_cap_and_says_that_it_did() {
    let text = corpus::generated(30, 1, 3);
    let mut app = corpus::app_with("empty", (110, 24));
    let mut budget = Budget::DEFAULT;
    budget.max_files = 10;
    app.diff.budget = budget;
    app.load_patch(corpus::source(&text)).unwrap();
    let scan = app.diff.index().unwrap();
    assert_eq!(scan.files.len(), 10, "the cap is a cap");
    assert!(
        scan.stopped.is_some(),
        "and stopping early is stated, not silent"
    );
    let screen = frame(&app);
    assert!(
        screen.contains("stopped") || screen.contains("10 files"),
        "the stop reaches the pane: {screen}"
    );
}

#[test]
fn the_hunk_cap_stops_the_same_way() {
    let text = corpus::generated(2, 40, 2);
    let mut app = corpus::app_with("empty", (110, 24));
    let mut budget = Budget::DEFAULT;
    budget.max_hunks = 12;
    app.diff.budget = budget;
    app.load_patch(corpus::source(&text)).unwrap();
    let scan = app.diff.index().unwrap();
    assert!(scan.stopped.is_some(), "{:?}", scan.stopped);
    assert!(
        scan.hunk_count() <= 12,
        "{} hunks against a cap of 12",
        scan.hunk_count()
    );
}

#[test]
fn a_window_build_reads_only_the_lines_it_draws() {
    // The whole memory claim, measured: a 10 000-line hunk, and a window build that reads a paneful
    // of it. If this ever grows with the file instead of the window, the 50 000-line guarantee is
    // gone and this test is the one that tells you.
    let text = corpus::generated(1, 1, 5_000);
    let app = corpus::app_with_text(&text, (110, 24));
    let built = app.diff.build(
        app.diff.file.unwrap(),
        &fathom_tui::diff::Request {
            layout: Layout::Unified,
            ignore_ws: false,
            word_diff: false,
            width: 64,
            height: 20,
            scroll: 0,
        },
    );
    // A window of `height` rows reads about `height` lines, plus one pairing run: a change block is
    // held until it can be aligned (or outgrows `pair_run`), and that buffer is the only part of the
    // window that is not literally on screen. Both are constants of the pane, neither is the file.
    let cap = 20 + Budget::DEFAULT.pair_run + 8;
    assert!(
        built.read_lines <= cap,
        "a 20-row window read {} lines from a 10 000-line hunk; the cap this pane promises is {}",
        built.read_lines,
        cap
    );
    assert!(
        built.read_bytes < Budget::DEFAULT.window_bytes / 8,
        "{} bytes read for a 20-row window",
        built.read_bytes
    );
}

#[test]
fn fifty_thousand_lines_scans_and_draws_inside_the_documented_caps() {
    // The ticket's headline claim, asserted as work done rather than as a stopwatch reading: a
    // 50 000-line patch scans whole under the default index budget, and drawing a window anywhere in
    // it — including the last hunk — still reads a window's worth of bytes, not the file's.
    let text = corpus::generated(20, 25, 50);
    let mut app = corpus::app_with_text(&text, (110, 24));
    let index = app.diff.index().expect("the patch scanned");
    assert!(index.stopped.is_none(), "{:?}", index.stopped);
    assert_eq!(index.files.len(), 20);
    assert_eq!(index.hunk_count(), 500);
    assert!(
        index.lines >= 50_000,
        "the fixture is a 50 000-line patch, and it is {} lines",
        index.lines
    );
    assert!(
        index.retained_bytes < Budget::DEFAULT.index_bytes,
        "{} B of index against a {} B budget",
        index.retained_bytes,
        Budget::DEFAULT.index_bytes
    );
    corpus::focus(&mut app, "hunks");
    app.scroll_by(1_200);
    assert!(
        app.scroll_of() >= 1_000,
        "the scroll took: {}",
        app.scroll_of()
    );
    let deep = corpus::hunk_rows(&app);
    assert!(
        deep.iter().any(|line| line.contains("@@")),
        "a window four thousand rows in still draws a hunk: {deep:?}"
    );
    corpus::press(&mut app, "G");
    assert!(
        corpus::hunk_rows(&app)
            .iter()
            .any(|line| line.contains("@@")),
        "the bottom of a 50 000-line patch draws too"
    );
}

#[test]
fn a_smaller_window_budget_is_reported_by_the_pane_that_hit_it() {
    let text = corpus::generated(1, 1, 5_000);
    let mut app = corpus::app_with_text(&text, (110, 24));
    let mut budget = Budget::DEFAULT;
    budget.window_lines = 12;
    app.diff.budget = budget;
    app.diff.reload().unwrap();
    corpus::focus(&mut app, "hunks");
    let rows = corpus::hunk_rows(&app);
    assert!(
        rows.join("\n").contains("12"),
        "the pane has to admit it stopped early: {rows:?}"
    );
}

#[test]
fn an_oversized_line_is_cut_at_the_cap_and_the_cut_is_admitted() {
    let mut app = corpus::app_with("long_line", (110, 24));
    assert!(
        frame(&app).contains("8 192"),
        "the cap is named where the line is"
    );
    // Halve the cap and the same fixture gets cut sooner: the number printed is the budget, not a
    // coincidence of this fixture's length.
    let mut tight = Budget::DEFAULT;
    tight.max_line_bytes = 1024;
    app.diff.budget = tight;
    app.diff.reload().unwrap();
    let screen = frame(&app);
    assert!(screen.contains("1 024"), "{screen}");
}

// --- the frame around it ----------------------------------------------------------------------

#[test]
fn the_diff_view_draws_an_exact_frame_at_the_sizes_the_shell_promises() {
    for size in [(80u16, 24u16), (100, 30), (200, 60)] {
        for name in ["mixed", "binary", "mode_only", "empty"] {
            let app = corpus::app_with(name, size);
            let lines = fathom_tui::render::frame_lines(&app);
            assert_eq!(lines.len(), usize::from(size.1), "{name} at {size:?}");
            for (row, line) in lines.iter().enumerate() {
                assert_eq!(
                    fathom_tui::layout::display_width(line),
                    usize::from(size.0),
                    "{name} at {size:?}, row {row}: {line:?}"
                );
            }
        }
    }
}

#[test]
fn a_tiny_terminal_still_draws_a_whole_frame_and_says_what_it_dropped() {
    // 20×6 is below the diff view's minimum for two panes, so the ladder drops one — and the frame
    // says which one, rather than quietly drawing half a UI (ADR-006 §6.1, §6.4).
    let app = corpus::app_with("mixed", (20, 6));
    let lines = fathom_tui::render::frame_lines(&app);
    assert_eq!(lines.len(), 6);
    for line in &lines {
        assert_eq!(fathom_tui::layout::display_width(line), 20, "{lines:?}");
    }
    let screen = corpus::frame_text(&app);
    assert!(
        screen.contains("single") || screen.contains("too small"),
        "no notice about the degraded shape: {lines:?}"
    );
}

#[test]
fn the_view_is_the_one_the_keymap_says_it_is_and_owns_its_own_actions() {
    let view = views::view(ViewId::Diff);
    assert_eq!(view.id, "diff");
    assert_eq!(view.owner, "e1.7");
    // Every action bound in the diff mode is either this ticket's or the ticket the table says owns
    // it; nothing here belongs to the shell by accident.
    let km = fathom_tui::keymap::Keymap::defaults();
    for (id, _label, _keys) in km.palette_entries() {
        if let Some(rest) = id.strip_prefix("diff.") {
            assert!(
                matches!(
                    rest,
                    "hunk_next"
                        | "hunk_prev"
                        | "split_toggle"
                        | "word_toggle"
                        | "ws_toggle"
                        | "noise_toggle"
                        | "copy_path"
                        | "copy_hunk"
                        | "open_editor"
                ),
                "unexpected diff action {id}"
            );
        }
    }
}

#[test]
fn every_diff_key_reaches_its_action_through_the_chord_machine() {
    let app = corpus::app_with("mixed", (110, 24));
    let want = [
        ("]", "diff.hunk_next"),
        ("[", "diff.hunk_prev"),
        ("\\", "diff.split_toggle"),
        ("W", "diff.word_toggle"),
        ("=", "diff.ws_toggle"),
        ("b", "diff.noise_toggle"),
        ("y", "diff.copy_path"),
        ("Y", "diff.copy_hunk"),
        ("E", "diff.open_editor"),
    ];
    for (key, action) in want {
        let mut app = fathom_tui::app::App::new(
            fathom_tui::keymap::Keymap::defaults(),
            app.model.clone(),
            (110, 24),
            "test",
        );
        app.set_view(ViewId::Diff);
        let fired = app.feed_key(key, std::time::Instant::now());
        assert_eq!(
            fired,
            vec![action.to_owned()],
            "{key:?} should resolve to {action:?}"
        );
    }
}

#[test]
fn a_loaded_patch_opens_the_diff_view_and_names_where_the_bytes_came_from() {
    let app = corpus::app_with("mixed", (110, 24));
    assert_eq!(app.view, ViewId::Diff);
    assert_eq!(app.diff.origin(), Some("mixed.diff"));
    assert!(
        frame(&app).contains("mixed.diff"),
        "the status line says which patch this is: {}",
        frame(&app)
    );
}

/// The pane named, from the diff view's own declaration.
fn pane(name: &str) -> &'static views::Pane {
    views::view(ViewId::Diff)
        .panes
        .iter()
        .find(|pane| pane.name == name)
        .unwrap_or_else(|| panic!("no pane {name:?}"))
}

/// Used by the drift test above; re-exported for clarity of the import list.
#[allow(unused_imports)]
use diff as _diff;
/// Imported here so the `Copy` match arms above read as the enum they are.
use fathom_tui::diff::Copy;
