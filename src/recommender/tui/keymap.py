"""The canonical Fathom keymap: modes, actions, bindings, reserved chords and the
canonical task sequences. **This module is the normative table of ADR-006 §7**; the
markdown is rendered from it (`python3 -m recommender.tui doc --write`), because a
keymap restated by hand is a keymap that lies in the help overlay.

Three rules this module exists to make checkable (ADR-006 §7.0):

1. **Everything is reachable from the keyboard.** Every action is either bound in a
   mode or is a palette entry (`palette=True`) — and the palette is itself a keyboard
   surface, so "palette-only" is still keyboard-reachable. Views are bound globally,
   one letter each.
2. **The table is the only table.** `fathom.toml` carries `keys.overrides.<mode>.<key>`
   overrides *only* (ADR-002 §4.5, and e0.8's refusal of a `[keymap]` section); the
   help overlay, the cheatsheet (`e7.5`) and the palette labels all come from the
   merged table this module seeds.
3. **A chord the terminal cannot deliver is not a binding.** ADR-006 §8 states which
   chords are physically ambiguous (measured by `recommender.tui.keyprobe`) and which
   are *product-reserved* for user override. Both lists are data here, and the conflict
   checker refuses a default binding that collides with either.

Notation: a *chord* is one keystroke (`j`, `<C-d>`, `<Enter>`) or a two-keystroke
sequence written without a space (`<C-w>h` = Ctrl-W then `h`; `gg` = `g` twice).
"""

from __future__ import annotations

from dataclasses import dataclass


# ---------------------------------------------------------------------------
# modes
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Mode:
    """A keymap layer. Exactly one *interaction* mode and one *view* mode are live at a
    time, over the always-live `global` layer; `prompt`/`pick` are *exclusive* and
    replace all three (ADR-006 §7.1).
    """

    name: str
    kind: str            # "interaction" | "view" | "root"
    doc: str
    exclusive: bool = False


MODES: tuple[Mode, ...] = (
    Mode("prompt", "interaction",
         "A one-line text field owns the keyboard (search query, palette filter, "
         "reason text, path). Everything typed is text.", exclusive=True),
    Mode("pick", "interaction",
         "A single-key chooser is open (dismiss reason, snooze window, format). One "
         "keystroke chooses; there is no text entry.", exclusive=True),
    Mode("list", "interaction",
         "A list pane has focus: queue worklist, episode rail, file tree, dependency "
         "table, search results, treemap rows."),
    Mode("body", "interaction",
         "A content surface has focus: hunk view, episode card, graph canvas, "
         "artefact preview."),
    Mode("queue", "view", "The attention queue is the live view."),
    Mode("timeline", "view", "The timeline (episode rail + card) is the live view."),
    Mode("diff", "view", "The diff view is the live view."),
    Mode("structure", "view", "The structure treemap view is the live view."),
    Mode("graph", "view", "The module graph view is the live view."),
    Mode("deps", "view", "The dependency view is the live view."),
    Mode("artefact", "view", "The export-preview view is the live view."),
    Mode("ambient", "view", "Ambient mode is the live display mode."),
    Mode("global", "root",
         "Live whenever a non-exclusive mode is not holding the key. Owns view "
         "switching, overlays, review actions and the frame."),
)

#: Resolution order. The first mode in this list that binds a chord wins, so a
#: narrower mode always beats a broader one (ADR-006 §7.1).
PRECEDENCE: tuple[str, ...] = (
    "prompt", "pick", "list", "body",
    "queue", "timeline", "diff", "structure", "graph", "deps", "artefact", "ambient",
    "global",
)

#: The navigation keys. A narrower mode may **never** shadow one of these
#: (ADR-006 §7.2 rule K4); the checker enforces it, which is what lets a reviewer press
#: `p` inside the graph canvas and get the dependency view. `<Esc>` is deliberately not
#: in this list: drill-out is bound per mode, and `prompt`/`pick` own it outright.
GLOBAL_NEVER_SHADOWED: tuple[str, ...] = (
    "q", "t", "d", "s", "g", "p", "a", "o", "/", ",", "?",
)

