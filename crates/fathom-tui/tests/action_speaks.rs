//! A key that cannot apply says why (`reco-xf8`, ADR-006 §10.3).
//!
//! The MVP walkthrough measured the frame, not the code: it pressed a key, redrew, and asked whether
//! anything was different. Eleven of its forty-one keystrokes produced no frame change at all — `y`,
//! `Y`, `<Enter>`, `x` and `i` over an empty rail — while `m` and `w` a few rows up the same table
//! answered in a sentence. That is not a missing feature, it is a rule applied unevenly, and the
//! uneven half is the half a reviewer reads as a broken key.
//!
//! The rule is ADR-006 §10.3's *a stated absence, never a blank*, in the place the ADR had not
//! spelled it out: not only does every pane have a named empty state, every **bound action** either
//! changes the frame or prints the reason it could not. "Nothing is selected, so there is no path to
//! copy" needs no store, no provider and no ticket — and a keystroke that cannot apply and says
//! nothing is indistinguishable from a keystroke the terminal lost.
//!
//! So this file is the rule as a measurement. It walks the shipped action registry against an empty
//! model in every view the shell ships — the walkthrough's walk, for every key rather than the five
//! it happened to try — and fails on any action that neither changed the drawn frame nor left a
//! sentence on the hint row. Three things keep the walk honest:
//!
//! * the exemption list is a table with a stated reason per row, and it is re-measured every run: an
//!   exempt id that has since learned to speak fails, so the list cannot outlive its reason;
//! * the refusals this ticket bought are held to the house shape — the absence, then the way out, in
//!   that order — which is `e5.9`'s rule for pane copy (`tests/empty_state.rs`) moved to the hint
//!   row. Naming the pane that is empty is half the sentence; naming the key that fills it is the
//!   half a reviewer can use;
//! * the sentences are checked in the states where they are the only feedback there is: an empty
//!   rail, a rail with rows in it, a chooser, a zoomed frame, and a one-pane view.
//!
//! One thing the walk deliberately does *not* do: re-police the copy of actions whose ticket has not
//! landed. `structure.sort_cycle` answering "owned by e2.3" is the ADR's own named refusal (ADR-006
//! §6.3 forbids a fake more strictly than it forbids a shrug), and rewriting every unbuilt action's
//! sentence is that ticket's work, not this one's.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

#[path = "timeline_corpus/mod.rs"]
mod tcorpus;

use std::time::Instant;

use fathom_tui::app::{App, Model, Overlay};
use fathom_tui::keymap::{ACTIONS, Keymap};
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::Batch;
use fathom_tui::views::ViewId;

use tcorpus::{NOW, mock};

/// The size the walkthrough used, and a size the hint row survives at (`e1.5`).
const SIZE: (u16, u16) = (120, 40);

/// The shell the walkthrough was holding: the shipped keymap, the façade's own model, no episodes,
/// no patch source, no search index, no producer. An empty rail is the state under test, not an
/// exception the shell was never meant to be in.
fn shell() -> App {
    App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "fathom/pilot · main",
    )
}

/// The drawn frame, trailing blanks trimmed.
fn frame(app: &App) -> Vec<String> {
    frame_lines(app)
        .iter()
        .map(|line| line.trim_end().to_owned())
        .collect()
}

/// The frame without its last row, which is the row a toast borrows.
///
/// A refusal is not allowed to be its own evidence: if printing the sentence counted as the visible
/// change, "the frame changed" would be true of every toast ever printed and the walk would measure
/// nothing. The frame *proper* has to move.
fn body(app: &App) -> Vec<String> {
    let mut lines = frame(app);
    let keep = lines.len().saturating_sub(1);
    lines.truncate(keep);
    lines
}

/// What one action owed, and what it paid.
struct Echo {
    /// Did the frame proper come out different?
    changed: bool,
    /// The sentence on the hint row, if any.
    toast: Option<String>,
    /// The registry row, so a failure names its owner.
    row: &'static fathom_tui::keymap::Action,
}

impl Echo {
    /// Did it answer, in either of the two languages a reviewer can read?
    fn spoke(&self) -> bool {
        self.changed || self.toast.is_some()
    }

    /// The complaint, shaped the same way everywhere the walk fails.
    fn complaint(&self, view: ViewId) -> String {
        format!(
            "{view:?} · {} ({}), owned by {} — the frame did not change and no reason was printed",
            self.row.id, self.row.label, self.row.ticket
        )
    }
}

/// Run one action against a fresh, empty shell in `view` and report what came back.
fn ask(view: ViewId, action: &'static fathom_tui::keymap::Action) -> Echo {
    let mut app = shell();
    app.set_view(view);
    let before = body(&app);
    app.dispatch(action.id);
    Echo {
        changed: body(&app) != before,
        toast: app.toast.clone(),
        row: action,
    }
}

