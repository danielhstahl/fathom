# Keyboard cheatsheet

- **Status**: generated, and CI-checked. Nothing between the GENERATED markers on this page was typed
  by hand, which is the only reason it can be trusted.
- **Ticket / plan key**: `e7.5` (**reco-60m**), under the epic `reco-6t7` (`e7`).
- **Source of truth**: [`src/recommender/tui/keymap.py`](../../src/recommender/tui/keymap.py) — the
  same table ADR-006 §7 quotes, the help overlay renders and the palette labels itself with.
  `crates/fathom-tui/src/keymap.rs` is the Rust copy, pinned to it by
  [`crates/fathom-tui/tests/keymap.rs`](../../crates/fathom-tui/tests/keymap.rs).
- **Regenerate**: `python3 tools/gen_docs.py write --page cheatsheet`. CI runs the `check` form, so a
  keymap change that did not regenerate this page is a red build.

Fathom is a keyboard tool: every action is either bound in a mode or is an entry in the `,` palette,
and ADR-006 §7.0's rule K1 is enforced by a test that enumerates the holes and requires the list to be
empty. So this page is not a list of shortcuts to memorise — it is the shipped binding table, in four
useful shapes: the three sequences people actually run, the views, the keys grouped by intent, and the
whole mode-by-mode table.

Four other places print the same bytes, and they are generated from the same module:

| Where | How you get it |
|---|---|
| Inside the UI | `?` — the help overlay, rendered from the **merged** table, so your own overrides appear in it |
| At the terminal, in a repository | `fathom tui --show-keys` — defaults plus `keys.overrides`, then exit, taking no terminal |
| Anywhere, no repository needed | `PYTHONPATH=src python3 -m recommender.tui keys --cheatsheet` — the printable sheet, from the normative module |
| The decision record | [ADR-006 §7](../adr/0006-tui.md) — the normative prose around the same table |

The difference matters. This page is the **default** table, because that is what a first-time reader
needs; `--show-keys` and the help overlay are the **merged** table, which is what your build actually
honours. If they disagree, the disagreement is your `fathom.toml`, and §8 says how to read it.

---

## 1 The three sequences

Everything else on this page is a lookup table. These are the three things the accountable human does
most, and they are fixed data (`keymap.TASKS`) so that the docs, the hint line and the help overlay
cannot each invent their own story about a keystroke. A test resolves every step through the real
precedence chain, so a rename that breaks a published sequence breaks the build
([ADR-006 §9](../adr/0006-tui.md)).

<!-- BEGIN GENERATED: tasks -->
#### T1 — Triage the top of the queue and sign it off

*The question that brings the reviewer here:* What is the next thing I have to look at, and am I happy with it?

```
q   j   <Enter>   d   x
```

| # | Key | Resolves in mode | Action | What it does |
|---|---|---|---|---|
| 1 | `q` | `global` | `view.queue` | Home. Focus lands on the worklist, ranked by risk (pane 1). |
| 2 | `j` | `list` | `list.down` | Move down the worklist. `H`/`M`/`L` and `<C-d>`/`<C-u>` too. |
| 3 | `<Enter>` | `list` | `drill.in` | Drill into the episode card (pane 2): what changed, where, blast radius, every figure a button. |
| 4 | `d` | `global` | `view.diff` | Same scope, diff view: file tree first, `j` then `<Enter>` to read a file's hunks. |
| 5 | `x` | `global` | `review.accept` | Mark reviewed: disposition `accepted` + identity + time to the ledger; the queue advances. |

`f` flags instead, `-` dismisses with a reason from the closed vocabulary, `z` snoozes, `w` prints the ranking the way `fathom why` does.

#### T2 — Read what the newest turn did, and whether the shape moved

*The question that brings the reviewer here:* The agent has been busy: what landed, and did the architecture get worse?

```
a   <Enter>   s   g   <Esc>
```

| # | Key | Resolves in mode | Action | What it does |
|---|---|---|---|---|
| 1 | `a` | `global` | `view.ambient` | Glance mode: latest episode, its risk, queue count, index state. |
| 2 | `<Enter>` | `body` | `drill.in` | Drill into that episode's card: one keypress, per ADR-001 §4.1. |
| 3 | `s` | `global` | `view.structure` | Where it landed: treemap with churn heat; `[`/`]` move the aggregation level. |
| 4 | `g` | `global` | `view.graph` | Module graph with this episode's delta overlay; `!` shows only the delta, `[`/`]` aggregate, `<`/`>` set the hop limit. |
| 5 | `<Esc>` | `body` | `drill.out` | Up one level, then again to ambient. Nothing was lost on the way. |

`p` for the dependency delta, `o` for what the export would say.

#### T3 — Find a thing and land on it

*The question that brings the reviewer here:* Where is `parse_lockfile`, who touched `Cargo.lock`, what flagged this?

```
/   <text>   <C-n>   <Enter>
```

| # | Key | Resolves in mode | Action | What it does |
|---|---|---|---|---|
| 1 | `/` | `global` | `search.open` | Search overlay, cursor already in the query field. |
| 2 | `<text>` | `prompt` | `(typing)` | Fuzzy over episodes, paths, symbols, dependencies and findings; `author:` `path:` `risk>` `dep:` tokens compose with text. |
| 3 | `<C-n>` | `prompt` | `prompt.result_down` | Walk results without leaving the field; `<C-p>` walks back. |
| 4 | `<Enter>` | `prompt` | `prompt.submit` | Jump into the right view with the right selection: a deep link, not a list of paths. |