#: Chords Fathom promises never to bind in v1, so a user may claim them through
#: `keys.overrides.*`. Each has a stated reason (ADR-006 §8.2).
#: chord -> (why it is free, where it is free). `all modes` means Fathom never touches
#: it; `outside prompt` means the text field uses it (readline muscle memory there beats
#: any personal binding) and every other mode leaves it alone.
RESERVED_FOR_USER: tuple[tuple[str, str, str], ...] = (
    ("<C-a>", "shell/multiplexer start-of-line, and screen's default prefix (the second "
              "prefix most tmux users bind)", "all modes"),
    ("<C-b>", "tmux's default prefix. Binding it here costs a user their whole "
              "multiplexer, so it stays free even though the terminal delivers it.",
     "all modes"),
    ("<C-e>", "`less`/vim scroll-one-line-down, left free with `<C-d>`/`<C-u>` already "
              "covering half-page", "all modes"),
    ("<C-g>", "readline abort — a good personal escape hatch", "all modes"),
    ("<C-k>", "readline kill-to-end-of-line: used inside the text field, free elsewhere",
     "outside prompt"),
    ("<C-n>", "readline next-history: walks results inside the text field, free elsewhere",
     "outside prompt"),
    ("<C-t>", "no convention worth honouring, so it is a good user chord", "all modes"),
    ("<C-v>", "readline literal-next, and the ubiquitous paste chord", "all modes"),
    ("<C-x>", "one of the most-requested personal chords in vim keymap threads; free",
     "all modes"),
    ("<C-6>", "vim's alternate-file", "all modes"),
    ("<S-F1>", "function keys are otherwise unbound in v1: their encodings vary more "
               "across terminals than any other group", "all modes"),
)

#: Chords that **cannot** be used as distinct bindings, with the measured reason. The
#: byte column is what a raw-mode reader actually receives, measured by
#: `python3 -m recommender.tui probe` (ADR-006 §8.1) rather than remembered from a man
#: page.
TERMINAL_UNUSABLE: tuple[tuple[str, str, str], ...] = (
    ("<C-i>", "one byte with `<Tab>` (0x09)",
     "The driver cannot distinguish them; `<Tab>` is bound, so `<C-i>` is not."),
    ("<C-j>", "one byte with LF (0x0a)",
     "LF is what `<Enter>` delivers in canonical mode, which is bound to drill."),
    ("<C-m>", "one byte with CR (0x0d), which the driver maps to LF",
     "So `<C-m>` and `<C-j>` arrive identical: both are `<Enter>`."),
    ("<C-h>", "one byte with <Backspace> (0x08, or 0x7f on some terminals)",
     "`<BS>` is bound to delete/back in `prompt`; `<C-h>` cannot be distinct from it."),
    ("<C-[>", "one byte with `<Esc>` (0x1b)",
     "`<Esc>` is the drill-out chord, so `<C-[>` is unusable by construction."),
    ("<C-@>", "NUL (0x00) — identical to `<C-Space>`",
     "Both spell NUL, so neither is bound and `<C-Space>` is left for users."),
    ("<C-c>", "no byte at all: SIGINT while `ISIG` is on",
     "Fathom keeps `ISIG` on so the exit/panic hook owns terminal restoration (`e1.5`), "
     "which makes `<C-c>` the only quit chord and never a rebinding target."),
    ("<C-z>", "no byte: SIGTSTP while `ISIG` is on", "Job control belongs to the shell."),
    ("<C-\\>", "no byte: SIGQUIT while `ISIG` is on", "Same reason as `<C-z>`."),
    ("A-<key>", "ESC + key: byte-identical to `<Esc>` then the key",
     "So **no Alt/Meta chord is bound in v1**. Two-keystroke chords (`<C-w>h`, `gg`) are "
     "the escape hatch and cost nothing to deliver."),
    ("<S-Tab>", "ESC [ Z — only if the terminal sends it",
     "Bound as an *alias* for `<Tab>`, with `<C-w>h` as the guaranteed route, so a "
     "hostile `TERM` loses a convenience and not a capability."),
)

#: Keys Fathom's terminal mode leaves as signals rather than bytes; measured, and the
#: reason `<C-c>` is the only quit key (ADR-006 §8.1).
SIGNAL_KEYS_KEEP_AS_SIGNAL: tuple[str, ...] = ("<C-c>", "<C-z>", "<C-\\>")


# ---------------------------------------------------------------------------
# actions
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Action:
    """An addressable command (`e5.4`): stable id, palette label, and the ticket that
    must implement it. `palette=True` means the action appears in the `,` palette,
    which is what makes an unbound action still keyboard-reachable.
    """

    id: str
    title: str
    scope: str
    ticket: str
    modes: tuple[str, ...] = ("global",)
    palette: bool = True
    doc: str = ""


def _a(id_, title, scope, ticket, modes=("global",), palette=True, doc=""):
    return Action(id_, title, scope, ticket, tuple(modes), palette, doc)


