//! Golden output for the diff viewer (`e1.7`, AC 2).
//!
//! One golden file per case, and a case is a *shape*: the ordinary patch, and every degenerate shape
//! the ticket names — binary, mode-only, rename-with-edits, deleted, oversize, not-available-offline —
//! plus CRLF churn and whitespace-only churn. Each golden holds both panes, drawn at fixed widths, so
//! a change to the shell's layout ladder cannot rewrite a diff golden and a change to the diff viewer
//! cannot hide inside a different terminal size.
//!
//! The text channel is what is pinned, not the styles. That is deliberate (ADR-006 §10.2): every fact
//! this pane carries has to be readable with colour switched off, so the text *is* the contract, and a
//! style-only change should not need a re-bake.
//!
//! Re-bake after an intentional change with:
//!
//! ```text
//! FATHOM_GOLDEN=write cargo test -p fathom-tui --test diff_golden
//! ```
//!
//! …and then actually read the diff. A golden that nobody re-read after re-baking has stopped being a
//! test; it is a cache.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

#[path = "corpus/mod.rs"]
mod corpus;

use fathom_tui::diff::Layout;
use fathom_tui::views::{self, ViewId};

/// The tree pane's golden width: narrow enough that path clipping is exercised in every case.
const TREE_COLS: usize = 44;
/// The hunk pane's golden width, and the number of body rows. Wide enough for a real line of code,
/// short enough that a footer or a statement row that runs over is caught.
const HUNK_COLS: usize = 64;
const HUNK_ROWS: usize = 16;
/// The frame golden's size: the ADR's own "wide" case, where the split is allowed to exist.
const FRAME: (u16, u16) = (108, 24);

/// One case: what to load, where the cursor starts, and what the pane is showing.
struct Case {
    /// The fixture to load.
    fixture: &'static str,
    /// The golden file's stem, so two cases can share a fixture and stay two goldens.
    golden: &'static str,
    /// Which tree row the cursor starts on, clamped by the tree itself.
    select: usize,
    layout: Layout,
    ignore_ws: bool,
    word_diff: bool,
    /// Body scroll, so a golden can look at the middle of a file and not only its top.
    scroll: usize,
    /// Why this case is in the set — printed in the golden's header comment.
    note: &'static str,
}

const CASES: &[Case] = &[
    Case {
        fixture: "mixed",
        golden: "mixed",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "the ordinary review: two classes, a rename, a mode change, a binary file, noise",
    },
    Case {
        fixture: "binary",
        golden: "binary",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "binary: no line text, and the size the source stated",
    },
    Case {
        fixture: "mode_only",
        golden: "mode_only",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "mode-only change: the bytes did not move, and the pane says so",
    },
    Case {
        fixture: "rename_edits",
        golden: "rename_edits",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "rename with edits: old path, new path, similarity, and the edits",
    },
    Case {
        fixture: "deleted",
        golden: "deleted",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "deleted file: every line removed, no new side",
    },
    Case {
        fixture: "oversize",
        golden: "oversize",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "over the materialisation cap: the file is listed, its text is not decoded",
    },
    Case {
        fixture: "long_line",
        golden: "long_line",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "one line over the per-line cap: clipped, and the clip is admitted",
    },
    Case {
        fixture: "crlf",
        golden: "crlf",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "CRLF churn, as written: every line changed, and the marker says why",
    },
    Case {
        fixture: "crlf",
        golden: "crlf_ws_ignored",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: true,
        word_diff: false,
        scroll: 0,
        note: "the same churn with whitespace ignored: folded, counted, and still stated",
    },
    Case {
        fixture: "whitespace",
        golden: "whitespace",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: true,
        scroll: 0,
        note: "indent-only churn with word-diff on: the change run inside the line is what is lit",
    },
    Case {
        fixture: "mixed",
        golden: "mixed_split",
        select: 0,
        layout: Layout::Split,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "split: old on the left, new on the right, inside one pane",
    },
    Case {
        fixture: "mixed",
        golden: "mixed_select3",
        select: 3,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "the cursor on a file rather than the counts row",
    },
    Case {
        fixture: "file_unavailable",
        golden: "file_unavailable",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "one file's text is not in the local object store; the other one still reads",
    },
    Case {
        fixture: "unavailable",
        golden: "unavailable",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "the whole patch is not available offline, with the command that would fetch it",
    },
    Case {
        fixture: "not_a_diff",
        golden: "not_a_diff",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "somebody pointed the viewer at a file that is not a patch",
    },
    Case {
        fixture: "states",
        golden: "states",
        select: 0,
        layout: Layout::Unified,
        ignore_ws: false,
        word_diff: false,
        scroll: 0,
        note: "read / dirty / unread markers and evidence ids, from the store's headers",
    },
];

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("diff")
}

fn blessing() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