`,` is the same mechanism for *commands* instead of content.

<!-- END GENERATED: tasks -->

## 2 The views, and the key that opens each one

One letter each, bound globally (rule K3: a view owns a letter, and the reserved set is a list rather
than a vibe). "Go with" is computed from the bindings, not from the view table, so a moved key shows
up here first.

<!-- BEGIN GENERATED: views -->
| View | Mode | Go with | The question it answers |
|---|---|---|---|
| **Queue** | `queue` | `q` | What is the next thing I have to look at, and why that one? |
| **Timeline** | `timeline` | `t` | What happened, in what order, and which of it have I not read? |
| **Diff** | `diff` | `d` | What exactly changed in this file, and is it what the card claims? |
| **Structure** | `structure` | `s` | Where did it land, and is the tree hottest where it already was? |
| **Graph** | `graph` | `g` | Did this turn make the shape of the system worse? |
| **Deps** | `deps` | `p` | What does the code now depend on that it did not? |
| **Ambient** | `ambient` | `a` | Anything I need to look at right now? (glance, do not study) |
| **Search** | `search` | *(no global key)* | Where is the thing I am thinking of? |
| **Artefact** | `artefact` | `o` | What would my review record say, before I make it permanent? |
<!-- END GENERATED: views -->

Ambient is the odd one out, and [the concept page](../concepts/ambient-mode.md) explains why: it is a
display mode, not a destination, and its floor is 40×3 because ADR-001 §4.1 promises it fits a tmux
pane beside the harness.

## 3 Keys by what you are trying to do

Grouped by intent, which is how most people look a key up. The "Live in" column is the mode a binding
needs: `global` means it works from anywhere in the frame, anything else means that view or interaction
mode has to be live (§4).