ACTIONS: dict[str, Action] = {a.id: a for a in (
    # --- view switching: one letter each (the ticket's fixed list) -------------
    _a("view.queue", "Go to: Queue", "view", "e4.5",
       doc="The home view: the ranked attention worklist (ADR-001 §4.1)."),
    _a("view.timeline", "Go to: Timeline", "view", "e1.6",
       doc="Chronological episode rail plus episode card."),
    _a("view.diff", "Go to: Diff", "view", "e1.7",
       doc="File tree plus hunk view for the live scope."),
    _a("view.structure", "Go to: Structure", "view", "e2.3",
       doc="Directory treemap with churn heat."),
    _a("view.graph", "Go to: Graph", "view", "e3.4",
       doc="Module graph with the per-episode delta overlay."),
    _a("view.deps", "Go to: Deps", "view", "e2.6",
       doc="Manifest table plus transitive tree."),
    _a("view.ambient", "Go to: Ambient", "view", "e5.1",
       doc="Glanceable display mode for the second terminal."),
    _a("view.artefact", "Go to: Artefact", "view", "e5.7",
       doc="Export preview of the current scope."),
    # --- overlays -------------------------------------------------------------
    _a("search.open", "Search: fuzzy over episodes, paths, symbols, deps, findings",
       "overlay", "e5.2", doc="Opens the search overlay in `prompt` mode."),
    _a("palette.open", "Command palette", "overlay", "e5.4",
       doc="Every addressable command, with its current binding shown."),
    _a("help.open", "Help: this keymap as the merged table", "overlay", "e1.5",
       doc="Generated from the merged keymap, so an override shows up here (e1.5 AC)."),
    _a("overlay.close", "Close overlay", "overlay", "e1.5", palette=False,
       modes=("prompt", "pick")),
    # --- motion: one implementation, every list (e5.4's AC) -------------------
    _a("list.down", "Move down one row", "motion", "e1.5", modes=("list", "body", "pick")),
    _a("list.up", "Move up one row", "motion", "e1.5", modes=("list", "body", "pick")),
    _a("list.page_down", "Page down", "motion", "e1.5", modes=("list", "body")),
    _a("list.page_up", "Page up", "motion", "e1.5", modes=("list", "body")),
    _a("list.half_down", "Half page down", "motion", "e1.5", modes=("list", "body")),
    _a("list.half_up", "Half page up", "motion", "e1.5", modes=("list", "body")),
    _a("list.top_visible", "First visible row", "motion", "e5.4", modes=("list",)),
    _a("list.mid_visible", "Middle visible row", "motion", "e5.4", modes=("list",)),
    _a("list.bot_visible", "Last visible row", "motion", "e5.4", modes=("list",)),
    _a("list.first", "First row", "motion", "e1.5", modes=("list", "body")),
    _a("list.last", "Last row", "motion", "e1.5", modes=("list", "body")),
    _a("list.expand", "Expand / collapse node", "motion", "e1.7", modes=("list",)),
    _a("list.next_match", "Next match", "motion", "e5.2", modes=("global",)),
    _a("list.prev_match", "Previous match", "motion", "e5.2", modes=("global",)),
    _a("list.scroll_down", "Scroll one line down", "motion", "e5.5", modes=("body",),
       palette=False),
    _a("list.scroll_up", "Scroll one line up", "motion", "e5.5", modes=("body",),
       palette=False),
    # --- focus, drill, frame --------------------------------------------------
    _a("drill.in", "Drill in (follow selection / evidence)", "frame", "e1.5",
       modes=("list", "body")),
    _a("drill.out", "Up one level (pop the drill stack)", "frame", "e1.5",
       modes=("list", "body")),
    _a("pane.focus_next", "Focus next pane", "frame", "e1.5"),
    _a("pane.focus_prev", "Focus previous pane", "frame", "e1.5"),
    _a("pane.zoom", "Zoom focused pane (fill the frame)", "frame", "e5.5"),
    _a("history.back", "Back to the previous place (drill history)", "frame", "e1.5"),
    _a("tab.next", "Next tab", "frame", "e1.5"),
    _a("tab.prev", "Previous tab", "frame", "e1.5"),
    _a("tab.pin", "Pin the current view and scope as a tab", "frame", "e1.5"),
    _a("tab.close", "Close the current tab", "frame", "e1.5"),
    # --- review actions: act on the live scope (ADR-001 §4.5 dispositions) ----
    _a("review.accept", "Mark reviewed (accept)", "review", "e4.5",
       doc="Disposition `accepted` + identity + time to the ledger (`e4.6`)."),
    _a("review.flag", "Flag for attention, with a note", "review", "e4.5"),
    _a("review.dismiss", "Dismiss with a reason", "review", "e4.5",
       doc="Opens the closed dismiss-vocabulary chooser (`pick`)."),
    _a("review.snooze", "Snooze until a condition", "review", "e4.5",
       doc="Opens the snooze-window chooser (`pick`)."),
    _a("risk.explain", "Why this score / this rank (`fathom why`)", "review", "e4.4"),
    _a("queue.retrospect", "Queue: what you skipped, and why", "review", "e4.5",
       modes=("queue",),
       doc="Toggles the worklist pane between the ranked queue and the dismissed-with-reason "
           "rollup, so systematic dismissing is a number on a screen."),
    _a("intent.show", "Show captured intent and its source", "review", "e4.2"),
    _a("pick.choose", "Choose this entry", "overlay", "e4.5", modes=("pick",),
       palette=False),
    _a("prompt.submit", "Submit", "overlay", "e5.2", modes=("prompt",), palette=False),
    _a("prompt.result_down", "Next result", "overlay", "e5.2", modes=("prompt",),
       palette=False),
    _a("prompt.result_up", "Previous result", "overlay", "e5.2", modes=("prompt",),
       palette=False),
    _a("prompt.delete_word", "Delete word", "overlay", "e5.4", modes=("prompt",),
       palette=False),
    _a("prompt.kill_line", "Kill to end of line", "overlay", "e5.4", modes=("prompt",),
       palette=False),
    _a("prompt.clear_line", "Clear the line", "overlay", "e5.4", modes=("prompt",),
       palette=False),
    _a("prompt.backspace", "Delete one character", "overlay", "e5.4", modes=("prompt",),
       palette=False),
    # --- scope, filtering, comparison ---------------------------------------
    _a("filter.edit", "Edit filters (author, area, state, date)", "scope", "e1.6"),
    _a("filter.clear", "Clear all filters", "scope", "e1.6"),
    _a("compare.mark", "Mark this endpoint for comparison", "scope", "e5.3"),
    _a("compare.with_mark", "Compare the live scope with the mark", "scope", "e5.3"),
    _a("goto.first_unread", "Jump to first unread episode", "scope", "e1.6",
       palette=True),
    _a("goto.today", "Jump to today", "scope", "e1.6", palette=True),
    # --- view-local content controls (bound only inside their own view mode) --
    _a("diff.split_toggle", "Diff: split / unified", "view", "e1.7", modes=("diff",)),
    _a("diff.word_toggle", "Diff: word-diff on / off", "view", "e1.7", modes=("diff",)),
    _a("diff.ws_toggle", "Diff: ignore whitespace", "view", "e1.7", modes=("diff",)),
    _a("diff.noise_toggle", "Diff: show suppressed noise (the rule is printed)",
       "view", "e2.2", modes=("diff",)),
    _a("diff.hunk_next", "Diff: next hunk", "view", "e1.7", modes=("diff",)),
    _a("diff.hunk_prev", "Diff: previous hunk", "view", "e1.7", modes=("diff",)),
    _a("diff.copy_path", "Diff: copy path", "view", "e5.8", modes=("diff",)),
    _a("diff.copy_hunk", "Diff: copy hunk", "view", "e5.8", modes=("diff",)),
    _a("diff.open_editor", "Diff: open file at line in $EDITOR", "view", "e5.8",
       modes=("diff",)),
    _a("structure.level_up", "Structure: aggregate one level up", "view", "e2.3",
       modes=("structure",)),
    _a("structure.level_down", "Structure: drill one level down", "view", "e2.3",
       modes=("structure",)),
    _a("structure.sort_cycle", "Structure: sort by churn / size / delta", "view",
       "e2.3", modes=("structure",)),
    _a("graph.level_up", "Graph: aggregate one level up", "view", "e3.4",
       modes=("graph",)),
    _a("graph.level_down", "Graph: aggregate one level down", "view", "e3.4",
       modes=("graph",)),
    _a("graph.hop_less", "Graph: one hop closer", "view", "e3.4", modes=("graph",)),
    _a("graph.hop_more", "Graph: one hop further", "view", "e3.4", modes=("graph",)),
    _a("graph.layout_cycle", "Graph: cycle layout (layered / circular)", "view",
       "e3.4", modes=("graph",)),
    _a("graph.overlay_only", "Graph: show only this episode's delta overlay", "view",
       "e3.4", modes=("graph",)),
    _a("deps.ecosystem_next", "Deps: next ecosystem", "view", "e2.5", modes=("deps",)),
    _a("deps.ecosystem_prev", "Deps: previous ecosystem", "view", "e2.5",
       modes=("deps",)),
    _a("deps.transitive_toggle", "Deps: show transitive-only changes", "view", "e2.6",
       modes=("deps",)),
    _a("artefact.format_cycle", "Artefact: cycle format (md / html / json)", "view",
       "e5.7", modes=("artefact",)),
    _a("artefact.scope_widen", "Artefact: widen the episode range", "view", "e5.7",
       modes=("artefact",)),
    _a("artefact.scope_narrow", "Artefact: narrow the episode range", "view", "e5.7",
       modes=("artefact",)),
    _a("artefact.write", "Artefact: write the export now", "view", "e5.7",
       modes=("artefact",)),
    # --- application and index ---------------------------------------------
    _a("index.reindex", "Re-index this repository", "index", "e1.4"),
    _a("screen.redraw", "Repaint the screen", "frame", "e1.5"),
    _a("app.quit", "Quit Fathom", "frame", "e1.5",
       doc="`<C-c>`: SIGINT with the terminal-restore hook. The only quit chord."),
)}

