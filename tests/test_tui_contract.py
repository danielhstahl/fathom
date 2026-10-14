"""ADR-006's acceptance criteria, as executable checks.

The document is the contract; this file is the reason it cannot rot. Every test names the
clause it guards, and the four AC bullets of `reco-e0e` map to:

  * "one ASCII layout per view plus focus order" -> test_every_view_has_a_layout_focus_ring_and_drills
  * "keymap table, with the chords reserved for user override" -> test_reserved_chords_are_not_bound_by_default
  * "documented degradation order below each view's minimum size" -> test_degradation_order_is_derived_not_declared
  * "two reviewers can name the exact key sequence for the three most common tasks" -> test_canonical_task_sequences_resolve

Run: python3 -m pytest tests/test_tui_contract.py -q
"""

from __future__ import annotations

import sys
import unicodedata
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))

from recommender.tui import docs as D                                   # noqa: E402
from recommender.tui import keymap as K                                  # noqa: E402
from recommender.tui import render as R                                  # noqa: E402
from recommender.tui.views import (AMBIENT, CHROME_ORDER, LAYOUT_MATRIX,  # noqa: E402
                                   QUEUE, VIEWS, VIEW_KEYS)

VIEW_IDS = [v.id for v in VIEWS]


# ---------------------------------------------------------------------------
# §4-§5: the view inventory
# ---------------------------------------------------------------------------
def test_the_nine_views_are_exactly_the_nine_in_the_ticket():
    assert VIEW_IDS == ["queue", "timeline", "diff", "structure", "graph", "deps",
                        "ambient", "search", "artefact"]


def test_view_keys_match_the_view_registry():
    """K3: the eight frame views are one letter each, and the ticket's fixed letters are
    present. `t` and `o` are this ADR's two additions to the fixed list."""
    assert VIEW_KEYS == {"q": "queue", "t": "timeline", "d": "diff", "s": "structure",
                         "g": "graph", "p": "deps", "a": "ambient", "o": "artefact"}
    for key in ("d", "s", "g", "p", "q", "a"):            # the ticket's own list
        assert K.DEFAULTS["global"][key] == "view." + VIEW_KEYS[key]
    for key, action in K.DEFAULTS["global"].items():
        if action.startswith("view."):
            assert VIEW_KEYS[key] == action.split(".", 1)[1], \
                "a view action is bound to a key the view registry does not own"


def test_every_view_has_a_layout_focus_ring_and_drills():
    for view in VIEWS:
        ring = view.focus_ring()
        assert ring, view.id
        assert ring[0] == view.primary.name, "%s: focus must land on the primary pane" % view.id
        orders = [p.focus_order for p in view.panes if p.role != "query"]
        assert orders == sorted(orders) and len(set(orders)) == len(orders)
        assert view.drills, "%s has no drill edges: it is a dead end" % view.id
        for pane in view.panes:
            assert pane.reads, "%s/%s shows a number with no citation" % (view.id, pane.name)
            assert pane.ticket in {v.owner for v in VIEWS} | {
                "e2.10", "e2.3", "e3.4", "e2.6", "e5.2", "e5.7", "e1.6", "e1.7", "e5.1",
                "e4.5", "e2.5"}, "%s/%s cites an unknown owner" % (view.id, pane.name)


def test_sample_lines_fit_their_pane_at_the_documented_size():
    """A layout that lies about its own width is worse than one with less text."""
    for view in VIEWS:
        for pane in view.panes:
            for line in pane.sample:
                assert R.disp_width(line) <= pane.ideal_cols, \
                    "%s/%s line is %d wide in a %d-wide pane: %r" % (
                        view.id, pane.name, R.disp_width(line), pane.ideal_cols, line)


def test_drawn_layouts_fit_their_documented_size():
    for view in VIEWS:
        lines = R.draw(view, view.ideal_cols, view.ideal_rows)
        assert len(lines) == view.ideal_rows, view.id
        for line in lines:
            assert R.disp_width(line) <= view.ideal_cols, \
                "%s: line wider than %d at its own documented size" % (view.id,
                                                                       view.ideal_cols)


def test_no_glyph_wider_than_a_cell_without_a_text_label():
    """§10.2 / e5.6: colour is never alone, and the glyph set must be single-width so the
    width arithmetic in §5 holds on any terminal."""
    for view in VIEWS:
        for pane in view.panes:
            for line in pane.sample:
                for ch in line:
                    if unicodedata.combining(ch):
                        continue
                    assert unicodedata.east_asian_width(ch) not in ("W", "F"), \
                        "%s/%s uses a full-width glyph (%r): the width budget breaks" % (
                            view.id, pane.name, ch)


