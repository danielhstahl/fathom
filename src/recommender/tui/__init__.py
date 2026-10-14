"""Fathom's TUI information architecture, as data (ADR-006).

`recommender.tui` is the normative source for three things the screens are built from:

* :mod:`recommender.tui.views` — the view inventory: panes, focus order, sizes,
  drill edges and the fields each pane reads from the delta model;
* :mod:`recommender.tui.keymap` — modes, actions, the canonical binding table, the
  chords reserved for user override and the three canonical task sequences;
* :mod:`recommender.tui.render` — the layout engine and the degradation ladder, which
  produce the ASCII layouts and the minimum/floor numbers ADR-006 documents.

The ADR is rendered from here (`python3 -m recommender.tui doc --write`), and
`tests/test_tui_contract.py` fails if the document and the data disagree.
"""

from .keymap import (ACTIONS, ALIASES, DEFAULTS, GLOBAL_NEVER_SHADOWED, MODES,
                     PRECEDENCE, PREFIX_C_W, RESERVED_FOR_USER, SIGNAL_KEYS_KEEP_AS_SIGNAL,
                     TERMINAL_UNUSABLE, TASKS, binding_index, conflicts, merged,
                     mode_chain, palette_entries, press, resolve, split_chords,
                     unbound_actions)
from .render import (FALLBACK, LADDER, REFUSED, Fit, chrome_rows, disp_width, draw, fit,
                     floor_cols, floor_rows, ladder_boundary, min_cols_view, pane_widths,
                     refuse_below_cols, refuse_below_rows)
from .views import (CHROME_ORDER, LAYOUT_MATRIX, VIEWS, VIEW_KEYS, Pane, View)

__all__ = [n for n in dir() if not n.startswith("_")]