#: Tab selection actions exist for palette completeness; the binding is `<C-w>1..9`.
for _n in range(1, 10):
    ACTIONS["tab.select_%d" % _n] = Action(
        id="tab.select_%d" % _n, title="Go to tab %d" % _n, scope="frame", ticket="e1.5",
        modes=("global",), palette=(_n <= 3), doc="`<C-w>%d`." % _n)

#: Two-key chords under `<C-w>`, the frame prefix (tmux/vim convention). Digits are
#: legal here because a prefix chord consumes its own count (`e5.4`).
PREFIX_C_W: tuple[str, ...] = ("h", "l", "n", "p", "o", "c", "1", "2", "3", "4", "5",
                               "6", "7", "8", "9")


#: mode -> chord -> action id. The normative default table (ADR-006 §7.3).
DEFAULTS: dict[str, dict[str, str]] = {
    # --- the root layer: reachable from everywhere except prompt/pick ---------
    "global": {
        "q": "view.queue",
        "t": "view.timeline",
        "d": "view.diff",
        "s": "view.structure",
        "g": "view.graph",
        "p": "view.deps",
        "a": "view.ambient",
        "o": "view.artefact",
        "/": "search.open",
        ",": "palette.open",
        "?": "help.open",
        "x": "review.accept",
        "f": "review.flag",
        "-": "review.dismiss",
        "z": "review.snooze",
        "w": "risk.explain",
        "i": "intent.show",
        "m": "compare.mark",
        "c": "compare.with_mark",
        "F": "filter.edit",
        "n": "list.next_match",
        "N": "list.prev_match",
        "J": "tab.prev",
        "K": "tab.next",
        "T": "tab.pin",
        "<C-r>": "index.reindex",
        "<C-l>": "screen.redraw",
        "<C-o>": "history.back",
        "<C-c>": "app.quit",
        "<C-w>h": "pane.focus_prev",
        "<C-w>l": "pane.focus_next",
        "<C-w>n": "tab.next",
        "<C-w>p": "tab.prev",
        "<C-w>o": "pane.zoom",
        "<C-w>c": "tab.close",
        "<C-w>1": "tab.select_1",
        "<C-w>2": "tab.select_2",
        "<C-w>3": "tab.select_3",
        "<C-w>4": "tab.select_4",
        "<C-w>5": "tab.select_5",
        "<C-w>6": "tab.select_6",
        "<C-w>7": "tab.select_7",
        "<C-w>8": "tab.select_8",
        "<C-w>9": "tab.select_9",
    },
    # --- shared list motion ---------------------------------------------------
    "list": {
        "j": "list.down",
        "k": "list.up",
        "<Down>": "list.down",
        "<Up>": "list.up",
        "H": "list.top_visible",
        "M": "list.mid_visible",
        "L": "list.bot_visible",
        "gg": "list.first",
        "G": "list.last",
        "h": "drill.out",
        "l": "drill.in",
        "<C-d>": "list.half_down",
        "<C-u>": "list.half_up",
        "<C-f>": "list.page_down",
        "<PageDown>": "list.page_down",
        "<PageUp>": "list.page_up",
        "<Space>": "list.expand",
        "<Enter>": "drill.in",
        "<Right>": "drill.in",
        "<Esc>": "drill.out",
        "<Left>": "drill.out",
        "<Tab>": "pane.focus_next",
        "<S-Tab>": "pane.focus_prev",
    },
    # --- a focused content surface (hunk, card, canvas, preview) --------------
    "body": {
        "j": "list.scroll_down",
        "k": "list.scroll_up",
        "<Down>": "list.scroll_down",
        "<Up>": "list.scroll_up",
        "<C-d>": "list.half_down",
        "<C-u>": "list.half_up",
        "<C-f>": "list.page_down",
        "<PageDown>": "list.page_down",
        "<PageUp>": "list.page_up",
        "gg": "list.first",
        "G": "list.last",
        "h": "drill.out",
        "l": "drill.in",
        "<Enter>": "drill.in",
        "<Right>": "drill.in",
        "<Esc>": "drill.out",
        "<Left>": "drill.out",
        "<Tab>": "pane.focus_next",
        "<S-Tab>": "pane.focus_prev",
    },
    # --- the `/` and `,` text field: everything typed is text -----------------
    "prompt": {
        "<Enter>": "prompt.submit",
        "<Esc>": "overlay.close",
        "<BS>": "prompt.backspace",
        "<C-w>": "prompt.delete_word",
        "<C-u>": "prompt.clear_line",
        "<C-k>": "prompt.kill_line",
        "<C-n>": "prompt.result_down",
        "<C-p>": "prompt.result_up",
        "<Down>": "prompt.result_down",
        "<Up>": "prompt.result_up",
        "<Tab>": "prompt.result_down",
    },
    # --- the reason / window chooser ------------------------------------------
    "pick": {
        "<Enter>": "pick.choose",
        "<Esc>": "overlay.close",
        "j": "list.down",
        "k": "list.up",
        "<Down>": "list.down",
        "<Up>": "list.up",
    },
    # --- view-local content controls, on keys the root layer leaves free ------
    "diff": {
        "\\": "diff.split_toggle",
        "W": "diff.word_toggle",
        "=": "diff.ws_toggle",
        "b": "diff.noise_toggle",
        "[": "diff.hunk_prev",
        "]": "diff.hunk_next",
        "y": "diff.copy_path",
        "Y": "diff.copy_hunk",
        "E": "diff.open_editor",
    },
    "structure": {
        "[": "structure.level_up",
        "]": "structure.level_down",
        "<": "structure.sort_cycle",
    },
    "graph": {
        "[": "graph.level_up",
        "]": "graph.level_down",
        "<": "graph.hop_less",
        ">": "graph.hop_more",
        "L": "graph.layout_cycle",
        "!": "graph.overlay_only",
    },
    "deps": {
        "[": "deps.ecosystem_prev",
        "]": "deps.ecosystem_next",
        "u": "deps.transitive_toggle",
    },
    "artefact": {
        "=": "artefact.format_cycle",
        "[": "artefact.scope_narrow",
        "]": "artefact.scope_widen",
        "<Enter>": "artefact.write",
    },
    # Views whose whole job is a list plus a card add nothing of their own: the
    # shared motion set *is* the whole keymap, which is what "identical motion
    # behaviour across timeline, queue, file tree and search results" (e5.4) demands.
    "queue": {
        "r": "queue.retrospect",
    },
    "timeline": {},
    "ambient": {},
}

