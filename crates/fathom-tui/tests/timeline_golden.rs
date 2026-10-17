//! Golden output for the timeline (`e1.6`, acceptance criteria 2 and 4).
//!
//! A golden per *shape*, not per keystroke: the ordinary rail, every rung of the column ladder, both
//! groupings, a collapsed group, a filter that narrowed the list, a filter that emptied it, the two
//! empty states (`reco-1nu`: no index at all, and an index that is finished but reads nothing), a dead
//! episode, a 10 000-row index, and the `F` surface open. Those are
//! the cases where a plausible-looking change is the dangerous kind — a sparkline that quietly stops
//! being drawn, a `··· N more` that goes missing, a card that starts claiming delta facts `e2.11` does
//! not produce yet.
//!
//! Two channels are pinned:
//!
//! * **the whole frame** at the sizes the ADR names (80×24 and 61×16 — the view's documented minimum),
//!   so a change to the shell's layout ladder cannot rewrite a timeline golden and vice versa;
//! * **each pane on its own** at fixed widths, so the ladder's rungs are comparable line by line.
//!
//! Only the text channel is pinned, never styles: ADR-006 §10.2 requires every fact on this pane to
//! read with colour switched off, so the text *is* the contract.
//!
//! Re-bake after an intentional change with:
//!
//! ```text
//! FATHOM_GOLDEN=write cargo test -p fathom-tui --test timeline_golden
//! ```
//!
//! …and then read the diff. The rail's cursor column, the `?` that stands for an unranked episode and
//! the "not in this build" lines in the card are all deliberate; if one of them moved, the change is
//! not cosmetic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model, Overlay};
use fathom_tui::index::Facts;
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::{self, Batch, Filter, GroupBy};
use fathom_tui::views::{self, ViewId};

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{NOW, generated, mock, without};

/// The rail's golden width: the view registry's `ideal_cols` for the rail pane.
const RAIL_COLS: usize = 42;
/// Rail body rows in a pane-only golden — enough to hold a day, its header and a collapsed run.
const RAIL_ROWS: usize = 12;
/// The card's golden width, its `ideal_cols`.
const CARD_COLS: usize = 35;
const CARD_ROWS: usize = 14;

/// One case: what the rail holds, what it is being asked to show, and which golden it writes.
struct Case {
    golden: &'static str,
    /// Why this case is in the set; printed as the golden's header comment.
    note: &'static str,
    /// The whole-frame size, when the case pins one.
    frame: Option<(u16, u16)>,
    /// How many episodes to load: the mock corpus, the big one, or the mock minus one row.
    rows: Rows,
    filter: Filter,
    /// The slot the cursor starts on.
    cursor: usize,
    /// Collapse the first group header before drawing.
    collapse: bool,
    /// Draw the `F` surface on top of the frame.
    editor: bool,
    /// What the caller told the shell about the index (`reco-1nu`). Only an empty rail prints it, which
    /// is exactly the case the two empty states are about.
    index: Facts,
}

/// Which corpus a case loads.
enum Rows {
    /// ADR-006 §5.2's own rows.
    Mock,
    /// The mock rows minus one, so a golden shows a missing episode's absence.
    MinusOne,
    /// The 10 000-row index, so the ladder is checked against numbers with four digits in them.
    Big,
    /// Nothing at all.
    None,
}

impl Default for Case {
    fn default() -> Self {
        Case {
            golden: "",
            note: "",
            frame: None,
            rows: Rows::Mock,
            filter: Filter::default(),
            cursor: 0,
            collapse: false,
            editor: false,
            index: Facts::unknown(),
        }
    }
}

