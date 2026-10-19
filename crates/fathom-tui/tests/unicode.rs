//! Unicode-safe rendering (`e5.5`): a reviewed fixture set, and the random text that goes looking
//! for what the fixture set missed.
//!
//! Three things have to be true at once, and each fails differently:
//!
//! 1. **A column costs the cells it was measured at.** A CJK ideograph costs two cells, a combining
//!    mark zero, an emoji usually two. Get one wrong by one and the box is one cell wider than the
//!    terminal, which wraps, and every line below it shifts by one.
//! 2. **A cut lands on a grapheme-cluster boundary.** `é` is one cluster whether it is `U+00E9` or
//!    `e` + `U+0301`; a ZWJ emoji family is one cluster of five characters. Cutting inside either
//!    does not produce a shorter cluster — it produces a different character and an ellipsis that
//!    costs a cell the terminal did not charge for.
//! 3. **Nothing invisible is drawn as itself.** A commit subject, a path and a diff line are all
//!    text somebody else wrote. A `U+202E` bidi override, a `U+0007` bell or a `U+2060` word joiner
//!    can hide part of a frame inside a filename, and the reviewer cannot see the half of the line
//!    they were not shown.
//!
//! [`the_fixture_set_*`] pins all three with named, reviewed strings — the ones a reviewer can read.
//! [`proptest`] is the same three claims over ten thousand strings nobody typed, which is where a
//! lone combining mark, a lone ZWJ, a C1 control and an unassigned code point turn up. That is the
//! acceptance criterion that says *"proptest finds no panic on random text"*, and it is a property
//! test rather than a fixture list precisely because the interesting inputs are the ones nobody
//! thinks to write down.
//!
//! ```text
//! cargo test -p fathom-tui --test unicode
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::layout::{display_width, fit_text};
use fathom_tui::render::{frame_lines, view_lines};
use fathom_tui::text::{self, clip, cluster_width, sanitize, width};
use fathom_tui::timeline::Batch;
use fathom_tui::views::ViewId;
use proptest::prelude::*;
use unicode_segmentation::UnicodeSegmentation;

#[path = "timeline_corpus/mod.rs"]
mod rail;

/// The diff fixtures: the same helper set `diff_behaviour.rs` uses, under a name that does not collide
/// with the timeline corpus above.
#[path = "corpus/mod.rs"]
mod diffcorpus;

use rail::NOW;

/// The fixture set: one entry per shape that has to work, named so a reviewer can read what is being
/// asserted about a string they may not be able to type. Each is `(what it is, the text)`.
const FIXTURES: &[(&str, &str)] = &[
    ("cjk-ideographs", "重构支付模块 · 調整した請求書"),
    ("cjk-with-ascii", "refactor: 支付 restructure 請求書 module"),
    ("japanese-kana", "認証まわりのリファクタリング"),
    ("korean", "결제 모듈 리팩터링"),
    (
        "accents-precomposed",
        "Corrección de la contraseña del cliente",
    ),
    (
        "accents-decomposed",
        "Correccio\u{301}n de la contraseñ\u{301}a del cliente",
    ),
    (
        "combining-stack",
        "a\u{302}\u{301} b\u{302}\u{301} c\u{300} refactor",
    ),
    ("devanagari", "बदलाव भुगतान मॉड्यूल में"),
    ("emoji-simple", "fix: 👀 watch this 🚀 ship it"),
    (
        "emoji-varselector",
        "red heart \u{2764}\u{fe0f} with a variation selector",
    ),
    (
        "emoji-family-zwj",
        "one glyph five characters 👨‍👩‍👧‍👦 in a subject",
    ),
    ("emoji-skin-tone", "👋🏾 a wave with a modifier"),
    ("emoji-flag", "🇯🇵 a flag is two regional indicators"),
    ("arabic-rtl", "إعادة هيكلة وحدة الدفع"),
    ("hebrew-rtl", "שיפוץ מודול התשלומים"),
    ("arabic-with-numbers", "הצגת 42 שורות חדשות"),
    ("path-cjk", "src/支付/payments_請求書.rs"),
    ("path-emoji", "docs/🚀launch/README.md"),
    ("path-spaces", "src/my file (copy).rs"),
    (
        "long-and-mixed",
        "🐛 fix: 인증 — the risk number 🔒 goes before the state glyph does, in a narrow column",
    ),
];