#: Aliases are bindings, not prose, so the cheatsheet prints them and the checker
#: counts them: "the arrows work everywhere" becomes a checked fact.
ALIASES: tuple[tuple[str, str, str, str], ...] = (
    ("list", "<Down>", "list.down", "same as `j`"),
    ("list", "<Up>", "list.up", "same as `k`"),
    ("list", "<Right>", "drill.in", "same as `<Enter>`"),
    ("list", "<Left>", "drill.out", "same as `<Esc>`"),
    ("body", "<Down>", "list.scroll_down", "same as `j`"),
    ("body", "<Up>", "list.scroll_up", "same as `k`"),
    ("body", "<Right>", "drill.in", "same as `<Enter>`"),
    ("body", "<Left>", "drill.out", "same as `<Esc>`"),
    ("prompt", "<Down>", "prompt.result_down", "same as `<C-n>`"),
    ("prompt", "<Up>", "prompt.result_up", "same as `<C-p>`"),
    ("prompt", "<Tab>", "prompt.result_down", "same as `<C-n>`"),
    ("pick", "<Down>", "list.down", "same as `j`"),
    ("pick", "<Up>", "list.up", "same as `k`"),
)


# ---------------------------------------------------------------------------
# canonical task sequences (ADR-006 §9)
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class TaskStep:
    key: str
    action: str
    effect: str
    mode: str = ""