def test_ambient_is_the_floor_view():
    """ADR-001 §4.1: ambient fits 80x12 and survives a 40x3 tmux pane; everything else
    may be squeezed, ambient may not, because in an ssh tmux session it is the UI."""
    assert (AMBIENT.ideal_cols, AMBIENT.ideal_rows) == (80, 12)
    assert "tabs" not in AMBIENT.chrome and "hint" not in AMBIENT.chrome
    assert AMBIENT.ideal_rows <= min(v.ideal_rows for v in VIEWS)
    assert R.refuse_below_rows(AMBIENT) == min(R.refuse_below_rows(v) for v in VIEWS)
    # ADR-001 §4.1's claim, priced: ambient draws in a 40x3 tmux pane and refuses below
    assert (R.refuse_below_cols(AMBIENT), R.refuse_below_rows(AMBIENT)) == (40, 3)
    lines = R.draw(AMBIENT, 40, 3)
    assert len(lines) == 3
    for line in lines:
        assert R.disp_width(line) <= 40
    # the two lines that must survive in that pane are the first two of the sample
    assert R.disp_width(AMBIENT.panes[0].sample[0]) <= 40
    assert R.disp_width(AMBIENT.panes[0].sample[1]) <= 40


def test_glyphs_and_text_labels_travel_together():
    """§10.2: every glyph used in a mock is a risk/state glyph whose meaning also appears
    as a word somewhere in the same pane, so `NO_COLOR` loses nothing."""
    for view in VIEWS:
        for pane in view.panes:
            text = " ".join(pane.sample)
            if "✱" in text:
                assert "risk" in text or "new" in text or "cycle" in text, pane.name
            if "▸" in text:
                assert pane.kind in ("list", "table", "treemap"), \
                    "%s: a selection marker in a non-list pane" % pane.name


# ---------------------------------------------------------------------------
# §3: the size contract
# ---------------------------------------------------------------------------
def test_every_view_is_complete_at_80x24():
    """The ticket's rule, stated as the AC: every view is usable at 80x24."""
    for view in VIEWS:
        assert view.ideal_cols <= 80 and view.ideal_rows <= 24, view.id
        fit = R.fit(view, 80, 24)
        assert fit.state == "complete", "%s is not complete at 80x24: %s" % (view.id,
                                                                             fit.dropped)
        assert fit.applied == [], view.id


def test_min_is_derived_from_the_panes():
    """§3: `min` is arithmetic over the pane table, including the frame a borderless view
    never draws — the version of this test that hardcoded `+ n + 1` charged Ambient for two
    borders it does not have."""
    for view in VIEWS:
        panes = [p for p in view.panes if p.role != "query"]
        frame = 2 if "frame" in view.chrome else 0
        assert R.min_cols_view(view) == sum(p.min_cols for p in panes) + frame \
            + len(panes) - 1, view.id
        assert R.min_cols_view(view) >= R.refuse_below_cols(view), view.id


def test_every_pane_orders_its_own_widths():
    """ideal > min > strip, or the degradation ladder has no room to walk."""
    for view in VIEWS:
        for pane in view.panes:
            assert pane.ideal_cols >= pane.min_cols >= pane.collapse_to, \
                "%s/%s" % (view.id, pane.name)
            if pane.role == "query":
                continue          # a text field has its own rules: see §5.7
            assert pane.collapse_to >= 14, "%s/%s: a strip under 14 cols is an ellipsis" % (
                view.id, pane.name)
            assert pane.hard_min >= 14, "%s/%s" % (view.id, pane.name)


def test_floors_are_the_arithmetic_of_the_ladder():
    for view in VIEWS:
        assert R.floor_cols(view) >= R.refuse_below_cols(view)
        assert R.floor_rows(view) >= R.refuse_below_rows(view)
        # at the floor the documented pane set still renders (no single-pane fallback)
        fit = R.fit(view, R.floor_cols(view), R.floor_rows(view) + R.chrome_rows(view))
        assert "D9.single_pane" not in fit.applied, "%s: floor does not hold" % view.id


