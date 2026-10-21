//! The keymap: the registry, the default table, the override merge, the chord machine.
//!
//! The behavioural half is the ticket's third acceptance criterion — an override changes what a key
//! does without recompiling, and the help overlay says the same thing — so most of these tests build
//! a `Keymap` from `Override` values the way `fathom-cli` will, then ask the resolver and the help
//! text the same question. The tables themselves are checked in `tests/tables` +
//! `the_action_registry_is_the_normative_one`.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::collections::BTreeSet;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fathom_tui::keymap::{
    self, CHORD_TIMEOUT, ChordState, HINT_ACTIONS, KeyStep, Keymap, Mode, Override, Task, chord_of,
    is_unusable, unusable_reason,
};
use fathom_tui::views::ViewId;

mod tables;

use tables::{EXPECTED_ACTIONS, EXPECTED_DEFAULT_BINDINGS, EXPECTED_TASKS, EXPECTED_UNUSABLE};

/// The ADR's generated blocks, read from the file rather than copied into this test: the same
/// reason `tools/check_docs.py` reads them (ADR-005 §12).
fn adr_block(name: &str) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/adr/0006-tui.md");
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{path} is not readable: {err}"));
    let begin = format!("<!-- BEGIN GENERATED: {name} -->");
    let end = format!("<!-- END GENERATED: {name} -->");
    let start = text
        .find(&begin)
        .unwrap_or_else(|| panic!("{path} has no {begin}"));
    let stop = text[start..]
        .find(&end)
        .map(|at| start + at)
        .unwrap_or_else(|| panic!("{path} has no {end}"));
    text[start + begin.len()..stop].trim().to_owned()
}

/// An override as `fathom-cli` would hand one over. `"-"` is the ADR's spelling for "unbind".
fn bound(mode: Mode, chord: &str, action: &str) -> Override {
    Override {
        mode,
        chord: chord.to_owned(),
        action: Some(action.to_owned()),
        source: None,
    }
}

fn unbound(mode: Mode, chord: &str) -> Override {
    Override {
        mode,
        chord: chord.to_owned(),
        action: None,
        source: None,
    }
}

/// The chain that is live with `view` showing and a list pane focused.
fn chain(view: Mode) -> Vec<Mode> {
    keymap::mode_chain(view, Mode::List)
}

#[test]
fn the_action_registry_is_the_normative_one() {
    type Row = (String, String, String, String, Vec<String>, bool);
    let expected: BTreeSet<Row> = EXPECTED_ACTIONS
        .iter()
        .map(|(id, label, scope, ticket, modes, palette)| {
            (
                (*id).to_owned(),
                (*label).to_owned(),
                (*scope).to_owned(),
                (*ticket).to_owned(),
                modes.iter().map(|m| m.name().to_owned()).collect(),
                *palette,
            )
        })
        .collect();
    let actual: BTreeSet<Row> = keymap::ACTIONS
        .iter()
        .map(|a: &fathom_tui::keymap::Action| {
            (
                a.id.to_owned(),
                a.label.to_owned(),
                a.scope.to_owned(),
                a.ticket.to_owned(),
                a.modes.iter().map(|m| m.name().to_owned()).collect(),
                a.palette,
            )
        })
        .collect();
    assert_eq!(
        expected, actual,
        "registry drift between the Python tables and the Rust port"
    );
}

#[test]
fn every_default_binding_is_in_the_default_table() {
    let km = Keymap::defaults();
    for (mode, chord, action) in EXPECTED_DEFAULT_BINDINGS {
        let mode =
            Mode::from_name(mode).unwrap_or_else(|| panic!("no mode {mode:?} in the Rust keymap"));
        let binds = km
            .table()
            .get(&mode)
            .unwrap_or_else(|| panic!("mode {mode:?} has no binding table"));
        assert_eq!(
            binds.get(*chord).map(String::as_str),
            Some(*action),
            "default `{chord}` in mode {} should run {action}",
            mode.name()
        );
    }
    let bound: usize = km.table().values().map(|b| b.len()).sum();
    assert_eq!(
        bound,
        EXPECTED_DEFAULT_BINDINGS.len(),
        "the Rust table binds {bound} chords; the normative table has {}",
        EXPECTED_DEFAULT_BINDINGS.len()
    );
}