@dataclass(frozen=True)
class Task:
    id: str
    name: str
    asks: str
    ticket: str
    steps: tuple[TaskStep, ...]
    note: str = ""


#: The three things the accountable human does most, fixed here so the docs, the hint
#: line (`e5.4`) and the cheatsheet (`e7.5`) all quote the same keys. A test resolves
#: every step through the real precedence chain and fails if a rename breaks a
#: published sequence.
TASKS: tuple[Task, ...] = (
    Task(
        id="T1",
        name="Triage the top of the queue and sign it off",
        asks="What is the next thing I have to look at, and am I happy with it?",
        ticket="e4.5",
        steps=(
            TaskStep("q", "view.queue",
                     "Home. Focus lands on the worklist, ranked by risk (pane 1).",
                     "global"),
            TaskStep("j", "list.down",
                     "Move down the worklist. `H`/`M`/`L` and `<C-d>`/`<C-u>` too.",
                     "list"),
            TaskStep("<Enter>", "drill.in",
                     "Drill into the episode card (pane 2): what changed, where, blast "
                     "radius, every figure a button.", "list"),
            TaskStep("d", "view.diff",
                     "Same scope, diff view: file tree first, `j` then `<Enter>` to "
                     "read a file's hunks.", "global"),
            TaskStep("x", "review.accept",
                     "Mark reviewed: disposition `accepted` + identity + time to the "
                     "ledger; the queue advances.", "global"),
        ),
        note="`f` flags instead, `-` dismisses with a reason from the closed vocabulary, "
             "`z` snoozes, `w` prints the ranking the way `fathom why` does.",
    ),
    Task(
        id="T2",
        name="Read what the newest turn did, and whether the shape moved",
        asks="The agent has been busy: what landed, and did the architecture get worse?",
        ticket="e2.10",
        steps=(
            TaskStep("a", "view.ambient",
                     "Glance mode: latest episode, its risk, queue count, index state.",
                     "global"),
            TaskStep("<Enter>", "drill.in",
                     "Drill into that episode's card: one keypress, per ADR-001 §4.1.",
                     "body"),
            TaskStep("s", "view.structure",
                     "Where it landed: treemap with churn heat; `[`/`]` move the "
                     "aggregation level.", "global"),
            TaskStep("g", "view.graph",
                     "Module graph with this episode's delta overlay; `!` shows only "
                     "the delta, `[`/`]` aggregate, `<`/`>` set the hop limit.",
                     "global"),
            TaskStep("<Esc>", "drill.out",
                     "Up one level, then again to ambient. Nothing was lost on the way.",
                     "body"),
        ),
        note="`p` for the dependency delta, `o` for what the export would say.",
    ),
    Task(
        id="T3",
        name="Find a thing and land on it",
        asks="Where is `parse_lockfile`, who touched `Cargo.lock`, what flagged this?",
        ticket="e5.2",
        steps=(
            TaskStep("/", "search.open",
                     "Search overlay, cursor already in the query field.", "global"),
            TaskStep("<text>", "(typing)",
                     "Fuzzy over episodes, paths, symbols, dependencies and findings; "
                     "`author:` `path:` `risk>` `dep:` tokens compose with text.",
                     "prompt"),
            TaskStep("<C-n>", "prompt.result_down",
                     "Walk results without leaving the field; `<C-p>` walks back.",
                     "prompt"),
            TaskStep("<Enter>", "prompt.submit",
                     "Jump into the right view with the right selection: a deep link, "
                     "not a list of paths.", "prompt"),
        ),
        note="`,` is the same mechanism for *commands* instead of content.",
    ),
)