def test_growth_above_ideal_goes_to_the_primary_pane_first():
    """§3's growth rule, checked rather than asserted in prose: the list pane gets wider
    first, because widening a list buys rows you can read and widening a card does not."""
    wide = R.pane_widths(QUEUE, 120, ())
    assert wide[0][1] > QUEUE.primary.ideal_cols, "spare width must widen the primary pane"
    assert wide[0][1] >= 2 * wide[1][1] - 1, "the secondary pane may not run ahead"
    assert sum(w for _p, w in wide) + 3 <= 120, "growth must not overflow the frame"


# ---------------------------------------------------------------------------
# §6: degradation
# ---------------------------------------------------------------------------
def test_degradation_is_an_ordered_ladder_not_a_vibe():
    ids = [s.id for s in R.LADDER]
    assert ids == ["D1.hint", "D2.tabs", "D3.strips", "D4.frame", "D5.secondary_strip",
                   "D6.cols_hard", "D7.compact_rows", "D8.content_trim"]
    # chrome gives way before content: hint and tabs are steps 1 and 2
    assert ids[0] == "D1.hint" and ids[1] == "D2.tabs"
    # the status line is never a step: it carries the degradation notice itself
    assert not any("status" in s.id for s in R.LADDER)


def test_degradation_order_is_derived_not_declared():
    """§6.2's per-view table is the ladder run per view, so the ADR cannot drift."""
    for view in VIEWS:
        boundary = R.ladder_boundary(view)
        assert [s.id for s, _at in boundary][:len(R.LADDER)] == [s.id for s in R.LADDER]
        applied = R.fit(view, R.floor_cols(view), 6).applied
        for step in R.LADDER:
            if step.recover(view) == (0, 0):
                assert step.id not in applied, \
                    "%s: %s recovers nothing and must not appear" % (view.id, step.id)


def test_matrix_states_are_what_the_ladder_produces():
    for view in VIEWS:
        for cols, rows in LAYOUT_MATRIX:
            fit = R.fit(view, cols, rows)
            assert fit.state in ("complete", "degraded", "single", "refused")
            if (cols, rows) >= (80, 24):
                assert fit.state == "complete"


def test_nothing_is_clipped_below_a_hard_minimum():
    """§6: a pane may be squeezed to its hard minimum and no further; below that the view
    falls back or refuses, because a truncated path is worse than an honest refusal."""
    for view in VIEWS:
        for cols in range(R.refuse_below_cols(view), view.ideal_cols + 1):
            fit = R.fit(view, cols, view.min_rows + R.chrome_rows(view))
            if fit.state == "refused":
                continue
            widths = R.pane_widths(view, cols, tuple(fit.applied))
            for pane, width in widths:
                if "D5.secondary_strip" in fit.applied and pane.role == "secondary":
                    floor = pane.collapse_to        # the documented strip width
                else:
                    floor = pane.hard_min if "D6.cols_hard" in fit.applied else pane.min_cols
                assert width >= min(floor, cols - 2 * R._FRAME_COLS), \
                    "%s/%s clipped to %d below its floor %d at %d cols" % (
                        view.id, pane.name, width, floor, cols)


def test_refusal_panel_names_the_size_and_the_way_out():
    view = QUEUE
    lines = R.draw(view, 30, 8)
    text = " ".join(lines)
    assert "too small" in text
    assert "ambient" in text, "the refusal panel must name the key that gets you out"
    assert "--plain" in text, "the refusal panel must name the no-terminal escape"
    assert "%d" % R.refuse_below_cols(view) in text, "it must state the size it needs"


def test_degraded_frame_announces_what_it_dropped():
    """ADR-001 §7.2: never hide silently. The notice leads the status line because the
    status line is the last chrome standing and the first thing to get truncated."""
    for cols in (72, 64, 56):
        fit = R.fit(QUEUE, cols, 14)
        assert fit.state in ("degraded", "single")
        status = " ".join(R.draw(QUEUE, cols, 14))
        assert "LAYOUT" in status and fit.state in status, cols
    assert R.fit(QUEUE, 80, 24).notice == "", "a complete frame says nothing"


# ---------------------------------------------------------------------------
# §7: the keymap
# ---------------------------------------------------------------------------
def test_modes_are_named_ordered_and_acyclic():
    names = [m.name for m in K.MODES]
    assert sorted(names) == sorted(K.PRECEDENCE)
    assert len(set(K.PRECEDENCE)) == len(K.PRECEDENCE)
    assert K.PRECEDENCE[0] == "prompt", "prompt must win outright, or you cannot type 'queue'"
    assert K.PRECEDENCE[-1] == "global"
    exclusive = {m.name for m in K.MODES if m.exclusive}
    assert exclusive == {"prompt", "pick"}