#[test]
fn the_unusable_chords_are_the_measured_ones() {
    assert_eq!(keymap::TERMINAL_UNUSABLE.len(), EXPECTED_UNUSABLE.len());
    for (chord, delivers, why) in EXPECTED_UNUSABLE {
        let found = keymap::TERMINAL_UNUSABLE
            .iter()
            .find(|u| u.chord == *chord)
            .unwrap_or_else(|| panic!("{chord} is missing from the Rust table"));
        assert_eq!(found.delivers, *delivers, "{chord} delivery changed");
        assert_eq!(found.why, *why, "{chord} reason changed");
        assert!(is_unusable(chord), "{chord} should be refused");
        assert!(
            unusable_reason(chord).is_some(),
            "{chord} should say why it is refused"
        );
    }
}

#[test]
fn the_three_canonical_tasks_are_the_adrs_three() {
    assert_eq!(keymap::TASKS.len(), EXPECTED_TASKS.len());
    for (task, (id, name, asks, steps)) in keymap::TASKS.iter().zip(EXPECTED_TASKS) {
        let task: &Task = task;
        assert_eq!((task.id, task.name), (*id, *name));
        assert_eq!(task.asks, *asks);
        let actual: Vec<(&str, &str, &str)> = task
            .steps
            .iter()
            .map(|s| (s.key, s.action, s.effect))
            .collect();
        assert_eq!(actual, steps.to_vec(), "task {id} steps changed");
    }
}

#[test]
fn the_hint_line_offers_exactly_the_adrs_eight_actions() {
    let rows: Vec<(String, String, String)> = adr_block("hint-line")
        .lines()
        .filter_map(|line| {
            let mut cells = line.split('|').skip(1).filter_map(|cell| {
                let cell = cell.trim();
                (!cell.is_empty() && cell != "Action" && !cell.starts_with("---"))
                    .then(|| cell.trim_matches('`').to_owned())
            });
            Some((cells.next()?, cells.next()?, cells.next()?))
        })
        .collect();
    assert_eq!(
        rows.iter().map(|(id, ..)| id.as_str()).collect::<Vec<_>>(),
        HINT_ACTIONS.to_vec(),
        "the hint line's action list is the ADR's table (ADR-006 §7.5)"
    );
    let km = Keymap::defaults();
    let line = km.hint_line();
    for (id, label, key) in &rows {
        assert_eq!(
            km.keys_for(id).split(' ').next().unwrap_or_default(),
            *key,
            "`{id}` should be offered under {key}"
        );
        assert_eq!(
            keymap::action(id).map_or("", |a| a.label),
            *label,
            "the hint line's label is the registry's, not a second copy"
        );
        assert!(
            line.contains(key.as_str()),
            "hint line should mention {key}: {line}"
        );
    }
}

// --- the acceptance criterion: an override changes behaviour, without recompiling ---------

#[test]
fn an_override_moves_a_key_to_a_different_action() {
    let km = Keymap::merged(&[bound(Mode::Global, "x", "view.timeline")]);
    assert_eq!(
        km.resolve(&chain(Mode::Queue), "x"),
        Some("view.timeline"),
        "the override must win over the default `x` = review.accept"
    );
    assert_eq!(km.bindings_of("review.accept"), Vec::new());
    assert!(
        km.palette_holes().contains(&"review.accept"),
        "the displaced action stays reachable from the palette (ADR-006 §7.0 K1)"
    );
    assert!(
        km.problems().is_empty(),
        "a legal override should not complain: {:?}",
        km.problems()
    );
    assert_eq!(km.applied().len(), 1);
}

#[test]
fn the_help_overlay_prints_the_overridden_key_not_the_default_one() {
    let km = Keymap::merged(&[bound(Mode::Global, "x", "view.timeline")]);
    // The cheatsheet is the same table as the overlay, one row per binding, tab-separated: the
    // honest way to ask "what does the documentation say `x` does" without grepping prose.
    let sheet: Vec<Vec<String>> = km
        .cheatsheet()
        .into_iter()
        .skip(1)
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect();
    let keys_for = |id: &str| -> Vec<String> {
        sheet
            .iter()
            .filter(|row| row.get(2).map(String::as_str) == Some(id))
            .filter_map(|row| row.get(1).cloned())
            .collect()
    };
    assert!(
        keys_for("view.timeline").contains(&"x".to_owned()),
        "the cheatsheet should show `x` for the action it was moved to: {:?}",
        keys_for("view.timeline")
    );
    assert!(
        !keys_for("review.accept").contains(&"x".to_owned()),
        "`x` no longer runs review.accept; the cheatsheet must not say it does: {:?}",
        keys_for("review.accept")
    );
    let overlay = km.help_lines(100).join("\n");
    assert!(
        overlay.contains("your overrides") && overlay.contains("x -> view.timeline"),
        "the overlay lists what was applied:\n{overlay}"
    );
    assert!(
        overlay.contains("(palette only)"),
        "the displaced action is shown as palette-only:\n{overlay}"
    );
    assert!(
        overlay.contains("global x -> view.timeline"),
        "the per-mode table must show the chord where the mode's other keys are:\n{overlay}"
    );
}