#: The hint line (`e5.4`) may advertise only these, in this order.
HINT_ACTIONS: tuple[str, ...] = (
    "view.queue", "view.timeline", "view.diff", "search.open", "palette.open",
    "review.accept", "risk.explain", "help.open",
)


# ---------------------------------------------------------------------------
# the engine (`e1.5`/`e5.4` implement this, they do not reinvent it)
# ---------------------------------------------------------------------------
def mode_chain(view: str, interaction: str) -> tuple[str, ...]:
    """The live modes for (view, interaction), in precedence order.

    An exclusive mode (`prompt`, `pick`) replaces the whole chain: while a text field
    has the keyboard, `q` is the letter q. This is what makes `queue` typeable in the
    palette and is the reason `prompt` sits at the head of :data:`PRECEDENCE`.
    """
    modes = {m.name: m for m in MODES}
    if interaction in modes and modes[interaction].exclusive:
        return (interaction,)
    return tuple(m for m in PRECEDENCE
                 if m not in ("prompt", "pick") and m in (view, interaction, "global"))


def resolve(chain: tuple[str, ...], chord: str,
            table: dict[str, dict[str, str]] | None = None) -> str | None:
    """First mode in the chain that binds `chord` wins (ADR-006 §7.1)."""
    table = table if table is not None else DEFAULTS
    for mode in chain:
        if chord in table.get(mode, {}):
            return table[mode][chord]
    return None


PREFIX_CHORDS: tuple[str, ...] = ("<C-w>",)


def split_chords(text: str) -> list[str]:
    """Parse a published key sequence into chords.

    `"<C-w>l <Enter> x"` -> `['<C-w>l', '<Enter>', 'x']`. Whitespace separates chords,
    with two allowances, because prose writes both forms: a prefix chord absorbs the next
    atom whether or not the author spaced it (`<C-w>l` and `<C-w> l` are the same chord),
    and a doubled bare key is one chord (`gg`), not two presses of the same key.
    """
    atoms: list[tuple[str, bool]] = []          # (atom, glued_to_previous)
    i = 0
    while i < len(text):
        ch = text[i]
        if ch.isspace():
            while i < len(text) and text[i].isspace():
                i += 1
            atoms.append((" ", True))
            continue
        if ch == "<":
            j = text.find(">", i)
            if j < 0:
                raise ValueError("unbalanced < in key sequence %r" % text)
            atoms.append((text[i:j + 1], False))
            i = j + 1
            continue
        atoms.append((ch, False))
        i += 1

    out: list[str] = []
    buf = ""
    space = True
    for atom, _marker in atoms:
        if atom == " ":
            if buf and buf not in PREFIX_CHORDS:
                out.append(buf)
                buf = ""
            space = True
            continue
        if buf and buf in PREFIX_CHORDS:   # a prefix chord always eats the next atom
            out.append(buf + atom)
            buf = ""
            space = True
            continue
        if not space and buf and len(buf) == 1 and not buf.startswith("<") \
                and not atom.startswith("<"):
            out.append(buf + atom)
            buf = ""
            space = True
            continue
        if buf:
            out.append(buf)
        buf = atom
        space = False
    if buf:
        out.append(buf)
    return out