/// The ids that act on the shell rather than on the frame, each with the reason it is not a gap.
///
/// A table rather than an `if`, because the set has to stay this small and this argued: an exemption
/// nobody had to justify is the old silence with paperwork on it.
const EXEMPT: &[(&str, &str)] = &[
    (
        "app.quit",
        "it ends the session — the frame it apparently failed to change is the one the terminal takes \
         back, and a toast about leaving would be read by nobody",
    ),
    (
        "screen.redraw",
        "an identical frame is the correct answer to `<C-l>`: the repaint is the action, and all the \
         loop is asked for is one more draw",
    ),
];

/// The exemption's reason, if this id has one.
fn exempt(id: &str) -> Option<&'static str> {
    EXEMPT
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, why)| *why)
}

/// The refusals this ticket is about, held to the house shape below.
///
/// The motion family (the rail, the worklist, every pane's `j`/`k`/`G`/`<C-d>`), the field family,
/// the focus and tab keys, the view keys pressed on the view that is already live, and the diff's
/// three ways out — that is, the keys that used to answer nothing at all. Other toasts belong to
/// their own tickets and are not rewritten here.
const POLICED: &[&str] = &[
    "list.down",
    "list.up",
    "list.first",
    "list.last",
    "list.top_visible",
    "list.mid_visible",
    "list.bot_visible",
    "list.page_down",
    "list.page_up",
    "list.half_down",
    "list.half_up",
    "list.scroll_down",
    "list.scroll_up",
    "prompt.backspace",
    "prompt.clear_line",
    "prompt.kill_line",
    "prompt.delete_word",
    "prompt.result_down",
    "prompt.result_up",
    "pane.focus_next",
    "pane.focus_prev",
    "drill.in",
    "diff.copy_path",
    "diff.copy_hunk",
    "diff.open_editor",
    "review.accept",
    "review.flag",
    "review.dismiss",
    "review.snooze",
    "intent.show",
    "compare.mark",
];

/// Is this id one of the refusals held to the house shape?
fn policed(id: &str) -> bool {
    POLICED.contains(&id) || id.starts_with("view.") || id.starts_with("tab.select_")
}

/// The house shape: the absence, then the way out, in that order, joined by an em dash — the same
/// shape `tests/empty_state.rs` requires of pane copy.
fn has_a_way_out(words: &str) -> bool {
    words.contains(" — ")
}

/// Press a chord the way the terminal would.
///
/// No assertion that it fired: half of what this file measures is a chord that is *not* bound in the
/// live mode chain, and the unbound path is one of the ways a keystroke is allowed to answer.
fn press(app: &mut App, chord: &str) {
    app.feed_key(chord, Instant::now());
}

/// The frame's answer to one chord: `(did the frame proper change, what did the hint row say)`.
fn chord(app: &mut App, what: &str) -> (bool, String) {
    app.clear_toast();
    let before = body(app);
    press(app, what);
    (body(app) != before, app.toast.clone().unwrap_or_default())
}

// --- 1. the registry walk ---------------------------------------------------------------------

#[test]
fn every_registered_action_changes_the_frame_or_says_why() {
    let mut gaps: Vec<String> = Vec::new();
    for view in ViewId::ALL.iter().copied() {
        for action in ACTIONS {
            let echo = ask(view, action);
            if echo.spoke() || exempt(action.id).is_some() {
                continue;
            }
            gaps.push(echo.complaint(view));
        }
    }
    assert!(
        gaps.is_empty(),
        "{} bound action(s) were silent over an empty model. ADR-006 §10.3: a stated absence, never \
         a blank — say what is missing and which key would end it:\n{}",
        gaps.len(),
        gaps.join("\n")
    );
}

#[test]
fn every_refusal_this_ticket_bought_ends_in_a_way_out() {
    let mut shrugs: Vec<String> = Vec::new();
    for view in ViewId::ALL.iter().copied() {
        for action in ACTIONS {
            if !policed(action.id) {
                continue;
            }
            let echo = ask(view, action);
            let Some(toast) = echo.toast else {
                continue; // it changed the frame, which is the other half of the rule
            };
            if has_a_way_out(&toast) {
                continue;
            }
            shrugs.push(format!("{view:?} · {} · {toast}", action.id));
        }
    }
    assert!(
        shrugs.is_empty(),
        "{} refusal(s) stated the absence and stopped there — the absence first, then the key or the \
         verb that ends it:\n{}",
        shrugs.len(),
        shrugs.join("\n")
    );
}