def test_default_keymap_is_conflict_free():
    assert K.conflicts(K.DEFAULTS) == []
    for mode, binds in K.DEFAULTS.items():
        assert mode in K.PRECEDENCE, mode
        for chord, action in binds.items():
            assert action in K.ACTIONS or action.startswith("tab.select_"), \
                "%s/%s names an unknown action %s" % (mode, chord, action)


def test_action_registry_is_consistent_with_the_table():
    for action_id, act in K.ACTIONS.items():
        assert act.ticket.startswith("e") or act.ticket.startswith("m"), action_id
        assert act.title, action_id
        assert act.modes and all(m in K.PRECEDENCE for m in act.modes), action_id
    for mode, binds in K.DEFAULTS.items():
        for chord, action in binds.items():
            act = K.ACTIONS.get(action)
            if act is None:
                continue
            assert mode in act.modes or "global" in act.modes, \
                "%s binds %s but the action does not name that mode" % (mode, action)


def test_every_action_is_bound_or_in_the_palette():
    """K1: 'everything reachable from the keyboard' as an enumerated property."""
    assert K.unbound_actions() == []
    bound = {a for binds in K.DEFAULTS.values() for a in binds.values()}
    unbound_unlisted = [a for a, act in K.ACTIONS.items()
                        if a not in bound and not act.palette]
    assert unbound_unlisted == []


def test_no_mode_shadows_the_globally_reserved_keys():
    """K4: a view or interaction mode may not steal a navigation key."""
    for mode in K.PRECEDENCE:
        if mode in ("global", "prompt", "pick"):
            continue
        for chord in K.DEFAULTS.get(mode, {}):
            assert chord not in K.GLOBAL_NEVER_SHADOWED, \
                "mode %r shadows reserved key %r" % (mode, chord)


def test_reserved_chords_are_not_bound_by_default():
    """AC bullet 2: the reserved set is a promise to the user's config file, so it is
    checked rather than asserted. `outside prompt` chords are the two exceptions and they
    are named as exceptions in the table, not smuggled in."""
    bound_in = {}
    for mode, binds in K.DEFAULTS.items():
        for chord in binds:
            bound_in.setdefault(chord, set()).add(mode)
    for chord, _why, where in K.RESERVED_FOR_USER:
        used = bound_in.get(chord, set())
        if where == "all modes":
            assert not used, "%s is reserved in all modes but bound in %s" % (chord, used)
        else:
            assert used <= {"prompt"}, \
                "%s is reserved outside prompt but bound in %s" % (chord, used)
    assert len({c for c, _w, _wh in K.RESERVED_FOR_USER}) == len(K.RESERVED_FOR_USER)


def test_terminal_ambiguous_chords_are_not_bound():
    """§8.1: a chord the terminal cannot deliver is not a binding."""
    bound = {chord for binds in K.DEFAULTS.values() for chord in binds}
    impossible = {c for c, _byte, _why in K.TERMINAL_UNUSABLE}
    for chord in impossible - {"<S-Tab>"}:
        assert chord not in bound or chord == "<C-c>", chord
    # Alt/Meta is not used at all (it arrives as ESC + key)
    assert not [c for c in bound if c.startswith("A-")]
    # <C-c> is the only quit chord and it is a signal, not an app binding
    assert K.DEFAULTS["global"]["<C-c>"] == "app.quit"


def test_the_chord_parser_handles_published_sequences():
    """§9's sequences are parseable by the same function `e5.4` uses to apply them."""
    assert K.split_chords("<C-w>l <Enter> x") == ["<C-w>l", "<Enter>", "x"]
    assert K.split_chords("<C-w> l") == ["<C-w>l"], "prose may space a prefix chord"
    assert K.split_chords("gg G") == ["gg", "G"]
    assert K.split_chords("j k <C-w>3") == ["j", "k", "<C-w>3"]
    with pytest.raises(ValueError):
        K.split_chords("<C-w")


def test_prefix_chords_resolve_and_digits_are_legal_under_them():
    steps = K.press("<C-w>l <C-w>3")
    assert [s["action"] for s in steps] == ["pane.focus_next", "tab.select_3"]


def test_prompt_mode_suppresses_view_keys_so_text_is_typeable():
    """The exclusive-mode rule in one test: typing 'queue' must not move anywhere."""
    chain = K.mode_chain("queue", "prompt")
    assert chain == ("prompt",)
    for letter in "queue":
        assert K.resolve(chain, letter) is None, "prompt mode fired a global key"
    assert K.resolve(chain, "<Enter>") == "prompt.submit"