#[test]
fn binding_nothing_unbinds_and_the_overlay_says_so() {
    let km = Keymap::merged(&[unbound(Mode::Global, "x")]);
    assert_eq!(km.resolve(&chain(Mode::Queue), "x"), None);
    assert!(km.keys_for("review.accept").is_empty());
    assert!(km.help_lines(100).join("\n").contains("(palette only)"));
    assert!(km.applied().iter().any(|a| a == "unbind global x"));
}

#[test]
fn a_chord_that_cannot_arrive_is_refused_with_the_reason() {
    for chord in ["<C-i>", "<C-j>", "<C-m>", "<C-[>", "<C-@>"] {
        let km = Keymap::merged(&[bound(Mode::Global, chord, "view.graph")]);
        assert_eq!(
            km.resolve(&chain(Mode::Queue), chord),
            None,
            "{chord} must not become live"
        );
        let problems = km.problems().join("\n");
        assert!(
            problems.contains(chord) && problems.contains("cannot be bound"),
            "the refusal should name {chord} and say why:\n{problems}"
        );
    }
}

#[test]
fn an_action_that_does_not_exist_is_refused_with_the_name() {
    let km = Keymap::merged(&[bound(Mode::Global, "y", "review.deny_everything")]);
    assert_eq!(km.resolve(&chain(Mode::Queue), "y"), None);
    assert!(
        km.problems()
            .iter()
            .any(|p| p.contains("review.deny_everything") && p.contains("unknown action")),
        "{:?}",
        km.problems()
    );
}

#[test]
fn an_action_is_only_bindable_in_the_modes_that_declare_it() {
    // `diff.hunk_next` means nothing in the queue (ADR-006 §7.0 K2), so binding it globally is a
    // typo, reported instead of installed.
    let km = Keymap::merged(&[bound(Mode::Global, "y", "diff.hunk_next")]);
    assert_eq!(km.resolve(&chain(Mode::Queue), "y"), None);
    assert!(
        km.problems()
            .iter()
            .any(|p| p.contains("not bindable") && p.contains("diff.hunk_next")),
        "{:?}",
        km.problems()
    );
    // In the diff view's own mode the same override is exactly what a user might want.
    let km = Keymap::merged(&[bound(Mode::Diff, "y", "diff.hunk_next")]);
    assert_eq!(km.resolve(&chain(Mode::Diff), "y"), Some("diff.hunk_next"));
    assert!(km.problems().is_empty());
}

#[test]
fn a_view_mode_cannot_shadow_a_reserved_navigation_key() {
    // ADR-006 §7.2 K4: `p` inside the graph canvas must still mean "go to deps".
    let km = Keymap::merged(&[bound(Mode::Graph, "p", "view.queue")]);
    assert_eq!(
        km.resolve(&keymap::mode_chain(Mode::Graph, Mode::Body), "p"),
        Some("view.deps"),
        "the shadowing override must not be applied"
    );
    assert!(
        km.problems()
            .iter()
            .any(|p| p.contains("shadows the globally reserved key") && p.contains("'p'")),
        "{:?}",
        km.problems()
    );
    assert!(
        km.shadowed_keys()
            .iter()
            .any(|k| k.contains("graph") && k.contains("'p'")),
        "the conflict report is what `config get keys.overrides` prints"
    );
    // At global level the same key is the user's to take.
    let km = Keymap::merged(&[bound(Mode::Global, "p", "view.queue")]);
    assert_eq!(km.resolve(&chain(Mode::Graph), "p"), Some("view.queue"));
    assert!(km.problems().is_empty());
}