#[test]
fn the_exemption_table_is_still_true_and_still_needed() {
    for (id, why) in EXEMPT {
        let Some(action) = ACTIONS.iter().find(|row| row.id == *id) else {
            panic!(
                "{id} is on the exemption list but is not in the registry — the list is stale, and \
                    a stale exemption is a hole with paperwork"
            );
        };
        assert!(
            why.len() > 40,
            "{id}'s exemption has to be a reason: {why:?}"
        );
        let echo = ask(ViewId::Timeline, action);
        assert!(
            !echo.spoke(),
            "{id} now changes the frame or prints a reason — drop it from EXEMPT, which is only for \
             the ids that act on the shell and not on the frame"
        );
    }
}

// --- 2. the walkthrough's own keystrokes -------------------------------------------------------

/// The ticket's repro, as keystrokes. `y` and `Y` are the diff's keys, so they are pressed in the
/// pane that owns them, reached the way a reviewer reaches it: `d` over an empty rail. `<Enter>`, `x`
/// and `i` are pressed in the rail itself, which is empty.
#[test]
fn the_walkthroughs_five_keys_each_answer_for_themselves() {
    // (chord, where it was pressed, the word the answer has to be about)
    let table = [
        ("y", "diff", "selectable"),
        ("Y", "diff", "file"),
        ("<Enter>", "rail", ""),
        ("x", "rail", "selected"),
        ("i", "rail", "scope"),
    ];
    for (key, where_, word) in table {
        let mut app = shell();
        if where_ == "diff" {
            // `d` over an empty rail: the diff opens on its stated empty state, and says so.
            let (changed, toast) = chord(&mut app, "d");
            assert!(
                changed || !toast.is_empty(),
                "`d` over an empty rail was itself a silent key: {toast:?}"
            );
            assert_eq!(app.view, ViewId::Diff, "`d` opens the diff from the rail");
        }
        let (changed, toast) = chord(&mut app, key);
        assert!(
            changed || !toast.is_empty(),
            "`{key}` in the {where_} over an empty rail changed no frame and printed no reason"
        );
        if changed {
            continue; // `<Enter>` moves the focus, which is the change it owes
        }
        assert!(
            toast.contains(word),
            "`{key}` in the {where_} answered, but not about what was missing — wanted {word:?}, got \
             {toast:?}"
        );
        assert!(
            has_a_way_out(&toast),
            "`{key}` in the {where_} stated an absence with no way out: {toast:?}"
        );
    }
}

#[test]
fn an_unbound_key_names_the_modes_it_is_unbound_in() {
    // The other half of "silent": a chord nothing binds here. `y` is the diff's key; in the rail it
    // is unbound, and the unbound toast is what stops the keystroke reading as a lost one.
    let mut app = shell();
    app.set_view(ViewId::Timeline);
    let (_, toast) = chord(&mut app, "y");
    assert!(
        toast.contains("unbound") && toast.contains("list") && toast.contains("`?`"),
        "`y` in the rail said: {toast:?}"
    );
}

// --- 3. a cursor that cannot move -------------------------------------------------------------

/// A pane with `count` rows in it, handed over the way an embedder hands rows over, so the ends of a
/// list are testable without a store. The motion refusals are one code path either way.
fn with_rows(count: usize) -> App {
    let mut app = shell();
    app.set_view(ViewId::Deps);
    app.model.rows.insert(
        (ViewId::Deps, "manifest".to_owned()),
        (0..count).map(|row| format!("requirement {row}")).collect(),
    );
    app
}

#[test]
fn an_empty_list_names_the_key_that_would_fill_it() {
    let mut app = shell();
    app.set_view(ViewId::Timeline);
    let (changed, toast) = chord(&mut app, "j");
    assert!(!changed, "there is nothing for `j` to move over");
    assert!(
        toast.contains("rail") && toast.contains("no rows"),
        "`j` over an empty rail said: {toast:?}"
    );
    assert!(has_a_way_out(&toast), "{toast:?}");
}

#[test]
fn a_cursor_at_the_end_of_a_list_says_it_is_at_the_end() {
    let mut app = with_rows(4);
    press(&mut app, "G");
    assert_eq!(app.selected_row(), 3, "`G` went to the last row");
    let (_, toast) = chord(&mut app, "G");
    assert!(
        toast.contains("last row of the manifest pane") && toast.contains("4 row(s)"),
        "pressing `G` at the last row said: {toast:?}"
    );
    assert!(toast.contains("gg"), "and it named the way back: {toast:?}");
    press(&mut app, "gg");
    assert_eq!(app.selected_row(), 0, "and `gg` still works");
    let (_, toast) = chord(&mut app, "gg");
    assert!(
        toast.contains("first row of the manifest pane"),
        "the same key at the first row says so: {toast:?}"
    );
}