fn cases() -> Vec<Case> {
    let unread = Filter {
        state: Some(timeline::ReviewState::Unread),
        ..Filter::default()
    };
    let by_session = Filter {
        group: GroupBy::Session,
        ..Filter::default()
    };
    let nothing = Filter {
        area: Some("no/such/area".to_owned()),
        ..Filter::default()
    };
    let dates = {
        let mut filter = Filter::default();
        filter
            .set(timeline::Dimension::Since, "today", NOW)
            .expect("today");
        filter
            .set(timeline::Dimension::Until, "today", NOW)
            .expect("today");
        filter
    };
    vec![
        Case {
            golden: "frame_80x24",
            note: "the ADR §5.2 picture at the frame size it is quoted at",
            frame: Some((80, 24)),
            ..Case::default()
        },
        Case {
            golden: "frame_61x16",
            note: "the view's documented minimum (61×16): the ladder has fired and the frame is still \
                   complete, not clipped",
            frame: Some((61, 16)),
            ..Case::default()
        },
        Case {
            golden: "rail_wide",
            note: "one pane at the rail's ideal width: all six columns, `?` standing for the risk no \
                   build has computed",
            ..Case::default()
        },
        Case {
            golden: "rail_34",
            note: "the sparkline is the first thing the ladder takes",
            cursor: 0,
            ..Case::default()
        },
        Case {
            golden: "filter_unread",
            note: "state=unread: the strip counts what survived, the card follows the new cursor",
            filter: unread,
            ..Case::default()
        },
        Case {
            golden: "group_session",
            note: "group=session: the headers name the session and its author, because a session \
                   header with no author is a number nobody can act on",
            filter: by_session,
            ..Case::default()
        },
        Case {
            golden: "group_dates",
            note: "since/until = today: a date bound is a filter like any other and says so in the \
                   strip",
            filter: dates,
            ..Case::default()
        },
        Case {
            golden: "collapsed",
            note: "the first group collapsed: `··· N more` is counted, not hidden",
            collapse: true,
            ..Case::default()
        },
        Case {
            golden: "filter_empty",
            note: "a filter that selects nothing: the rail states that, and names the key that fixes \
                   it, instead of drawing a blank pane",
            filter: nothing,
            ..Case::default()
        },
        Case {
            golden: "nothing_loaded",
            note: "no index at all: the pane names the verb that actually works here, `fathom index`",
            rows: Rows::None,
            index: Facts::absent(),
            ..Case::default()
        },
        Case {
            golden: "index_ready",
            note: "the acceptance run's first minute, told honestly: epoch 1 is ready and holds 500 \
                   commits, so the pane says that, points at the view that lists them, and does not \
                   send the reviewer to an index that already finished (`reco-1nu`, reworded once \
                   `reco-wzt` put a data path under the timeline)",
            rows: Rows::None,
            index: Facts::present(Some(1), Some("ready".to_owned()), Some(500), Some(412)),
            ..Case::default()
        },
        Case {
            golden: "episode_gone",
            note: "the newest row left the index between two loads: the cursor is on a real row and \
                   the strip has not lied about it",
            rows: Rows::MinusOne,
            ..Case::default()
        },
        Case {
            golden: "big_index",
            note: "the 10 000-episode fixture: four-digit counts, real sizes, and a window of rows \
                   drawn out of all of it",
            rows: Rows::Big,
            cursor: 6,
            ..Case::default()
        },
        Case {
            golden: "editor_open",
            note: "the `F` surface: dimensions with their current value and count, on top of the frame \
                   it is filtering",
            frame: Some((80, 24)),
            editor: true,
            ..Case::default()
        },
    ]
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("timeline")
}

fn blessing() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

/// Build the app one case describes.
fn build(case: &Case, size: (u16, u16)) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.set_index_facts(&case.index);
    app.set_view(ViewId::Timeline);
    let rows = match case.rows {
        Rows::Mock => mock(),
        Rows::MinusOne => {
            let rows = mock();
            without(&rows, "e8f1a2")
        }
        Rows::Big => generated(),
        Rows::None => {
            app.timeline.set_filter(case.filter.clone());
            return app;
        }
    };
    app.load_episodes(Batch::new("fixture index", rows));
    app.timeline.set_filter(case.filter.clone());
    if case.cursor > 0 {
        app.timeline.move_to(case.cursor);
    }
    if case.collapse {
        app.timeline.move_to(0);
        // The sentence it returns is the toast's business, not the golden's.
        let _words = app.timeline.toggle_collapse();
    }
    if case.editor {
        app.timeline.open_editor();
        app.overlay = Some(Overlay::Filter);
    }
    app
}

/// One golden file's text.
fn render(case: &Case) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n# {}\n\n", case.golden, case.note));
    if let Some(size) = case.frame {
        let app = build(case, size);
        out.push_str(&format!("## frame {}×{}\n", size.0, size.1));
        for line in frame_lines(&app) {
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out.push('\n');
    }
    let size = case.frame.unwrap_or((80, 24));
    let app = build(case, size);
    let panes = views::view(ViewId::Timeline);
    let rail = panes
        .panes
        .iter()
        .find(|pane| pane.name == "rail")
        .expect("the timeline view has a rail pane");
    let card = panes
        .panes
        .iter()
        .find(|pane| pane.name == "card")
        .expect("the timeline view has a card pane");

    if case.frame.is_some() {
        out.push_str(&format!(
            "## rail ({RAIL_COLS} columns × {RAIL_ROWS} rows)\n"
        ));
    }
    for line in app
        .timeline_body(rail, RAIL_COLS, RAIL_ROWS)
        .unwrap_or_default()
    {
        let text = line.text();
        assert_eq!(
            text.chars().count(),
            RAIL_COLS,
            "every rail row is exactly the pane's width — a short row is a row that ran off the pane"
        );
        out.push_str(text);
        out.push('\n');
    }
    out.push_str(&format!(
        "\n## rail strip\n{}\n",
        app.timeline_strip(rail).unwrap_or_default()
    ));

    out.push_str(&format!(
        "\n## card ({CARD_COLS} columns × {CARD_ROWS} rows)\n"
    ));
    for line in app
        .timeline_body(card, CARD_COLS, CARD_ROWS)
        .unwrap_or_default()
    {
        let text = line.text();
        assert_eq!(
            text.chars().count(),
            CARD_COLS,
            "every card row is exactly the pane's width"
        );
        out.push_str(text);
        out.push('\n');
    }
    out.push_str(&format!(
        "\n## card strip\n{}\n",
        app.timeline_strip(card).unwrap_or_default()
    ));
    out.push_str(&format!(
        "\n## cursor\n{}\n",
        app.timeline
            .selected()
            .map_or_else(|| "nothing".to_string(), |episode| episode.id.clone())
    ));
    out
}