#[test]
fn precedence_is_interaction_then_view_then_global() {
    let km = Keymap::merged(&[
        bound(Mode::Global, "y", "view.graph"),
        bound(Mode::Queue, "y", "view.deps"),
    ]);
    // Nothing in `list` binds `y`, so the view mode answers.
    assert_eq!(km.resolve(&chain(Mode::Queue), "y"), Some("view.deps"));
    assert_eq!(km.resolve(&chain(Mode::Timeline), "y"), Some("view.graph"));
    assert_eq!(
        km.resolving_mode(&chain(Mode::Queue), "y"),
        Some(Mode::Queue)
    );
    // Add the same chord to the interaction mode and it wins: the focused pane owns the key.
    let km = Keymap::merged(&[
        bound(Mode::Global, "y", "view.graph"),
        bound(Mode::Queue, "y", "view.deps"),
        bound(Mode::List, "y", "review.flag"),
    ]);
    assert_eq!(km.resolve(&chain(Mode::Queue), "y"), Some("review.flag"));
    assert_eq!(
        km.resolving_mode(&chain(Mode::Queue), "y"),
        Some(Mode::List)
    );
    // An exclusive interaction mode replaces the chain instead of joining it (ADR-006 §7.1).
    let prompt = keymap::mode_chain(Mode::Queue, Mode::Prompt);
    assert_eq!(prompt, vec![Mode::Prompt]);
    assert_eq!(
        km.resolve(&prompt, "y"),
        None,
        "in the search field `y` is text, not an action"
    );
}

#[test]
fn a_view_mode_may_add_a_key_of_its_own() {
    let km = Keymap::merged(&[bound(Mode::Queue, "y", "review.flag")]);
    assert_eq!(km.resolve(&chain(Mode::Queue), "y"), Some("review.flag"));
    assert_eq!(
        km.resolve(&chain(Mode::Timeline), "y"),
        None,
        "the key belongs to the queue, so another view must not answer it"
    );
}

// --- the chord state machine ---------------------------------------------------------------

fn feed(state: &mut ChordState, km: &Keymap, view: Mode, chords: &[&str]) -> Vec<String> {
    let chain = chain(view);
    chords
        .iter()
        .flat_map(|chord| state.feed(km, &chain, chord))
        .map(|step| match step {
            KeyStep::Action(id) => id.to_owned(),
            KeyStep::Pending => "<pending>".to_owned(),
            KeyStep::Unbound(chord) => format!("<unbound {chord}>"),
        })
        .collect()
}

#[test]
fn a_prefix_waits_and_a_complete_chord_fires() {
    let km = Keymap::defaults();
    let mut state = ChordState::new();
    let chain = chain(Mode::Queue);
    let first = state.feed(&km, &chain, "<C-w>");
    assert_eq!(first.len(), 1);
    assert!(matches!(first[0], KeyStep::Pending), "`<C-w>` alone waits");
    assert!(state.is_pending());
    assert_eq!(state.pending(), "<C-w>");
    let second = state.feed(&km, &chain, "o");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].action(), Some("pane.zoom"));
    assert!(!state.is_pending());
}

#[test]
fn a_dead_end_flushes_the_prefix_as_its_own_key_and_keeps_the_new_one() {
    let km = Keymap::defaults();
    let mut state = ChordState::new();
    // `<C-w>` waits; `<C-w> y` is not a chord anybody meant, so the prefix is flushed as unbound
    // and `y` is then resolved on its own rather than swallowed.
    let out = feed(&mut state, &km, Mode::Queue, &["<C-w>", "y"]);
    assert_eq!(
        out,
        vec!["<pending>", "<unbound <C-w>y>"],
        "a dead-end chord is reported as the chord the user typed, not as two separate mistakes, \
         and nothing fires"
    );
    assert!(!state.is_pending());
}

#[test]
fn a_timeouted_prefix_resolves_alone_and_clears() {
    let km = Keymap::defaults();
    let chain = chain(Mode::Queue);
    let mut state = ChordState::new();
    state.feed(&km, &chain, "<C-w>");
    assert!(state.is_pending());
    assert_eq!(
        state.timeout(&km, &chain).action(),
        None,
        "`<C-w>` alone is bound to nothing, and the toast says so"
    );
    assert!(!state.is_pending());
    // A prefix that is *also* a binding of its own fires that binding when it times out: `g` is
    // `view.graph` and the first half of `gg`, and a user who types `g` and stops meant the graph.
    let mut state = ChordState::new();
    state.feed(&km, &chain, "g");
    assert_eq!(state.timeout(&km, &chain).action(), Some("view.graph"));
    assert!(!state.is_pending());
}