#[test]
fn a_pane_that_fits_the_frame_says_there_is_nothing_to_scroll() {
    let mut app = with_rows(3);
    // Three rows in a pane forty rows tall: the first jump moves the window as far as it goes, and
    // the next one has run out of pane and says so instead of redrawing the same three rows.
    press(&mut app, "<C-d>");
    let (changed, toast) = chord(&mut app, "<C-d>");
    assert!(!changed, "the window had already reached the bottom");
    assert!(
        toast.contains("nothing to scroll"),
        "a scroll with nothing left to scroll said: {toast:?}"
    );
}

// --- 4. focus, tabs and views already where they were -------------------------------------------

#[test]
fn a_one_pane_view_says_its_tab_has_nowhere_to_go() {
    let mut app = shell();
    app.set_view(ViewId::Ambient);
    let (changed, toast) = chord(&mut app, "<Tab>");
    assert!(!changed);
    assert!(
        toast.contains("one pane") && toast.contains("`?`"),
        "`<Tab>` in the one-pane ambient view said: {toast:?}"
    );
}

#[test]
fn a_zoomed_pane_says_which_key_undoes_the_zoom() {
    let mut app = shell();
    press(&mut app, "<C-w>o");
    let (_, toast) = chord(&mut app, "<Tab>");
    assert!(
        toast.contains("<C-w>o"),
        "`<Tab>` over a zoomed pane said: {toast:?}"
    );
}

#[test]
fn a_view_that_is_already_live_says_so() {
    let mut app = shell();
    press(&mut app, "t");
    let (_, toast) = chord(&mut app, "t");
    assert!(
        toast.contains("already the live view"),
        "the second `t` said: {toast:?}"
    );
    press(&mut app, "q");
    assert_eq!(app.view, ViewId::Queue, "`q` still switches");
}

#[test]
fn the_live_tab_says_it_is_the_live_tab() {
    let mut app = shell();
    let (changed, toast) = chord(&mut app, "<C-w>1");
    assert!(!changed, "one tab, and it is this one");
    assert!(
        toast.contains("tab 1 is already live") && toast.contains("`T`"),
        "{toast:?}"
    );
}

// --- 5. a field key with no field --------------------------------------------------------------

/// The field keys are bound only in `prompt` mode, so no chord reaches these refusals from a
/// terminal — which is exactly why they need testing: the palette dispatches by action id, an
/// embedder dispatches by action id, and a key whose answer is a silent `prompt.pop()` is the bug
/// this ticket was filed against wherever the id came from.
#[test]
fn a_field_key_with_no_field_open_names_the_fields() {
    for id in [
        "prompt.backspace",
        "prompt.clear_line",
        "prompt.kill_line",
        "prompt.delete_word",
        "prompt.result_down",
        "prompt.result_up",
    ] {
        let mut app = shell();
        app.set_view(ViewId::Timeline);
        app.clear_toast();
        let before = body(&app);
        app.dispatch(id);
        let toast = app.toast.clone().unwrap_or_default();
        assert_eq!(body(&app), before, "{id} must not move anything here");
        assert!(
            toast.contains("no field is open") && toast.contains("`/`"),
            "{id} with no field open said: {toast:?}"
        );
    }
}

#[test]
fn a_field_key_over_a_chooser_says_it_is_a_chooser() {
    let mut app = with_episodes();
    press(&mut app, "-");
    assert_eq!(app.overlay, Some(Overlay::Reason), "`-` opens the chooser");
    app.clear_toast();
    app.dispatch("prompt.backspace");
    let toast = app.toast.clone().unwrap_or_default();
    assert!(
        toast.contains("chooser") && toast.contains("<Enter>"),
        "a field key over the dismiss-reason chooser said: {toast:?}"
    );
}

#[test]
fn a_field_still_takes_its_keys_when_one_is_open() {
    // The refusals must not have eaten the field they guard: `/` opens, and the field edits.
    let mut app = shell();
    press(&mut app, "/");
    assert!(app.prompt_open(), "`/` opens the search field");
    press(&mut app, "s");
    press(&mut app, "t");
    press(&mut app, "<BS>");
    assert_eq!(
        app.prompt, "s",
        "`<BS>` deleted the character before the cursor"
    );
}

/// A rail with the fixture's episodes in it, so the chooser has something to act on.
fn with_episodes() -> App {
    let mut app = shell();
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", mock()));
    app.set_view(ViewId::Timeline);
    app
}

// --- 6. the acts, which already spoke, pinned so they keep doing it -----------------------------

#[test]
fn the_acts_over_an_empty_rail_name_the_motion_that_would_fill_it() {
    let mut app = shell();
    app.set_view(ViewId::Timeline);
    for key in ["x", "f", "-", "z", "i", "m"] {
        let (changed, toast) = chord(&mut app, key);
        assert!(
            !changed && has_a_way_out(&toast) && toast.contains("j"),
            "`{key}` over an empty rail said: {toast:?}"
        );
    }
}