/// The characters that must never reach the screen as themselves: every one of them legal in a commit
/// message, a path or a diff line, and none of them visible.
const HOSTILE: &[(&str, &str)] = &[
    ("bell", "fix\u{7}: a bell in a subject"),
    ("carriage-return", "line\rone overwrites the other"),
    ("vertical-tab", "line\u{b}two"),
    ("form-feed", "line\u{c}two"),
    ("nul", "fix\u{0}: a NUL in a subject"),
    ("c1-control", "fix\u{85}: a C1 control"),
    ("del", "fix\u{7f}: a DEL"),
    ("rtl-override", "src/crs/tepocymo\u{202e}evil.rs"),
    (
        "rlm-in-a-path",
        "fix `\u{200f}payment.rs` a mark hidden in a subject",
    ),
    ("rtl-embed", "a\u{202b}b"),
    ("lro", "\u{202d}src/lib.rs"),
    ("rtl-isolate", "a\u{2067}b"),
    ("word-joiner", "pay\u{2060}ment.rs"),
    ("lr-mark", "a\u{200e}b"),
    ("bom", "\u{feff}a byte-order mark mid-line"),
    ("tab-inside-a-line", "one\ttwo"),
    ("newline-inside-a-line", "one\ntwo"),
];

/// An app whose one episode carries `text` in every place text can hide: subject, author, two paths,
/// and the branch name in the header.
fn app_with(text: &str, size: (u16, u16)) -> App {
    let mut episode = rail::mock()
        .into_iter()
        .next()
        .expect("the mock corpus has a row");
    episode.summary = text.to_owned();
    episode.author_name = text.to_owned();
    episode.paths = vec![text.to_owned(), format!("src/{text}.rs")];
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        format!("fathom/pilot · {text}"),
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("unicode fixtures", vec![episode]));
    app
}

// ---------------------------------------------------------------------------
// the fixture half
// ---------------------------------------------------------------------------

#[test]
fn the_fixture_widths_are_the_cells_a_terminal_advances_by() {
    // Each of these is a claim about one specific string, reviewed by a person rather than derived by
    // a property — the property is that these hold, and here is where the values are pinned.
    assert_eq!(display_width("queue"), 5);
    assert_eq!(display_width("支付"), 4, "two ideographs are four cells");
    assert_eq!(display_width("é"), 1, "a combining mark is free");
    assert_eq!(
        display_width("é"),
        display_width("é"),
        "precomposed and decomposed must cost the same, or a column depends on how a file was saved"
    );
    assert_eq!(display_width("👀"), 2, "an emoji is two cells");
    assert_eq!(
        display_width("👨‍👩‍👧"),
        6,
        "a ZWJ family is the sum of its parts — the pessimistic answer, and the one `text` argues for"
    );
    assert_eq!(display_width("إعادة"), 5, "RTL letters cost one cell each");
    assert_eq!(
        display_width("a\0b"),
        3,
        "a control char costs the cell its replacement is drawn in"
    );
}

#[test]
fn the_fixture_set_fills_the_column_it_was_given() {
    for (name, fixture) in FIXTURES {
        for cells in [1, 2, 3, 7, 12, 20, 40] {
            let fitted = fit_text(fixture, cells);
            assert_eq!(
                width(&fitted),
                cells,
                "{name}: {fixture:?} fitted to {cells} cells came back as {:?} ({} cells)",
                fitted,
                width(&fitted)
            );
            if width(fixture) > cells {
                assert!(
                    fitted.trim_end().ends_with(text::ELLIPSIS),
                    "{name}: clipping {fixture:?} to {cells} should say that it did: {fitted:?}"
                );
            } else {
                assert_eq!(
                    fitted.trim_end(),
                    *fixture,
                    "{name}: text that fits must not be rewritten: {fitted:?}"
                );
            }
        }
    }
}