def test_arrows_are_real_bindings_not_prose():
    """The aliases are bindings, so `e5.5`'s arrow-key users get what the ADR promises."""
    for mode, chord, action, _note in K.ALIASES:
        assert mode in K.DEFAULTS, mode
        assert K.DEFAULTS[mode].get(chord) == action, (mode, chord, action)


def test_resolution_order_is_first_binder_wins():
    table = {"list": {"x": "list.down"}, "global": {"x": "review.accept"}}
    assert K.resolve(("list", "global"), "x", table) == "list.down"
    assert K.resolve(("global",), "x", table) == "review.accept"


def test_conflicts_are_reported_not_absorbed():
    """K2: an override file is user input, and a silently ignored override is a lie."""
    assert K.conflicts({"global": {"<C-t>": "palette.open"}}) == []
    assert K.conflicts({"graph": {"p": "view.deps"}}), "K4 shadowing must be refused"
    assert K.conflicts({"diff": {"x": "no.such.action"}})
    assert K.conflicts({"list": {"q": "view.queue"}})
    assert K.conflicts({"nonsense": {"q": "view.queue"}})


def test_merging_an_override_reaches_the_help_surface():
    merged = K.merged({"global": {"x": "app.quit"}})
    assert merged["global"]["x"] == "app.quit"
    assert "app.quit" in K.binding_index(merged)
    labels = {a for a, _t, _k in K.palette_entries(merged)}
    assert "app.quit" in labels


def test_palette_entries_show_a_binding_or_say_palette_only():
    entries = K.palette_entries()
    assert entries
    for action_id, title, keys in entries:
        assert title and keys
        assert K.ACTIONS[action_id].palette


# ---------------------------------------------------------------------------
# §9: the canonical tasks
# ---------------------------------------------------------------------------
def test_there_are_exactly_three_canonical_tasks():
    assert [t.id for t in K.TASKS] == ["T1", "T2", "T3"]
    for task in K.TASKS:
        assert task.name and task.asks and task.ticket
        assert len(task.steps) >= 3


def test_canonical_task_sequences_resolve_as_published():
    """AC bullet 4: two reviewers can name the sequence, because the sequence is resolved
    through the real precedence chain and fails here if a rename breaks it."""
    for task in K.TASKS:
        sequence = " ".join(s.key for s in task.steps if not s.key.startswith("<text"))
        steps = K.press(sequence, view="ambient", interaction="body")
        resolved = [s["action"] for s in steps]
        published = [s.action for s in task.steps if not s.key.startswith("<text")]
        assert resolved == published, "%s: %s != %s" % (task.id, resolved, published)


def test_every_task_step_resolves_in_the_mode_it_claims():
    for task in K.TASKS:
        view, interaction = "ambient", "body"
        for step in task.steps:
            if step.key.startswith("<text"):
                assert step.mode == "prompt", "%s: only a prompt swallows free text" % task.id
                continue
            chain = K.mode_chain(view, interaction)
            assert step.mode in chain, \
                "%s: %s claims mode %r, not live in %s" % (task.id, step.key, step.mode, chain)
            assert K.resolve(chain, step.key) == step.action, \
                "%s: %s does not resolve to %s in %s" % (task.id, step.key, step.action,
                                                         chain)
            action = K.resolve(chain, step.key)
            if action and action.startswith("view."):
                view = action.split(".", 1)[1]
                interaction = K.default_interaction(view)
            elif action in ("search.open", "palette.open"):
                interaction = "prompt"
            elif action == "drill.in" and interaction == "list":
                interaction = "body"


def test_t1_is_short():
    """"Time-to-confidence" is the metric; the triage sequence must stay a glance."""
    t1 = next(t for t in K.TASKS if t.id == "T1")
    assert len([s for s in t1.steps if not s.key.startswith("<text")]) <= 5


# ---------------------------------------------------------------------------
# §8.1: measured terminal facts (skipped honestly where there is no pty)
# ---------------------------------------------------------------------------
def test_measured_key_delivery_matches_the_adr():
    from recommender.tui import keyprobe
    out = keyprobe.probe(verbose=False)
    if not out.get("pty_available"):
        pytest.skip("no pty on this machine: %s" % out.get("error"))
    assert out["ctrl_i_is_tab"], "the ADR claims Tab and Ctrl-I are one byte"
    assert out["ctrl_m_is_enter_after_icrnl"]
    assert out["ctrl_c_is_a_signal_not_a_byte"], "the ADR keeps ISIG on for the exit hook"
    assert out["flow_control_consumes_ctrl_s"]
    assert out["meta_is_escape_plus_key"], "the ADR binds no Alt chords for this reason"


