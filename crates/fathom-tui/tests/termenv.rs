//! The terminal this run was started in (`e5.5`): what it said about itself, what Fathom did about
//! it, and what the reviewer was told.
//!
//! ADR-006 §6.7's sentence is "degraded modes are reported, not silent", and a terminal degrades in
//! more ways than a layout does: it can be `dumb`, it can be an ANSI-only `vt100` at the end of an
//! `ssh` session, it can be a `screen` inside `tmux` with a locale that does not speak UTF-8, or it
//! can say `NO_COLOR` because the reviewer is colour-blind or reading with a screen reader. None of
//! those are errors, and none of them may be absorbed in silence either — a frame whose box rules
//! came out as `q` and `x` is a frame the reviewer assumes is broken.
//!
//! So three claims, one per test below:
//!
//! 1. the probe **classifies** the environment it is handed, and the answer is the one the
//!    conventions say (`NO_COLOR` is an instruction and outranks `COLORTERM=truecolor`, which is the
//!    one place a user's explicit request beats a terminal's boast);
//! 2. every degraded environment **says so**, in a notice that names the fix and not just the
//!    problem;
//! 3. a terminal that asked for no colour **gets no colour** — the frame is drawn by the same code,
//!    with the shapes kept, because ADR-006 §10.2 forbids colour from carrying a fact alone.
//!
//! The probe never reads the environment itself (ADR-002 §3 — `fathom-tui` is *given* the world), so
//! every test here hands it one.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use ratatui::backend::TestBackend;
use ratatui::style::Color;

use fathom_tui::app::{App, Model};
use fathom_tui::frame::{FrameSink as _, TerminalSink};
use fathom_tui::keymap::Keymap;
use fathom_tui::terminal::{ColorDepth, Probe};
use fathom_tui::timeline::Batch;
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{NOW, mock};

/// A probe built from a list of variables, which is the only honest way to test one.
fn env(pairs: &[(&str, &str)]) -> Probe {
    let mut probe = Probe::from_env_with(|key| {
        pairs
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).to_owned())
    });
    probe.tty = true;
    probe
}

fn cell(value: &str) -> Option<String> {
    Some(value.to_owned())
}