#[test]
fn the_fixture_set_needs_no_sanitizing() {
    // The other half of the sanitizer's contract: it is a sanitizer, not a mangler. Every character
    // of every fixture above — CJK, Arabic, ZWJ families, variation selectors — is content, and
    // content survives byte for byte.
    for (name, fixture) in FIXTURES {
        assert!(
            !text::needs_sanitizing(fixture),
            "{name}: {fixture:?} was flagged as hostile, and would be rewritten"
        );
        assert_eq!(sanitize(fixture), *fixture, "{name} was rewritten");
    }
}

#[test]
fn the_hostile_set_is_made_visible_instead_of_drawn() {
    for (name, text) in HOSTILE {
        assert!(
            text::needs_sanitizing(text),
            "{name}: {text:?} was not flagged, so it would have been pasted through"
        );
        let clean = sanitize(text);
        assert!(
            !clean.chars().any(|ch| text::substitute(ch).is_some()),
            "{name}: a control or format character survived sanitizing: {clean:?}"
        );
        assert!(
            !clean.chars().any(|ch| (ch as u32) < 0x20 || ch == '\u{7f}'),
            "{name}: a control character reached the sanitized string: {clean:?}"
        );
        assert!(
            !clean.chars().any(|ch| matches!(
                ch,
                '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}'
            )),
            "{name}: a bidi or zero-width format control survived sanitizing: {clean:?}"
        );
        // And the substitution costs exactly the cell `width` counts, or the column would drift.
        assert_eq!(
            width(&clean),
            text.chars().count(),
            "{name}: sanitizing changed the cell count of {text:?}: {clean:?}"
        );
        assert_eq!(
            clean.chars().count(),
            text.chars().count(),
            "{name}: sanitizing changed the character count of {text:?}: {clean:?}"
        );
    }
}

#[test]
fn a_cut_never_lands_inside_a_grapheme_cluster() {
    for (name, fixture) in FIXTURES {
        for cells in 1..=width(fixture).max(1) {
            let cut = clip(fixture, cells);
            // Whatever came back is whole clusters, so it re-measures stably and carries no orphaned
            // mark: a cluster's marks travel with its base.
            for cluster in cut.trim_end_matches(text::ELLIPSIS).graphemes(true) {
                assert!(
                    !cluster.starts_with(is_mark),
                    "{name} at {cells} cells was cut in front of a combining mark: {cut:?}"
                );
                assert!(
                    !cluster.starts_with('\u{200d}') && !cluster.starts_with('\u{fe0f}'),
                    "{name} at {cells} cells was cut in front of a joiner or selector: {cut:?}"
                );
            }
            assert!(
                width(&cut) <= cells,
                "{name} at {cells} cells came back as {} cells: {cut:?}",
                width(&cut)
            );
        }
    }
}

fn is_mark(ch: char) -> bool {
    ('\u{0300}'..='\u{036f}').contains(&ch)
}

#[test]
fn a_wide_character_is_never_cut_in_half() {
    // One cell left and two cells of ideograph to put there: the cluster goes whole, and the ellipsis
    // takes the cell — rather than half a glyph being printed and the column drifting by one.
    assert_eq!(clip("支付", 3), "支…");
    assert_eq!(clip("👀👀", 3), "👀…");
    assert_eq!(
        clip("é", 1),
        "é",
        "a base plus its mark is one cell, not two"
    );
    assert_eq!(clip("éx", 2), "éx");
    assert_eq!(
        clip("éx", 1),
        "…",
        "one cell is the ellipsis's, not half of `éx`"
    );
    assert_eq!(
        clip("👨‍👩‍👧", 3),
        "…",
        "a cluster wider than the column is dropped whole"
    );
    assert_eq!(clip("", 5), "");
    assert_eq!(
        clip("queue", 5),
        "queue",
        "exactly-fitting text is not clipped"
    );
    assert_eq!(clip("queue", 4), "que…");
    assert_eq!(clip("queue", 1), "…");
    assert_eq!(
        clip("queue", 0),
        "…",
        "a zero-cell column cannot be drawn anyway"
    );
}