#[test]
fn a_double_tap_resolves_without_waiting_for_the_timeout() {
    let km = Keymap::defaults();
    let mut state = ChordState::new();
    let out = feed(&mut state, &km, Mode::Queue, &["g", "g"]);
    assert_eq!(out, vec!["<pending>", "list.first"]);
}

#[test]
fn two_keys_in_one_batch_flush_the_prefix_on_the_second_key_not_on_the_timer() {
    // The coalescing path: a held key arrives several per frame. The timer is only the fallback for
    // a user who stops mid-chord.
    let km = Keymap::defaults();
    let mut state = ChordState::new();
    let out = feed(&mut state, &km, Mode::Queue, &["<C-w>", "c"]);
    assert_eq!(out, vec!["<pending>", "tab.close"]);
}

#[test]
fn the_chord_timeout_is_a_fifth_of_a_second() {
    assert_eq!(CHORD_TIMEOUT, Duration::from_millis(250));
}

#[test]
fn chord_spellings_match_the_table_they_are_matched_against() {
    let plain = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    assert_eq!(chord_of(plain('g')), "g");
    assert_eq!(
        chord_of(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        "<Enter>"
    );
    assert_eq!(
        chord_of(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
        "<S-Tab>"
    );
    assert_eq!(
        chord_of(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        "<Esc>"
    );
    assert_eq!(
        chord_of(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        "<C-c>",
        "crossterm's raw mode delivers Ctrl-C as a byte, so this spelling has to match the table"
    );
    assert_eq!(
        chord_of(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)),
        "J",
        "the published table writes shifted letters bare (`J`, `K`, `G`), so the chord must spell \
         them that way too"
    );
    assert_eq!(
        chord_of(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)),
        "<C-Space>"
    );
}

#[test]
fn split_chords_splits_a_written_sequence_the_way_the_resolver_reads_it() {
    // A prefix chord absorbs the next atom whether or not the author spaced it, and a doubled bare
    // key is one chord — both are how the ADR writes sequences (`e1.1`'s `press` docs).
    assert_eq!(keymap::split_chords("<C-w>og"), vec!["<C-w>o", "g"]);
    assert_eq!(keymap::split_chords("<C-w> l"), vec!["<C-w>l"]);
    assert_eq!(keymap::split_chords("gg"), vec!["gg"]);
    assert_eq!(keymap::split_chords("g t"), vec!["g", "t"]);
    assert_eq!(keymap::split_chords(""), Vec::<String>::new());
}

#[test]
fn palette_only_actions_are_listed_as_such() {
    let km = Keymap::defaults();
    let holes = km.palette_holes();
    assert!(
        !holes.is_empty(),
        "the table has actions with no default key, and K1 says they must stay reachable"
    );
    for id in holes {
        assert!(
            keymap::action(id).is_some_and(|a| a.palette),
            "{id} is a palette hole but is not palette-listed"
        );
        assert!(km.keys_for(id).is_empty(), "{id} should have no key");
    }
    assert_eq!(
        km.keys_for("app.quit"),
        "<C-c>",
        "`app.quit` is the one chord a signal also delivers (ADR-006 §8.1)"
    );
}

#[test]
fn every_view_starts_in_a_mode_that_answers_the_shared_motion_set() {
    let km = Keymap::defaults();
    for view in ViewId::ALL {
        let id = format!("view.{}", format!("{view:?}").to_lowercase());
        if fathom_tui::views::view(view).kind == "view" {
            assert!(
                keymap::action(&id).is_some(),
                "no action id for view {view:?}"
            );
        }
        let list_chain = keymap::mode_chain(Mode::for_view(view), Mode::List);
        for motion in ["j", "k", "<Down>", "<Enter>"] {
            assert!(
                km.resolve(&list_chain, motion).is_some(),
                "{view:?} with a list focused should respond to {motion}"
            );
        }
        let body_chain = keymap::mode_chain(Mode::for_view(view), Mode::Body);
        assert!(
            km.resolve(&body_chain, "<Esc>").is_some(),
            "{view:?} with a body focused must answer <Esc> (drill.out)"
        );
    }
}