<!-- BEGIN GENERATED: keys -->
| Group | Key | Does | Live in |
|---|---|---|---|
| **Move around** | `<C-c>` | Quit Fathom | global |
| **Move around** | `<Enter>` `<Right>` | Drill in (follow selection / evidence) | `list`, `body` |
| **Move around** | `<Esc>` `<Left>` | Up one level (pop the drill stack) | `list`, `body` |
| **Move around** | `<C-o>` | Back to the previous place (drill history) | global |
| **Move around** | `<C-w>l` `<Tab>` | Focus next pane | global |
| **Move around** | `<C-w>h` `<S-Tab>` | Focus previous pane | global |
| **Move around** | `<C-w>o` | Zoom focused pane (fill the frame) | global |
| **Move around** | `<C-l>` | Repaint the screen | global |
| **Move around** | `<C-w>c` | Close the current tab | global |
| **Move around** | `<C-w>n` `K` | Next tab | global |
| **Move around** | `T` | Pin the current view and scope as a tab | global |
| **Move around** | `<C-w>p` `J` | Previous tab | global |
| **Move around** | `<C-w>1` | Go to tab 1 | global |
| **Move around** | `<C-w>2` | Go to tab 2 | global |
| **Move around** | `<C-w>3` | Go to tab 3 | global |
| **Move around** | `<C-w>4` | Go to tab 4 | global |
| **Move around** | `<C-w>5` | Go to tab 5 | global |
| **Move around** | `<C-w>6` | Go to tab 6 | global |
| **Move around** | `<C-w>7` | Go to tab 7 | global |
| **Move around** | `<C-w>8` | Go to tab 8 | global |
| **Move around** | `<C-w>9` | Go to tab 9 | global |
| **Review the episode** | `i` | Show captured intent and its source | global |
| **Review the episode** | `r` | Queue: what you skipped, and why | `queue` |
| **Review the episode** | `x` | Mark reviewed (accept) | global |
| **Review the episode** | `-` | Dismiss with a reason | global |
| **Review the episode** | `f` | Flag for attention, with a note | global |
| **Review the episode** | `z` | Snooze until a condition | global |
| **Review the episode** | `w` | Why this score / this rank (`fathom why`) | global |
| **Change what you are looking at** | `m` | Mark this endpoint for comparison | global |
| **Change what you are looking at** | `c` | Compare the live scope with the mark | global |
| **Change what you are looking at** | `F` | Edit filters (author, area, state, date) | global |
| **Search, palette, help** | `?` | Help: this keymap as the merged table | global |
| **Search, palette, help** | `<Esc>` | Close overlay | `prompt`, `pick` |
| **Search, palette, help** | `,` | Command palette | global |
| **Search, palette, help** | `<Enter>` | Choose this entry | `pick` |
| **Search, palette, help** | `<BS>` | Delete one character | `prompt` |
| **Search, palette, help** | `<C-u>` | Clear the line | `prompt` |
| **Search, palette, help** | `<C-w>` | Delete word | `prompt` |
| **Search, palette, help** | `<C-k>` | Kill to end of line | `prompt` |
| **Search, palette, help** | `<C-n>` `<Down>` `<Tab>` | Next result | `prompt` |
| **Search, palette, help** | `<C-p>` `<Up>` | Previous result | `prompt` |
| **Search, palette, help** | `<Enter>` | Submit | `prompt` |
| **Search, palette, help** | `/` | Search: fuzzy over episodes, paths, symbols, deps, findings | global |
| **Motion, every list** | `L` | Last visible row | `list` |
| **Motion, every list** | `<Down>` `j` | Move down one row | `list`, `body`, `pick` |
| **Motion, every list** | `<Space>` | Expand / collapse node | `list` |
| **Motion, every list** | `gg` | First row | `list`, `body` |
| **Motion, every list** | `<C-d>` | Half page down | `list`, `body` |
| **Motion, every list** | `<C-u>` | Half page up | `list`, `body` |
| **Motion, every list** | `G` | Last row | `list`, `body` |
| **Motion, every list** | `M` | Middle visible row | `list` |
| **Motion, every list** | `n` | Next match | global |
| **Motion, every list** | `<C-f>` `<PageDown>` | Page down | `list`, `body` |
| **Motion, every list** | `<PageUp>` | Page up | `list`, `body` |
| **Motion, every list** | `N` | Previous match | global |
| **Motion, every list** | `<Down>` `j` | Scroll one line down | `body` |
| **Motion, every list** | `<Up>` `k` | Scroll one line up | `body` |
| **Motion, every list** | `H` | First visible row | `list` |
| **Motion, every list** | `<Up>` `k` | Move up one row | `list`, `body`, `pick` |
| **Index and screen** | `<C-r>` | Re-index this repository | global |
| **View-local** | `=` | Artefact: cycle format (md / html / json) | `artefact` |
| **View-local** | `[` | Artefact: narrow the episode range | `artefact` |
| **View-local** | `]` | Artefact: widen the episode range | `artefact` |
| **View-local** | `<Enter>` | Artefact: write the export now | `artefact` |
| **View-local** | `]` | Deps: next ecosystem | `deps` |
| **View-local** | `[` | Deps: previous ecosystem | `deps` |
| **View-local** | `u` | Deps: show transitive-only changes | `deps` |
| **View-local** | `Y` | Diff: copy hunk | `diff` |
| **View-local** | `y` | Diff: copy path | `diff` |
| **View-local** | `]` | Diff: next hunk | `diff` |
| **View-local** | `[` | Diff: previous hunk | `diff` |
| **View-local** | `b` | Diff: show suppressed noise (the rule is printed) | `diff` |
| **View-local** | `E` | Diff: open file at line in $EDITOR | `diff` |
| **View-local** | `\` | Diff: split / unified | `diff` |
| **View-local** | `W` | Diff: word-diff on / off | `diff` |
| **View-local** | `=` | Diff: ignore whitespace | `diff` |
| **View-local** | `<` | Graph: one hop closer | `graph` |
| **View-local** | `>` | Graph: one hop further | `graph` |
| **View-local** | `L` | Graph: cycle layout (layered / circular) | `graph` |
| **View-local** | `]` | Graph: aggregate one level down | `graph` |
| **View-local** | `[` | Graph: aggregate one level up | `graph` |
| **View-local** | `!` | Graph: show only this episode's delta overlay | `graph` |
| **View-local** | `]` | Structure: drill one level down | `structure` |
| **View-local** | `[` | Structure: aggregate one level up | `structure` |
| **View-local** | `<` | Structure: sort by churn / size / delta | `structure` |
| **View-local** | `a` | Go to: Ambient | global |
| **View-local** | `o` | Go to: Artefact | global |
| **View-local** | `p` | Go to: Deps | global |
| **View-local** | `d` | Go to: Diff | global |
| **View-local** | `g` | Go to: Graph | global |
| **View-local** | `q` | Go to: Queue | global |
| **View-local** | `s` | Go to: Structure | global |
| **View-local** | `t` | Go to: Timeline | global |
<!-- END GENERATED: keys -->

The three keys worth learning before anything else are `,` (the palette: every command, with its
current binding printed beside it), `/` (search: fuzzy over episodes, paths, symbols, dependencies and
findings) and `?` (this table, from inside the program).

## 4 Modes: why one key can be four different things

Exactly one *interaction* mode and one *view* mode are live over the always-live `global` layer.
`prompt` and `pick` are **exclusive**: while a text field or a chooser owns the keyboard, `q` is the
letter q and not "go to the queue" — which is the reason the palette can be typed into at all.
Precedence is first-binder-wins down this list (ADR-006 §7.1):

<!-- BEGIN GENERATED: modes -->
| Mode | Kind | Exclusive | Live when |
|---|---|---|---|
| `prompt` | interaction | yes | A one-line text field owns the keyboard (search query, palette filter, reason text, path). Everything typed is text. |
| `pick` | interaction | yes | A single-key chooser is open (dismiss reason, snooze window, format). One keystroke chooses; there is no text entry. |
| `list` | interaction | no | A list pane has focus: queue worklist, episode rail, file tree, dependency table, search results, treemap rows. |
| `body` | interaction | no | A content surface has focus: hunk view, episode card, graph canvas, artefact preview. |
| `queue` | view | no | The attention queue is the live view. |
| `timeline` | view | no | The timeline (episode rail + card) is the live view. |
| `diff` | view | no | The diff view is the live view. |
| `structure` | view | no | The structure treemap view is the live view. |
| `graph` | view | no | The module graph view is the live view. |
| `deps` | view | no | The dependency view is the live view. |
| `artefact` | view | no | The export-preview view is the live view. |
| `ambient` | view | no | Ambient mode is the live display mode. |
| `global` | root | no | Live whenever a non-exclusive mode is not holding the key. Owns view switching, overlays, review actions and the frame. |
<!-- END GENERATED: modes -->

## 5 The full table, mode by mode

Every default binding, in precedence order. This is the table the help overlay draws.

<!-- BEGIN GENERATED: keymap -->
| Mode | Key | Action | Palette label |
|---|---|---|---|
| `prompt` | `<BS>` | `prompt.backspace` | Delete one character |
| `prompt` | `<C-k>` | `prompt.kill_line` | Kill to end of line |
| `prompt` | `<C-n>` | `prompt.result_down` | Next result |
| `prompt` | `<C-p>` | `prompt.result_up` | Previous result |
| `prompt` | `<C-u>` | `prompt.clear_line` | Clear the line |
| `prompt` | `<C-w>` | `prompt.delete_word` | Delete word |
| `prompt` | `<Down>` | `prompt.result_down` | Next result |
| `prompt` | `<Enter>` | `prompt.submit` | Submit |
| `prompt` | `<Esc>` | `overlay.close` | Close overlay |
| `prompt` | `<Tab>` | `prompt.result_down` | Next result |
| `prompt` | `<Up>` | `prompt.result_up` | Previous result |
| `pick` | `<Down>` | `list.down` | Move down one row |
| `pick` | `<Enter>` | `pick.choose` | Choose this entry |
| `pick` | `<Esc>` | `overlay.close` | Close overlay |
| `pick` | `<Up>` | `list.up` | Move up one row |
| `pick` | `j` | `list.down` | Move down one row |
| `pick` | `k` | `list.up` | Move up one row |
| `list` | `<C-d>` | `list.half_down` | Half page down |
| `list` | `<C-f>` | `list.page_down` | Page down |
| `list` | `<C-u>` | `list.half_up` | Half page up |
| `list` | `<Down>` | `list.down` | Move down one row |
| `list` | `<Enter>` | `drill.in` | Drill in (follow selection / evidence) |
| `list` | `<Esc>` | `drill.out` | Up one level (pop the drill stack) |
| `list` | `<Left>` | `drill.out` | Up one level (pop the drill stack) |
| `list` | `<PageDown>` | `list.page_down` | Page down |
| `list` | `<PageUp>` | `list.page_up` | Page up |
| `list` | `<Right>` | `drill.in` | Drill in (follow selection / evidence) |
| `list` | `<S-Tab>` | `pane.focus_prev` | Focus previous pane |
| `list` | `<Space>` | `list.expand` | Expand / collapse node |
| `list` | `<Tab>` | `pane.focus_next` | Focus next pane |
| `list` | `<Up>` | `list.up` | Move up one row |
| `list` | `G` | `list.last` | Last row |
| `list` | `H` | `list.top_visible` | First visible row |
| `list` | `L` | `list.bot_visible` | Last visible row |
| `list` | `M` | `list.mid_visible` | Middle visible row |
| `list` | `gg` | `list.first` | First row |
| `list` | `j` | `list.down` | Move down one row |
| `list` | `k` | `list.up` | Move up one row |
| `body` | `<C-d>` | `list.half_down` | Half page down |
| `body` | `<C-f>` | `list.page_down` | Page down |
| `body` | `<C-u>` | `list.half_up` | Half page up |
| `body` | `<Down>` | `list.scroll_down` | Scroll one line down |
| `body` | `<Enter>` | `drill.in` | Drill in (follow selection / evidence) |
| `body` | `<Esc>` | `drill.out` | Up one level (pop the drill stack) |
| `body` | `<Left>` | `drill.out` | Up one level (pop the drill stack) |
| `body` | `<PageDown>` | `list.page_down` | Page down |
| `body` | `<PageUp>` | `list.page_up` | Page up |
| `body` | `<Right>` | `drill.in` | Drill in (follow selection / evidence) |
| `body` | `<S-Tab>` | `pane.focus_prev` | Focus previous pane |
| `body` | `<Tab>` | `pane.focus_next` | Focus next pane |
| `body` | `<Up>` | `list.scroll_up` | Scroll one line up |
| `body` | `G` | `list.last` | Last row |
| `body` | `gg` | `list.first` | First row |
| `body` | `j` | `list.scroll_down` | Scroll one line down |
| `body` | `k` | `list.scroll_up` | Scroll one line up |
| `queue` | `r` | `queue.retrospect` | Queue: what you skipped, and why |
| `timeline` | — | — | inherits the shared motion set: no view-local bindings |
| `diff` | `=` | `diff.ws_toggle` | Diff: ignore whitespace |
| `diff` | `E` | `diff.open_editor` | Diff: open file at line in $EDITOR |
| `diff` | `W` | `diff.word_toggle` | Diff: word-diff on / off |
| `diff` | `Y` | `diff.copy_hunk` | Diff: copy hunk |
| `diff` | `[` | `diff.hunk_prev` | Diff: previous hunk |
| `diff` | `\` | `diff.split_toggle` | Diff: split / unified |
| `diff` | `]` | `diff.hunk_next` | Diff: next hunk |
| `diff` | `b` | `diff.noise_toggle` | Diff: show suppressed noise (the rule is printed) |
| `diff` | `y` | `diff.copy_path` | Diff: copy path |
| `structure` | `<` | `structure.sort_cycle` | Structure: sort by churn / size / delta |
| `structure` | `[` | `structure.level_up` | Structure: aggregate one level up |
| `structure` | `]` | `structure.level_down` | Structure: drill one level down |
| `graph` | `!` | `graph.overlay_only` | Graph: show only this episode's delta overlay |
| `graph` | `<` | `graph.hop_less` | Graph: one hop closer |
| `graph` | `>` | `graph.hop_more` | Graph: one hop further |
| `graph` | `L` | `graph.layout_cycle` | Graph: cycle layout (layered / circular) |
| `graph` | `[` | `graph.level_up` | Graph: aggregate one level up |
| `graph` | `]` | `graph.level_down` | Graph: aggregate one level down |
| `deps` | `[` | `deps.ecosystem_prev` | Deps: previous ecosystem |
| `deps` | `]` | `deps.ecosystem_next` | Deps: next ecosystem |
| `deps` | `u` | `deps.transitive_toggle` | Deps: show transitive-only changes |
| `artefact` | `<Enter>` | `artefact.write` | Artefact: write the export now |
| `artefact` | `=` | `artefact.format_cycle` | Artefact: cycle format (md / html / json) |
| `artefact` | `[` | `artefact.scope_narrow` | Artefact: narrow the episode range |
| `artefact` | `]` | `artefact.scope_widen` | Artefact: widen the episode range |
| `ambient` | — | — | inherits the shared motion set: no view-local bindings |
| `global` | `,` | `palette.open` | Command palette |
| `global` | `-` | `review.dismiss` | Dismiss with a reason |
| `global` | `/` | `search.open` | Search: fuzzy over episodes, paths, symbols, deps, findings |
| `global` | `<C-c>` | `app.quit` | Quit Fathom |
| `global` | `<C-l>` | `screen.redraw` | Repaint the screen |
| `global` | `<C-o>` | `history.back` | Back to the previous place (drill history) |
| `global` | `<C-r>` | `index.reindex` | Re-index this repository |
| `global` | `<C-w>1` | `tab.select_1` | Go to tab 1 |
| `global` | `<C-w>2` | `tab.select_2` | Go to tab 2 |
| `global` | `<C-w>3` | `tab.select_3` | Go to tab 3 |
| `global` | `<C-w>4` | `tab.select_4` | Go to tab 4 |
| `global` | `<C-w>5` | `tab.select_5` | Go to tab 5 |
| `global` | `<C-w>6` | `tab.select_6` | Go to tab 6 |
| `global` | `<C-w>7` | `tab.select_7` | Go to tab 7 |
| `global` | `<C-w>8` | `tab.select_8` | Go to tab 8 |
| `global` | `<C-w>9` | `tab.select_9` | Go to tab 9 |
| `global` | `<C-w>c` | `tab.close` | Close the current tab |
| `global` | `<C-w>h` | `pane.focus_prev` | Focus previous pane |
| `global` | `<C-w>l` | `pane.focus_next` | Focus next pane |
| `global` | `<C-w>n` | `tab.next` | Next tab |
| `global` | `<C-w>o` | `pane.zoom` | Zoom focused pane (fill the frame) |
| `global` | `<C-w>p` | `tab.prev` | Previous tab |
| `global` | `?` | `help.open` | Help: this keymap as the merged table |
| `global` | `F` | `filter.edit` | Edit filters (author, area, state, date) |
| `global` | `J` | `tab.prev` | Previous tab |
| `global` | `K` | `tab.next` | Next tab |
| `global` | `N` | `list.prev_match` | Previous match |
| `global` | `T` | `tab.pin` | Pin the current view and scope as a tab |
| `global` | `a` | `view.ambient` | Go to: Ambient |
| `global` | `c` | `compare.with_mark` | Compare the live scope with the mark |
| `global` | `d` | `view.diff` | Go to: Diff |
| `global` | `f` | `review.flag` | Flag for attention, with a note |
| `global` | `g` | `view.graph` | Go to: Graph |
| `global` | `i` | `intent.show` | Show captured intent and its source |
| `global` | `m` | `compare.mark` | Mark this endpoint for comparison |
| `global` | `n` | `list.next_match` | Next match |
| `global` | `o` | `view.artefact` | Go to: Artefact |
| `global` | `p` | `view.deps` | Go to: Deps |
| `global` | `q` | `view.queue` | Go to: Queue |
| `global` | `s` | `view.structure` | Go to: Structure |
| `global` | `t` | `view.timeline` | Go to: Timeline |
| `global` | `w` | `risk.explain` | Why this score / this rank (`fathom why`) |
| `global` | `x` | `review.accept` | Mark reviewed (accept) |
| `global` | `z` | `review.snooze` | Snooze until a condition |
<!-- END GENERATED: keymap -->

## 6 Every action, bound or in the palette

The action registry: every addressable command, the keys that currently reach it, whether `,` offers
it, and the plan item that owns its implementation. An action with `—` in the *Keys* column is not
missing, it is palette-only — which still counts as keyboard-reachable, and is the half of K1 that
makes "unbound" different from "unreachable".

<!-- BEGIN GENERATED: actions -->
| Action id | Label | Scope | Default keys | In palette | Modes | Owner |
|---|---|---|---|---|---|---|
| `app.quit` | Quit Fathom | frame | `<C-c>` | yes | `global` | `e1.5` |
| `artefact.format_cycle` | Artefact: cycle format (md / html / json) | view | `=` | yes | `artefact` | `e5.7` |
| `artefact.scope_narrow` | Artefact: narrow the episode range | view | `[` | yes | `artefact` | `e5.7` |
| `artefact.scope_widen` | Artefact: widen the episode range | view | `]` | yes | `artefact` | `e5.7` |
| `artefact.write` | Artefact: write the export now | view | `<Enter>` | yes | `artefact` | `e5.7` |
| `compare.mark` | Mark this endpoint for comparison | scope | `m` | yes | `global` | `e5.3` |
| `compare.with_mark` | Compare the live scope with the mark | scope | `c` | yes | `global` | `e5.3` |
| `deps.ecosystem_next` | Deps: next ecosystem | view | `]` | yes | `deps` | `e2.5` |
| `deps.ecosystem_prev` | Deps: previous ecosystem | view | `[` | yes | `deps` | `e2.5` |
| `deps.transitive_toggle` | Deps: show transitive-only changes | view | `u` | yes | `deps` | `e2.6` |
| `diff.copy_hunk` | Diff: copy hunk | view | `Y` | yes | `diff` | `e5.8` |
| `diff.copy_path` | Diff: copy path | view | `y` | yes | `diff` | `e5.8` |
| `diff.hunk_next` | Diff: next hunk | view | `]` | yes | `diff` | `e1.7` |
| `diff.hunk_prev` | Diff: previous hunk | view | `[` | yes | `diff` | `e1.7` |
| `diff.noise_toggle` | Diff: show suppressed noise (the rule is printed) | view | `b` | yes | `diff` | `e2.2` |
| `diff.open_editor` | Diff: open file at line in $EDITOR | view | `E` | yes | `diff` | `e5.8` |
| `diff.split_toggle` | Diff: split / unified | view | `\` | yes | `diff` | `e1.7` |
| `diff.word_toggle` | Diff: word-diff on / off | view | `W` | yes | `diff` | `e1.7` |
| `diff.ws_toggle` | Diff: ignore whitespace | view | `=` | yes | `diff` | `e1.7` |
| `drill.in` | Drill in (follow selection / evidence) | frame | `<Enter>` `<Right>` | yes | `list`, `body` | `e1.5` |
| `drill.out` | Up one level (pop the drill stack) | frame | `<Esc>` `<Left>` | yes | `list`, `body` | `e1.5` |
| `filter.clear` | Clear all filters | scope | — | yes | `global` | `e1.6` |
| `filter.edit` | Edit filters (author, area, state, date) | scope | `F` | yes | `global` | `e1.6` |
| `goto.first_unread` | Jump to first unread episode | scope | — | yes | `global` | `e1.6` |
| `goto.today` | Jump to today | scope | — | yes | `global` | `e1.6` |
| `graph.hop_less` | Graph: one hop closer | view | `<` | yes | `graph` | `e3.4` |
| `graph.hop_more` | Graph: one hop further | view | `>` | yes | `graph` | `e3.4` |
| `graph.layout_cycle` | Graph: cycle layout (layered / circular) | view | `L` | yes | `graph` | `e3.4` |
| `graph.level_down` | Graph: aggregate one level down | view | `]` | yes | `graph` | `e3.4` |
| `graph.level_up` | Graph: aggregate one level up | view | `[` | yes | `graph` | `e3.4` |
| `graph.overlay_only` | Graph: show only this episode's delta overlay | view | `!` | yes | `graph` | `e3.4` |
| `help.open` | Help: this keymap as the merged table | overlay | `?` | yes | `global` | `e1.5` |
| `history.back` | Back to the previous place (drill history) | frame | `<C-o>` | yes | `global` | `e1.5` |
| `index.reindex` | Re-index this repository | index | `<C-r>` | yes | `global` | `e1.4` |
| `intent.show` | Show captured intent and its source | review | `i` | yes | `global` | `e4.2` |
| `list.bot_visible` | Last visible row | motion | `L` | yes | `list` | `e5.4` |
| `list.down` | Move down one row | motion | `<Down>` `j` | yes | `list`, `body`, `pick` | `e1.5` |
| `list.expand` | Expand / collapse node | motion | `<Space>` | yes | `list` | `e1.7` |
| `list.first` | First row | motion | `gg` | yes | `list`, `body` | `e1.5` |
| `list.half_down` | Half page down | motion | `<C-d>` | yes | `list`, `body` | `e1.5` |
| `list.half_up` | Half page up | motion | `<C-u>` | yes | `list`, `body` | `e1.5` |
| `list.last` | Last row | motion | `G` | yes | `list`, `body` | `e1.5` |
| `list.mid_visible` | Middle visible row | motion | `M` | yes | `list` | `e5.4` |
| `list.next_match` | Next match | motion | `n` | yes | `global` | `e5.2` |
| `list.page_down` | Page down | motion | `<C-f>` `<PageDown>` | yes | `list`, `body` | `e1.5` |
| `list.page_up` | Page up | motion | `<PageUp>` | yes | `list`, `body` | `e1.5` |
| `list.prev_match` | Previous match | motion | `N` | yes | `global` | `e5.2` |
| `list.scroll_down` | Scroll one line down | motion | `<Down>` `j` | no | `body` | `e5.5` |
| `list.scroll_up` | Scroll one line up | motion | `<Up>` `k` | no | `body` | `e5.5` |
| `list.top_visible` | First visible row | motion | `H` | yes | `list` | `e5.4` |
| `list.up` | Move up one row | motion | `<Up>` `k` | yes | `list`, `body`, `pick` | `e1.5` |
| `overlay.close` | Close overlay | overlay | `<Esc>` | no | `prompt`, `pick` | `e1.5` |
| `palette.open` | Command palette | overlay | `,` | yes | `global` | `e5.4` |
| `pane.focus_next` | Focus next pane | frame | `<C-w>l` `<Tab>` | yes | `global` | `e1.5` |
| `pane.focus_prev` | Focus previous pane | frame | `<C-w>h` `<S-Tab>` | yes | `global` | `e1.5` |
| `pane.zoom` | Zoom focused pane (fill the frame) | frame | `<C-w>o` | yes | `global` | `e5.5` |
| `pick.choose` | Choose this entry | overlay | `<Enter>` | no | `pick` | `e4.5` |
| `prompt.backspace` | Delete one character | overlay | `<BS>` | no | `prompt` | `e5.4` |
| `prompt.clear_line` | Clear the line | overlay | `<C-u>` | no | `prompt` | `e5.4` |
| `prompt.delete_word` | Delete word | overlay | `<C-w>` | no | `prompt` | `e5.4` |
| `prompt.kill_line` | Kill to end of line | overlay | `<C-k>` | no | `prompt` | `e5.4` |
| `prompt.result_down` | Next result | overlay | `<C-n>` `<Down>` `<Tab>` | no | `prompt` | `e5.2` |
| `prompt.result_up` | Previous result | overlay | `<C-p>` `<Up>` | no | `prompt` | `e5.2` |
| `prompt.submit` | Submit | overlay | `<Enter>` | no | `prompt` | `e5.2` |
| `queue.retrospect` | Queue: what you skipped, and why | review | `r` | yes | `queue` | `e4.5` |
| `review.accept` | Mark reviewed (accept) | review | `x` | yes | `global` | `e4.5` |
| `review.dismiss` | Dismiss with a reason | review | `-` | yes | `global` | `e4.5` |
| `review.flag` | Flag for attention, with a note | review | `f` | yes | `global` | `e4.5` |
| `review.snooze` | Snooze until a condition | review | `z` | yes | `global` | `e4.5` |
| `risk.explain` | Why this score / this rank (`fathom why`) | review | `w` | yes | `global` | `e4.4` |
| `screen.redraw` | Repaint the screen | frame | `<C-l>` | yes | `global` | `e1.5` |
| `search.open` | Search: fuzzy over episodes, paths, symbols, deps, findings | overlay | `/` | yes | `global` | `e5.2` |
| `structure.level_down` | Structure: drill one level down | view | `]` | yes | `structure` | `e2.3` |
| `structure.level_up` | Structure: aggregate one level up | view | `[` | yes | `structure` | `e2.3` |
| `structure.sort_cycle` | Structure: sort by churn / size / delta | view | `<` | yes | `structure` | `e2.3` |
| `tab.close` | Close the current tab | frame | `<C-w>c` | yes | `global` | `e1.5` |
| `tab.next` | Next tab | frame | `<C-w>n` `K` | yes | `global` | `e1.5` |
| `tab.pin` | Pin the current view and scope as a tab | frame | `T` | yes | `global` | `e1.5` |
| `tab.prev` | Previous tab | frame | `<C-w>p` `J` | yes | `global` | `e1.5` |
| `tab.select_1` | Go to tab 1 | frame | `<C-w>1` | yes | `global` | `e1.5` |
| `tab.select_2` | Go to tab 2 | frame | `<C-w>2` | yes | `global` | `e1.5` |
| `tab.select_3` | Go to tab 3 | frame | `<C-w>3` | yes | `global` | `e1.5` |
| `tab.select_4` | Go to tab 4 | frame | `<C-w>4` | no | `global` | `e1.5` |
| `tab.select_5` | Go to tab 5 | frame | `<C-w>5` | no | `global` | `e1.5` |
| `tab.select_6` | Go to tab 6 | frame | `<C-w>6` | no | `global` | `e1.5` |
| `tab.select_7` | Go to tab 7 | frame | `<C-w>7` | no | `global` | `e1.5` |
| `tab.select_8` | Go to tab 8 | frame | `<C-w>8` | no | `global` | `e1.5` |
| `tab.select_9` | Go to tab 9 | frame | `<C-w>9` | no | `global` | `e1.5` |
| `view.ambient` | Go to: Ambient | view | `a` | yes | `global` | `e5.1` |
| `view.artefact` | Go to: Artefact | view | `o` | yes | `global` | `e5.7` |
| `view.deps` | Go to: Deps | view | `p` | yes | `global` | `e2.6` |
| `view.diff` | Go to: Diff | view | `d` | yes | `global` | `e1.7` |
| `view.graph` | Go to: Graph | view | `g` | yes | `global` | `e3.4` |
| `view.queue` | Go to: Queue | view | `q` | yes | `global` | `e4.5` |
| `view.structure` | Go to: Structure | view | `s` | yes | `global` | `e2.3` |
| `view.timeline` | Go to: Timeline | view | `t` | yes | `global` | `e1.6` |
<!-- END GENERATED: actions -->

## 7 Chords that will not work, and chords Fathom leaves alone

Two lists, both data (`keymap.TERMINAL_UNUSABLE`, `keymap.RESERVED_FOR_USER`), both measured rather
than remembered — the first by `recommender.tui.keyprobe` on a real pty, in CI.

The first list is physics: a terminal that cannot tell `<C-i>` from `<Tab>` cannot deliver a binding
on both, so no configuration can fix a chord on that list. The second list is a promise: those keys
are Fathom's reserved-but-unused set, yours for the taking, and the conflict checker refuses a
*default* binding that collides with one.

<!-- BEGIN GENERATED: reserved -->
| Class | Chord | What the terminal delivers | Free in | Why |
|---|---|---|---|---|
| **terminal cannot deliver** | <C-i> | one byte with `<Tab>` (0x09) | — | The driver cannot distinguish them; `<Tab>` is bound, so `<C-i>` is not. |
| **terminal cannot deliver** | <C-j> | one byte with LF (0x0a) | — | LF is what `<Enter>` delivers in canonical mode, which is bound to drill. |
| **terminal cannot deliver** | <C-m> | one byte with CR (0x0d), which the driver maps to LF | — | So `<C-m>` and `<C-j>` arrive identical: both are `<Enter>`. |
| **terminal cannot deliver** | <C-h> | one byte with <Backspace> (0x08, or 0x7f on some terminals) | — | `<BS>` is bound to delete/back in `prompt`; `<C-h>` cannot be distinct from it. |
| **terminal cannot deliver** | <C-[> | one byte with `<Esc>` (0x1b) | — | `<Esc>` is the drill-out chord, so `<C-[>` is unusable by construction. |
| **terminal cannot deliver** | <C-@> | NUL (0x00) — identical to `<C-Space>` | — | Both spell NUL, so neither is bound and `<C-Space>` is left for users. |
| **terminal cannot deliver** | <C-c> | no byte at all: SIGINT while `ISIG` is on | — | Fathom keeps `ISIG` on so the exit/panic hook owns terminal restoration (`e1.5`), which makes `<C-c>` the only quit chord and never a rebinding target. |
| **terminal cannot deliver** | <C-z> | no byte: SIGTSTP while `ISIG` is on | — | Job control belongs to the shell. |
| **terminal cannot deliver** | <C-\> | no byte: SIGQUIT while `ISIG` is on | — | Same reason as `<C-z>`. |
| **terminal cannot deliver** | A-<key> | ESC + key: byte-identical to `<Esc>` then the key | — | So **no Alt/Meta chord is bound in v1**. Two-keystroke chords (`<C-w>h`, `gg`) are the escape hatch and cost nothing to deliver. |
| **terminal cannot deliver** | <S-Tab> | ESC [ Z — only if the terminal sends it | — | Bound as an *alias* for `<Tab>`, with `<C-w>h` as the guaranteed route, so a hostile `TERM` loses a convenience and not a capability. |
| **reserved for user override** | <C-a> | a byte, simply unbound | all modes | shell/multiplexer start-of-line, and screen's default prefix (the second prefix most tmux users bind) |
| **reserved for user override** | <C-b> | a byte, simply unbound | all modes | tmux's default prefix. Binding it here costs a user their whole multiplexer, so it stays free even though the terminal delivers it. |
| **reserved for user override** | <C-e> | a byte, simply unbound | all modes | `less`/vim scroll-one-line-down, left free with `<C-d>`/`<C-u>` already covering half-page |
| **reserved for user override** | <C-g> | a byte, simply unbound | all modes | readline abort — a good personal escape hatch |
| **reserved for user override** | <C-k> | a byte, simply unbound | outside prompt | readline kill-to-end-of-line: used inside the text field, free elsewhere |
| **reserved for user override** | <C-n> | a byte, simply unbound | outside prompt | readline next-history: walks results inside the text field, free elsewhere |
| **reserved for user override** | <C-t> | a byte, simply unbound | all modes | no convention worth honouring, so it is a good user chord |
| **reserved for user override** | <C-v> | a byte, simply unbound | all modes | readline literal-next, and the ubiquitous paste chord |
| **reserved for user override** | <C-x> | a byte, simply unbound | all modes | one of the most-requested personal chords in vim keymap threads; free |
| **reserved for user override** | <C-6> | a byte, simply unbound | all modes | vim's alternate-file |
| **reserved for user override** | <S-F1> | a byte, simply unbound | all modes | function keys are otherwise unbound in v1: their encodings vary more across terminals than any other group |
<!-- END GENERATED: reserved -->

## 8 Making a key yours

`fathom.toml` carries overrides, and only overrides — there is no `[keymap]` section to copy the whole
table into, which is ADR-002 §4.5's refusal and the reason the table has one owner:

```toml
[keys.overrides.global]
"," = "palette.open"      # same key, on purpose, now explicit
"gt" = "tab.next"         # a two-keystroke chord, written without a space
```

Four ways an override cannot be honoured — an unknown mode, an unknown action, a collision with a
hard-reserved key, a shadowing of a globally reserved key in a narrower mode — are a **startup report
with the offending key named**, not a silent fallback (rule K2). Read that report; `--show-keys` prints
the merged result, and the help overlay shows the same merged table at runtime:

```bash
fathom tui --show-keys                 # what your build really binds (defaults + your overrides)
fathom tui --show-session              # the session state stored for this repository
fathom tui --show-session --json       # and the same rows as data — `--json` belongs to this one
```

Override precedence is the layer order of every other key in Fathom
([the config model](../concepts/config.md)): built-in defaults, then `~/.config/fathom/fathom.toml`,
then the repository's `fathom.toml`, then `FATHOM_*` environment, then the command line. A repository
may rebind your keys because you put it in your clone, and `explain` prints which layer won for any key
([config §8](../concepts/config.md) — `PYTHONPATH=src python3 -m recommender.config explain --key keys`
in this tree).

## 9 What the status line is allowed to advertise

The hint line at the bottom of the frame is generated too, from a fixed list, so it can never promise a
key the build does not bind ([ADR-006 §5](../adr/0006-tui.md), `keymap.HINT_ACTIONS`):

<!-- BEGIN GENERATED: hint -->
| Action | Label | Key |
|---|---|---|
| `view.queue` | Go to: Queue | `q` |
| `view.timeline` | Go to: Timeline | `t` |
| `view.diff` | Go to: Diff | `d` |
| `search.open` | Search: fuzzy over episodes, paths, symbols, deps, findings | `/` |
| `palette.open` | Command palette | `,` |
| `review.accept` | Mark reviewed (accept) | `x` |
| `risk.explain` | Why this score / this rank (`fathom why`) | `w` |
| `help.open` | Help: this keymap as the merged table | `?` |
<!-- END GENERATED: hint -->

---

**Next**: [the CLI reference](cli.md) for the headless half of the same facts ·
[the detector catalogue](../detectors/index.md) for what the findings on those screens are ·
[troubleshooting](../troubleshooting.md) when a key does not do what this page says.