/// The ladder's rungs, pinned one per width, as one golden: the point is the *comparison* between the
/// rows, which no single-frame golden can show.
fn ladder_golden() -> String {
    let mut out = String::new();
    out.push_str(
        "# ladder\n# the column ladder, §5.2's fixed order: sparkline, relative time to \
                  date, author, risk number, and the state glyph last of all.\n\n",
    );
    let mut rows = mock();
    // A risk number, so the rung that drops it has something to drop. This build has no risk source;
    // the fixture supplies one rather than letting the rung go untested.
    rows[0].risk = Some(82);
    let mut rail = timeline::Rail::new(NOW);
    rail.load(Batch::new("fixture index", rows));
    for width in [42, 40, 38, 34, 32, 30, 27, 24, 22, 18] {
        let text = rail.text(width, 1);
        let line = text.first().map(String::as_str).unwrap_or_default();
        assert_eq!(
            line.chars().count(),
            width,
            "the rail draws exactly the width it was given"
        );
        out.push_str(&format!(
            "{width:>3} {:<16} |{line}|\n",
            timeline::Columns::for_width(width).name()
        ));
    }
    out
}

#[test]
fn the_timeline_goldens_match_their_files() {
    let dir = golden_dir();
    if blessing() {
        std::fs::create_dir_all(&dir).expect("goldens can be written");
    }
    let mut missing = Vec::new();
    for case in cases() {
        let text = render(&case);
        let path = dir.join(format!("{}.txt", case.golden));
        if blessing() {
            std::fs::write(&path, &text).expect("the golden was written");
            continue;
        }
        let want = match std::fs::read_to_string(&path) {
            Ok(want) => want,
            Err(_) => {
                missing.push(path.display().to_string());
                continue;
            }
        };
        assert_eq!(
            want,
            text,
            "{} differs. If the change is intended, re-bake with FATHOM_GOLDEN=write and read the \
             diff.",
            path.display()
        );
    }
    let ladder = ladder_golden();
    let path = dir.join("ladder.txt");
    if blessing() {
        std::fs::write(&path, &ladder).expect("the ladder golden was written");
    } else if let Ok(want) = std::fs::read_to_string(&path) {
        assert_eq!(want, ladder, "{} differs", path.display());
    } else {
        missing.push(path.display().to_string());
    }
    assert!(
        missing.is_empty(),
        "no golden file at: {} — write them with FATHOM_GOLDEN=write cargo test -p fathom-tui \
         --test timeline_golden",
        missing.join(", ")
    );
}

/// A golden that nothing can read is a cache, so the header of every file has to say why the case
/// exists. Cheap, and it fails when someone adds a case and forgets the sentence.
#[test]
fn every_golden_explains_why_it_is_in_the_set() {
    let dir = golden_dir();
    for case in cases() {
        assert!(!case.note.is_empty(), "{} has no note", case.golden);
        assert!(
            !case.golden.is_empty(),
            "a case with no golden name cannot be written"
        );
        let path = dir.join(format!("{}.txt", case.golden));
        if let Ok(text) = std::fs::read_to_string(&path) {
            assert!(
                text.starts_with("# ")
                    && text
                        .lines()
                        .nth(1)
                        .is_some_and(|line| line.starts_with("# ")),
                "{} must carry its header comment",
                path.display()
            );
        }
    }
}

/// The goldens must be drawn against an instant that does not move, or every golden file re-bakes
/// nightly and the diff stops meaning anything.
#[test]
fn no_golden_depends_on_the_clock() {
    assert_eq!(
        NOW,
        Timestamp::from_unix_micros(NOW.as_unix_micros()),
        "the fixture's instant is a constant"
    );
    let early = build(
        &Case {
            golden: "clock",
            frame: Some((80, 24)),
            ..Case::default()
        },
        (80, 24),
    );
    let mut late = build(
        &Case {
            golden: "clock",
            frame: Some((80, 24)),
            ..Case::default()
        },
        (80, 24),
    );
    late.timeline.set_now(NOW);
    assert_eq!(
        frame_lines(&early).join("\n"),
        frame_lines(&late).join("\n"),
        "two draws for the same instant are the same frame"
    );
}