/// Draw one case into the text a golden file holds.
fn render(case: &Case) -> String {
    let mut app = corpus::app_with(case.fixture, FRAME);
    app.diff.layout = case.layout;
    app.diff.ignore_ws = case.ignore_ws;
    app.diff.word_diff = case.word_diff;
    app.set_selection(case.select);
    let panes = views::view(ViewId::Diff);
    let tree = panes
        .panes
        .iter()
        .find(|pane| pane.name == "tree")
        .expect("the diff view has a tree pane");
    let hunks = panes
        .panes
        .iter()
        .find(|pane| pane.name == "hunks")
        .expect("the diff view has a hunks pane");

    if case.scroll > 0 {
        app.scroll_by(i32::try_from(case.scroll).unwrap_or(i32::MAX));
    }
    let mut out = String::new();
    out.push_str(&format!("# {} — {}\n", case.fixture, case.note));
    out.push_str(&format!(
        "# layout {} · whitespace {} · word-diff {} · cursor row {} · scroll {}\n\n",
        case.layout.name(),
        if case.ignore_ws {
            "ignored"
        } else {
            "as written"
        },
        if case.word_diff { "on" } else { "off" },
        case.select,
        case.scroll
    ));
    out.push_str(&format!("## tree ({TREE_COLS} columns)\n"));
    for line in app.diff_text(tree).unwrap_or_default() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str(&format!(
        "\n## hunks ({HUNK_COLS} columns × {HUNK_ROWS} rows)\n"
    ));
    let body = app
        .diff_body(hunks, HUNK_COLS, HUNK_ROWS)
        .expect("the diff view draws a body");
    for line in &body {
        let text = line.text();
        assert_eq!(
            fathom_tui::layout::display_width(text),
            HUNK_COLS,
            "a hunk row must be exactly the pane's width, or the pane rule lands in the wrong column"
        );
        out.push_str(text.trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn every_shape_matches_its_golden_file() {
    let dir = golden_dir();
    if blessing() {
        std::fs::create_dir_all(&dir).expect("the golden directory can be created");
    }
    let mut missing = Vec::new();
    for case in CASES {
        let want_path = dir.join(format!("{}.txt", case.golden));
        let got = render(case);
        if blessing() {
            std::fs::write(&want_path, &got).expect("the golden file can be written");
            continue;
        }
        let want = match std::fs::read_to_string(&want_path) {
            Ok(text) => text,
            Err(err) => {
                missing.push(format!("{} ({err})", want_path.display()));
                continue;
            }
        };
        assert_eq!(
            want,
            got,
            "{} drifted. If the change is intended, re-bake with FATHOM_GOLDEN=write and read \
             what changed.",
            want_path.display()
        );
    }
    assert!(
        missing.is_empty(),
        "no golden file at: {} — generate with FATHOM_GOLDEN=write cargo test -p fathom-tui \
         --test diff_golden",
        missing.join(", ")
    );
}

/// The whole frame, once: the panes inside the shell's chrome, at the ADR's wide size.
///
/// This is the golden that catches a composition mistake — a strip that ran over, a rule in the wrong
/// column, a status line that swallowed a body row — none of which a pane golden can see.
#[test]
fn the_frame_itself_matches_its_golden_file() {
    let path = golden_dir().join("frame_mixed.txt");
    let mut app = corpus::app_with("mixed", FRAME);
    app.dispatch("pane.focus_next");
    app.dispatch("diff.hunk_next");
    let got = fathom_tui::render::frame_lines(&app).join("\n") + "\n";
    for (row, line) in fathom_tui::render::frame_lines(&app).iter().enumerate() {
        assert_eq!(
            fathom_tui::layout::display_width(line),
            usize::from(FRAME.0),
            "frame row {row} is not {} cells: {line:?}",
            FRAME.0
        );
    }
    if blessing() {
        std::fs::create_dir_all(golden_dir()).expect("the golden directory can be created");
        std::fs::write(&path, &got).expect("the frame golden can be written");
    }
    let want =
        std::fs::read_to_string(&path).expect("the frame golden exists (FATHOM_GOLDEN=write)");
    assert_eq!(want, got, "{} drifted", path.display());
}

/// The text channel and the styled channel must draw the same characters.
///
/// The styled frame is what a terminal gets; the text one is what `--plain`, the golden files and a
/// screen reader get. They are built by different code paths in `render`, so the agreement is worth
/// asserting rather than hoping: a fact that exists only in a colour is exactly what ADR-006 §10.2
/// forbids, and this is the cheapest place that can tell the difference.
#[test]
fn the_styled_and_text_channels_draw_the_same_characters() {
    for (name, _) in corpus::corpus() {
        let app = corpus::app_with(name, FRAME);
        let text = fathom_tui::render::frame_lines(&app);
        let styled: Vec<String> = fathom_tui::render::frame_styled(&app)
            .iter()
            .map(|line| line.text().to_owned())
            .collect();
        assert_eq!(
            text, styled,
            "{name}: the plain frame and the styled frame disagree about what the screen says"
        );
        assert_eq!(text.len(), usize::from(FRAME.1), "{name}: not a full frame");
    }
}

/// Every marker the tree draws is a character, not a colour.
///
/// The ADR's rule (§10.2) is that colour never carries a fact alone, so the facts a reviewer needs must
/// be present in the plain frame: the counts, what happened to the file, and what the pane is hiding.
#[test]
fn the_tree_carries_its_facts_without_colour() {
    let app = corpus::app_with("states", FRAME);
    let frame = fathom_tui::render::frame_lines(&app).join("\n");
    assert!(frame.contains("3 files"), "no counts row: {frame}");
    assert!(frame.contains("+1 −1"), "no per-file counts: {frame}");
    assert!(
        frame.contains('✓') || frame.contains('*'),
        "no read/dirty marker"
    );

    let app = corpus::app_with("mixed", FRAME);
    let frame = fathom_tui::render::frame_lines(&app).join("\n");
    for fact in ["bin", "del", "ren", "mode 00644→00755", "new", "noise"] {
        assert!(frame.contains(fact), "the plain frame loses {fact:?}");
    }
}