def press(sequence: str, view: str = "queue", interaction: str = "list") -> list[dict]:
    """Resolve a published sequence against the real table.

    Returns one record per chord: the chord, the mode that answered, the action, and
    whether the action is palette-listed. `fathom tui press` prints this; the test
    suite asserts the canonical tasks against it.
    """
    steps = []
    for chord in split_chords(sequence):
        chain = mode_chain(view, interaction)
        action = resolve(chain, chord)
        steps.append({
            "chord": chord,
            "chain": "->".join(chain),
            "mode": next((m for m in chain if chord in DEFAULTS.get(m, {})), "-"),
            "action": action or "(unbound)",
            "palette": bool(action and ACTIONS.get(action, Action("", "", "", "")).palette),
        })
        # a view switch moves the chain; a drill moves focus into pane 2
        if action and action.startswith("view."):
            view = action.split(".", 1)[1]
            interaction = default_interaction(view)
        elif action in ("search.open", "palette.open"):
            interaction = "prompt"
        elif action == "drill.in" and interaction == "list":
            interaction = "body"
        elif action == "overlay.close":
            interaction = "list"
    return steps


def default_interaction(view: str) -> str:
    """Which interaction mode a view opens with: the mode of its pane 1, because focus
    lands on the primary pane on entry (ADR-006 §6.0)."""
    from .views import VIEWS
    for candidate in VIEWS:
        if candidate.id == view:
            return candidate.primary.interaction
    return "list"


def merged(overrides: dict[str, dict[str, str]] | None = None) -> dict[str, dict[str, str]]:
    """`keys.overrides.<mode>.<key>` applied over the defaults (`e5.4`)."""
    table = {mode: dict(binds) for mode, binds in DEFAULTS.items()}
    for mode, binds in (overrides or {}).items():
        table.setdefault(mode, {})
        for chord, action in binds.items():
            if action is None:
                table[mode].pop(chord, None)
            else:
                table[mode][chord] = action
    return table


def binding_index(table: dict[str, dict[str, str]] | None = None) -> dict[str, list[str]]:
    """action id -> `mode:chord` list, which is what the cheatsheet prints."""
    table = table if table is not None else DEFAULTS
    out: dict[str, list[str]] = {}
    for mode in PRECEDENCE:
        for chord, action in sorted(table.get(mode, {}).items()):
            out.setdefault(action, []).append("%s:%s" % (mode, chord))
    return out


def conflicts(overrides: dict[str, dict[str, str]]) -> list[str]:
    """Load-time conflict detection for a user's `keys.overrides` (`e5.4`'s AC).

    Four ways an override file goes wrong, each with its own message:

      * it names an action that does not exist (`unknown action`);
      * it binds an action in a mode that does not accept it (`not bindable`);
      * it claims a navigation key the ADR reserves globally (`shadows`);
      * it names a mode that does not exist (`unknown mode`).
    """
    problems: list[str] = []
    known = {m.name for m in MODES}
    for mode, binds in (overrides or {}).items():
        if mode not in known:
            problems.append("keys.overrides.%s: unknown mode (known: %s)"
                            % (mode, ", ".join(PRECEDENCE)))
            continue
        for chord, action in sorted(binds.items()):
            act = ACTIONS.get(action)
            if act is None and not action.startswith("tab.select_"):
                problems.append("keys.overrides.%s.%s -> unknown action %r"
                                % (mode, chord, action))
                continue
            if act is not None and mode not in act.modes and "global" not in act.modes:
                problems.append("keys.overrides.%s.%s -> %r is not bindable in mode %r "
                                "(it names %s)" % (mode, chord, action, mode,
                                                   ",".join(act.modes)))
            if chord in GLOBAL_NEVER_SHADOWED and mode not in ("global", "prompt",
                                                               "pick"):
                problems.append("keys.overrides.%s.%s shadows the globally reserved key "
                                "%r (ADR-006 §7.2 K4)" % (mode, chord, chord))
    return problems


def unbound_actions(table: dict[str, dict[str, str]] | None = None) -> list[str]:
    """Actions with no key anywhere and no palette entry: the keyboard-completeness
    hole ADR-006 §7.2 rule K1 forbids. Must be empty."""
    table = table if table is not None else DEFAULTS
    bound = {a for binds in table.values() for a in binds.values()}
    return sorted(a for a, act in ACTIONS.items()
                  if a not in bound and not act.palette)


def palette_entries(table: dict[str, dict[str, str]] | None = None) -> list[tuple[str, str, str]]:
    """`(id, title, keys)` for every palette entry — `e5.4`'s completeness test input."""
    index = binding_index(table)
    out = []
    for action_id, act in sorted(ACTIONS.items()):
        if not act.palette:
            continue
        keys = index.get(action_id, [])
        shown = " ".join(sorted({k.split(":", 1)[1] for k in keys})) or "(palette only)"
        out.append((action_id, act.title, shown))
    return out