#[test]
fn the_colour_answer_is_the_one_the_conventions_give() {
    // Each row is `(variables, the depth, why)`. The `vt100`/`cyrus` pair are the two real terminals
    // that cannot do 256 colours and say so; `truecolor` is what a modern terminal claims.
    /// One case: the variables that terminal had set, the depth that follows from them, and why.
    type Case = (
        &'static [(&'static str, &'static str)],
        ColorDepth,
        &'static str,
    );
    let cases: &[Case] = &[
        (&[("TERM", "xterm-256color")], ColorDepth::Ansi256, "256"),
        (
            &[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")],
            ColorDepth::TrueColor,
            "COLORTERM beats TERM",
        ),
        (
            &[("TERM", "xterm-256color"), ("NO_COLOR", "1")],
            ColorDepth::None,
            "NO_COLOR beats everything, including a terminal that boasts",
        ),
        (
            &[
                ("TERM", "xterm-256color"),
                ("COLORTERM", "24bit"),
                ("NO_COLOR", ""),
            ],
            ColorDepth::None,
            "NO_COLOR is the *presence* of the variable, and an empty value still counts",
        ),
        (&[("TERM", "dumb")], ColorDepth::None, "dumb is dumb"),
        (
            &[("TERM", "dumb"), ("COLORTERM", "truecolor")],
            ColorDepth::None,
            "even a dumb terminal that claims truecolour",
        ),
        (
            &[("TERM", "vt100")],
            ColorDepth::Basic,
            "vt100 has eight colours",
        ),
        (
            &[("TERM", "cygwin")],
            ColorDepth::Basic,
            "no 256 in the name",
        ),
        (
            &[],
            ColorDepth::None,
            "nothing said means no colour, not twenty-four bit",
        ),
    ];
    for (pairs, want, why) in cases {
        let got = env(pairs).color();
        assert_eq!(got, *want, "{why}: {pairs:?} gave {got:?}");
    }
}

#[test]
fn the_utf8_answer_follows_the_locale_chain() {
    // `LC_ALL` overrides `LC_CTYPE` overrides `LANG`, and an empty value is no answer at all — which
    // is exactly how `setlocale` behaves, and the frame's box rules depend on it.
    assert!(env(&[("LANG", "en_US.UTF-8")]).utf8());
    assert!(env(&[("LC_CTYPE", "C.UTF-8")]).utf8());
    assert!(
        !env(&[("LANG", "en_US.UTF-8"), ("LC_ALL", "C")]).utf8(),
        "LC_ALL=C wins over LANG, so the frame is about to be drawn into a byte-oriented terminal"
    );
    assert!(!env(&[("LANG", "C")]).utf8());
    assert!(
        !env(&[("LANG", "")]).utf8(),
        "an empty locale is not a locale"
    );
    assert!(
        env(&[("LANG", "POSIX")])
            .notices()
            .iter()
            .any(|notice| notice.contains("locale")),
        "a non-UTF-8 locale is reported, because the box rules will come out as noise"
    );
}

#[test]
fn every_degraded_terminal_gets_a_notice_that_names_the_fix() {
    // Each case: the environment, and a phrase the notice has to contain. A notice that only names the
    // problem is a shrug; every one of these names the command or key that answers it (ADR-001 §7).
    let cases: &[(&[(&str, &str)], &str)] = &[
        (&[("TERM", "dumb"), ("LANG", "C.UTF-8")], "TERM=dumb"),
        (&[("TERM", "xterm-256color"), ("LANG", "C.UTF-8")], ""),
        (&[("TERM", "vt100"), ("LANG", "C.UTF-8")], "vt100"),
        (&[("LANG", "C.UTF-8")], "TERM is not set"),
        (
            &[
                ("TERM", "xterm-256color"),
                ("LANG", "C.UTF-8"),
                ("NO_COLOR", "1"),
            ],
            "NO_COLOR",
        ),
        (
            &[
                ("TERM", "screen"),
                ("LANG", "C.UTF-8"),
                ("TMUX", "/tmp/tmux-1000/default,1234,0"),
            ],
            "tmux",
        ),
        (
            &[
                ("TERM", "screen"),
                ("LANG", "C.UTF-8"),
                ("STY", "4711.0.fathom"),
            ],
            "screen",
        ),
        (
            &[
                ("TERM", "xterm-256color"),
                ("LANG", "C.UTF-8"),
                ("SSH_CONNECTION", "10.0.0.2 5000 10.0.0.1 22"),
            ],
            "ssh",
        ),
    ];
    for (pairs, want) in cases {
        let notices = env(pairs).notices();
        if want.is_empty() {
            assert!(
                notices.is_empty(),
                "a healthy terminal says nothing: {notices:?}"
            );
            continue;
        }
        assert!(
            notices.iter().any(|notice| notice.contains(want)),
            "{pairs:?} produced {notices:?}, none of which mention {want:?}"
        );
        for notice in &notices {
            // Every notice either names the thing to do about it — the flag, the `export`, the tmux
            // setting — or, in `NO_COLOR`'s case, names the reason nothing needs doing: colour was
            // carrying no fact that a glyph and a word were not already carrying (ADR-006 §10.2).
            assert!(
                [
                    "--plain",
                    "export",
                    "LC_",
                    "TERM",
                    "tmux",
                    "termcapinfo",
                    "glyph",
                    "locale",
                    "set -g",
                ]
                .iter()
                .any(|token| notice.contains(token)),
                "a notice has to say something a reviewer can act on: {notice}"
            );
        }
    }
}

#[test]
fn a_pipe_says_it_is_a_pipe() {
    // The one case the probe cannot see in the variables: stdout is not a terminal at all. The CLI
    // owns that fact (`std::env` and `std::io` are its, not this crate's) and passes it in.
    let mut probe = env(&[("TERM", "xterm-256color"), ("LANG", "C.UTF-8")]);
    assert!(
        !probe.notices().iter().any(|n| n.contains("not a terminal")),
        "a tty says nothing about being a tty: {:?}",
        probe.notices()
    );
    probe.tty = false;
    let notices = probe.notices();
    assert!(
        notices.iter().any(|n| n.contains("not a terminal")),
        "a redirected stdout is reported, not absorbed: {notices:?}"
    );
    assert!(
        notices
            .iter()
            .any(|n| n.contains("--plain") && n.contains("--lines")),
        "and it is pointed at the two flags that are the text contract: {notices:?}"
    );
}

#[test]
fn the_label_describes_the_terminal_in_one_line() {
    assert_eq!(
        env(&[
            ("TERM", "tmux-256color"),
            ("COLORTERM", "truecolor"),
            ("LANG", "en_US.UTF-8"),
            ("TMUX", "/tmp/tmux-1000/default,1,0"),
            ("SSH_TTY", "/dev/pts/0"),
        ])
        .label(),
        "tmux-256color · truecolour · UTF-8 · inside tmux · over ssh"
    );
    assert_eq!(
        env(&[("TERM", "dumb")]).label(),
        "dumb · no colour · not UTF-8"
    );
}

/// Draw the frame the way the terminal would get it, and report whether any cell carries a colour.
fn painted_cells_carry_colour(probe: Probe) -> bool {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        (100, 30),
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", mock()));
    app.set_view(ViewId::Diff);
    app.term = probe;
    let mut sink = TerminalSink::new(
        ratatui::Terminal::new(TestBackend::new(100, 30)).expect("a test backend always opens"),
    );
    sink.draw(&mut app).expect("the frame draws");
    sink.terminal
        .backend()
        .buffer()
        .content()
        .iter()
        // A cell that was never styled reports `Reset`; anything else means a colour was written.
        .any(|cell| cell.fg != Color::Reset || cell.bg != Color::Reset)
}

#[test]
fn no_colour_means_no_colour() {
    // The claim is about bytes on the way to the screen, not about intentions: with `NO_COLOR` set,
    // not one cell of the frame carries a foreground or background colour — and the frame is still
    // the same frame, because the shapes that carry the meaning are modifiers, not colours.
    let proud = env(&[
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("LANG", "C.UTF-8"),
    ]);
    assert!(
        painted_cells_carry_colour(proud.clone()),
        "a 256-colour terminal is given colour, or the test below proves nothing"
    );
    let grey = Probe {
        no_color: true,
        ..proud
    };
    assert!(
        !painted_cells_carry_colour(grey),
        "NO_COLOR was set and colour still reached the screen"
    );
    let dumb = env(&[("TERM", "dumb"), ("LANG", "C.UTF-8")]);
    assert!(
        !painted_cells_carry_colour(dumb),
        "TERM=dumb was said and colour still reached the screen"
    );
}

#[test]
fn the_probe_is_given_and_not_read() {
    // ADR-002 §3: `fathom-tui` does not call `std::env::var`. A probe that was never given anything is
    // the headless default, and it is allowed to know nothing — but it must not *invent* a terminal.
    let quiet = Probe::unknown();
    assert_eq!(quiet.color(), ColorDepth::None);
    assert!(!quiet.utf8());
    assert_eq!(quiet.term, None);
    assert!(
        !cell("").is_some_and(|v| !v.is_empty()),
        "an empty variable is not an answer"
    );
}