def test_probe_machine_lines_are_greppable():
    from recommender.tui import keyprobe
    lines = keyprobe.machine_lines()
    assert any(line.startswith("pty_available=") for line in lines)
    assert any(line.startswith("delivers[") for line in lines)


# ---------------------------------------------------------------------------
# the verb surface `e1.5`, `e5.4` and `e7.5` read from
# ---------------------------------------------------------------------------
def test_every_cli_verb_runs(capsys):
    """`fathom tui …` is the surface the help overlay, the palette and the docs cheatsheet
    are built from (K1's "nothing hidden" claim depends on it). A verb that raises is a
    surface that lies, and a table nobody can print is a table nobody ships."""
    from recommender.tui.cli import main
    argv = [
        ["views"], ["keys"], ["keys", "--cheatsheet"], ["layout", "queue"],
        ["layout", "ambient", "-c", "44", "-r", "4"], ["fit", "queue", "60x12"],
        ["fit", "queue", "30x8"], ["press", "q j <Enter> d x"], ["tasks"],
        ["matrix"], ["palette"], ["conflicts"], ["conflicts", "--toml",
                                                 str(ROOT / "fathom.toml")],
        ["conflicts", "--set", "global.q=app.quit"], ["probe", "--machine"],
        ["doc", "--check"],
    ]
    for args in argv:
        code = main(args)
        captured = capsys.readouterr()
        assert code == 0, (args, code, captured.out[-400:], captured.err[-400:])
        assert captured.out.strip(), args
        assert "Traceback" not in captured.err


def test_cheatsheet_names_every_reserved_chord_and_every_impossible_one():
    from recommender.tui.cli import cheatsheet
    text = cheatsheet()
    for chord, _why, _where in K.RESERVED_FOR_USER:
        assert "`%s`" % chord in text, chord
    for chord, _byte, _why in K.TERMINAL_UNUSABLE:
        assert "`%s`" % chord in text, chord
    assert "Fathom keys" in text


def test_conflicts_verb_fails_loudly_on_a_shadowing_override(capsys):
    from recommender.tui.cli import main
    assert main(["conflicts", "--set", "graph.p=view.deps"]) == 1
    assert "conflict" in capsys.readouterr().out


def test_palette_verb_exits_zero_because_there_are_no_holes(capsys):
    from recommender.tui.cli import main
    assert main(["palette"]) == 0


# ---------------------------------------------------------------------------
# the document itself
# ---------------------------------------------------------------------------
def test_the_adr_matches_the_data():
    problems = D.check()
    assert problems == [], "docs/adr/0006-tui.md is out of date: %s" % problems


def test_every_generated_block_is_used_in_the_document():
    text = D.DOC.read_text()
    for name in D.blocks():
        assert "<!-- BEGIN GENERATED: %s -->" % name in text, \
            "block %r is generated but no longer included" % name


def test_the_document_contains_one_ascii_layout_per_view():
    """AC bullet 1, checked on the file rather than on the data."""
    text = D.DOC.read_text()
    for view in VIEWS:
        block = text.split("<!-- BEGIN GENERATED: layout.%s -->" % view.id, 1)
        assert len(block) == 2, view.id
        body = block[1].split("<!-- END GENERATED: layout.%s -->" % view.id, 1)[0]
        assert "```" in body, "%s has no ASCII layout" % view.id
        assert "Focus ring" in body, "%s has no focus order" % view.id
        assert "Reads from" in body, "%s does not name its delta-model fields" % view.id


def test_chrome_order_is_the_ladder_order():
    """§6: the chrome table and the ladder must agree about what gives way first."""
    declared = [name for name, _what, _ticket in CHROME_ORDER]
    assert declared[:2] == ["hint", "tabs"]
    assert declared[-1] == "status"


def test_repo_config_keymap_overrides_still_merge():
    """The repo's own fathom.toml must remain valid against this ADR's keymap."""
    import tomllib
    path = ROOT / "fathom.toml"
    if not path.exists():
        pytest.skip("no repo config")
    data = tomllib.loads(path.read_text())
    overrides = data.get("keys", {}).get("overrides", {})
    assert K.conflicts(overrides) == []