#[test]
fn a_frame_full_of_the_fixture_set_is_still_exactly_the_frame() {
    // "renders with correct alignment", made testable: every drawn line is exactly `cols` cells at
    // every matrix size, for every view, for every fixture — including the subject three times wider
    // than the pane and the path carrying a bidi override that would have reordered the frame had it
    // been pasted through.
    for (name, fixture) in FIXTURES.iter().chain(HOSTILE.iter()) {
        for size in [(40, 10), (80, 24), (100, 30), (160, 50)] {
            let app = app_with(fixture, size);
            for view in [ViewId::Queue, ViewId::Timeline, ViewId::Search] {
                let lines = view_lines(&app, view, usize::from(size.0), usize::from(size.1));
                assert_eq!(
                    lines.len(),
                    usize::from(size.1),
                    "{name} in {view:?} at {}×{} drew {} lines",
                    size.0,
                    size.1,
                    lines.len()
                );
                for (index, line) in lines.iter().enumerate() {
                    assert_eq!(
                        display_width(line),
                        usize::from(size.0),
                        "{name} in {view:?} at {}×{} line {index} is not the frame width: {line:?}",
                        size.0,
                        size.1
                    );
                    assert!(
                        line.chars().all(|ch| text::substitute(ch).is_none()),
                        "{name} in {view:?} line {index} reached the screen with something invisible \
                         in it: {line:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_hostile_path_is_visible_in_the_diff_panes_too() {
    // `unquote` is the one place git's C-style quoting becomes a path, and the pane that prints it is
    // the one a reviewer quotes into a command line. The hunk text is the other way in: a diff line is
    // as much somebody else's text as a commit subject is.
    let mut app = diffcorpus::app_with("mixed", (110, 30));
    let patch = "diff --git a/\"src/\\302\\241\u{202e}evil.rs\" b/\"src/\\302\\241\u{202e}evil.rs\"\n\
                 --- a/\"src/\\302\\241\u{202e}evil.rs\"\n\
                 +++ b/\"src/\\302\\241\u{202e}evil.rs\"\n\
                 @@ -1 +1 @@\n-before\n+after\twith a tab and a bell\u{7}\n";
    app.load_patch(diffcorpus::source(patch))
        .expect("the patch loads");
    for line in diffcorpus::tree_rows(&app) {
        assert!(
            line.chars().all(|ch| text::substitute(ch).is_none()),
            "a path reached the tree pane still carrying an invisible character: {line:?}"
        );
    }
    let hunks = diffcorpus::hunk_rows(&app);
    assert!(
        !hunks.is_empty(),
        "the hunk pane drew the added line at all"
    );
    for line in &hunks {
        assert!(
            line.chars().all(|ch| text::substitute(ch).is_none()),
            "a diff line reached the screen with an invisible character in it: {line:?}"
        );
    }
    // The tab is still expanded to the tab stop the rest of the pane aligns on, so the hunk's column
    // arithmetic did not change; the bell became something a reviewer can quote.
    assert!(
        hunks.iter().any(|line| line.contains("after    with")),
        "the tab still expands to the tab stop: {hunks:?}"
    );
    assert!(
        hunks.iter().any(|line| line.contains(text::REPLACEMENT)),
        "the bell in the added line became visible rather than invisible: {hunks:?}"
    );
}

#[test]
fn the_line_mode_survives_the_whole_fixture_set() {
    // The `--plain --lines` path is a different route from the frame to the same text, and `less` is
    // the consumer that would show up a wrapped line worst.
    for (name, fixture) in FIXTURES.iter().chain(HOSTILE.iter()) {
        let app = app_with(fixture, (100, 30));
        for line in fathom_tui::render::plain_flow(&app) {
            assert!(
                !line.contains('\u{1b}'),
                "{name}: the line mode wrote an escape sequence: {line:?}"
            );
            assert!(
                line.chars().all(|ch| text::substitute(ch).is_none()),
                "{name}: the line mode wrote an invisible character: {line:?}"
            );
            assert!(
                !line.ends_with(' '),
                "{name}: the line mode padded a line it was done with: {line:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// the property half: `proptest finds no panic on random text`
// ---------------------------------------------------------------------------

/// Arbitrary text with the shapes that matter weighted up. The plain `char` strategy would spend
/// nearly all of its cases on Latin and never produce a CJK ideograph, a lone ZWJ, a C1 control or a
/// combining mark in a stream of ASCII — which is precisely where a width table is wrong.
fn weird_text() -> impl Strategy<Value = String> {
    let nasty = prop::sample::select(vec![
        'a', 'z', ' ', '中', '日', '한', '\u{301}', '\u{300}', '\u{200d}', '\u{fe0f}', '👀', '👨',
        '👩', '🇯', '🇵', 'إ', 'ע', 'ა', 'ß', '\u{202e}', '\u{202a}', '\u{2060}', '\u{200e}',
        '\u{2069}', '\u{feff}', '\u{7}', '\u{0}', '\r', '\n', '\t', '\u{85}', '\u{9f}', '\u{7f}',
        '\u{e000}', '\u{ffff}',
    ]);
    // 3:1 against the whole Unicode range, so a case is likely to contain something from this list
    // *and* something from outside it.
    prop::collection::vec(
        prop::strategy::Union::new_weighted(vec![
            (3, nasty.boxed()),
            (1, prop::char::any().boxed()),
        ]),
        0..48,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn clipping_costs_exactly_the_column_it_was_given(text in weird_text()) {
        // No panic, and no line wider than the terminal it was drawn into — for any text at all,
        // including the lone marks and lone joiners a fixture set never thinks to include.
        for cells in [1usize, 2, 5, 13, 40] {
            let fitted = fit_text(&text, cells);
            prop_assert!(
                width(&fitted) == cells,
                "{text:?} at {cells} cells -> {fitted:?}"
            );
        }
        prop_assert_eq!(fit_text(&text, 0), "");
    }

    #[test]
    fn a_cut_is_always_a_cluster_boundary(text in weird_text()) {
        for cells in [1usize, 3, 8, 24] {
            let Some(cut) = text::cut(&text, cells) else { continue };
            prop_assert!(text.is_char_boundary(cut), "{text:?} cut at {cut} is not a char boundary");
            let prefix = &text[..cut];
            // The prefix plus the ellipsis is the drawn string, and it must fit.
            let drawn = format!("{prefix}{}", text::ELLIPSIS);
            prop_assert!(width(&drawn) <= cells, "{text:?} at {cells} -> {drawn:?}");
            // And the cut did not lose a partial cluster: the halves rejoin into the original.
            prop_assert_eq!(format!("{prefix}{}", &text[cut..]), text.to_owned());
        }
    }

    #[test]
    fn measurement_is_stable_under_sanitizing(text in weird_text()) {
        // What is measured is what is drawn. Sanitizing may not change a string's cell count, or a
        // padded column would disagree with the text it padded, and no substitution may be a
        // character that itself needs substituting.
        let clean = sanitize(&text);
        prop_assert_eq!(width(&text), width(&clean));
        prop_assert_eq!(clean.chars().count(), text.chars().count());
        prop_assert!(
            !text::needs_sanitizing(&clean) && clean.chars().all(|ch| text::substitute(ch).is_none()),
            "{text:?} -> {clean:?}"
        );
        prop_assert!(
            clean.chars().all(|ch| text::substitute(ch).is_none()),
            "{text:?} -> {clean:?}"
        );
    }

    #[test]
    fn cluster_summing_agrees_with_string_summing(text in weird_text()) {
        let by_cluster: usize = text.graphemes(true).map(cluster_width).sum();
        prop_assert_eq!(width(&text), by_cluster);
    }

    #[test]
    fn a_frame_never_panics_on_random_text(text in weird_text()) {
        // The renderer's own "no panic", through the real entry point, at matrix sizes and at one
        // below every floor so the refusal panel is in the coverage too.
        for size in [(40, 10), (80, 24), (100, 30), (24, 6)] {
            let app = app_with(&text, size);
            let lines = frame_lines(&app);
            prop_assert_eq!(lines.len(), usize::from(size.1));
            for line in &lines {
                prop_assert!(
                    display_width(line) == usize::from(size.0),
                    "{text:?} at {size:?}: {line:?} is {} cells",
                    display_width(line)
                );
                prop_assert!(
                    line.chars().all(|ch| text::substitute(ch).is_none()),
                    "{text:?} leaked an invisible character into {line:?}"
                );
            }
        }
    }
}
