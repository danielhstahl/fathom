"""Render ADR-006's normative tables and layouts from the data in this package.

Same contract as `recommender.config doc` and `tools/gen_adr_tables.py`: markdown
between `<!-- BEGIN GENERATED: name -->` markers in `docs/adr/0006-tui.md` is generated,
`doc --check` fails CI on drift, and `doc --write` refreshes the file. The point is that
the keymap in the help overlay, the cheatsheet in the docs site (`e7.5`), the palette
labels (`e5.4`) and this document are all the same bytes, so none of them can lie.
"""

from __future__ import annotations

import pathlib
import re

from . import keymap as K
from . import render as R
from .views import CHROME_ORDER, LAYOUT_MATRIX, VIEWS

DOC = pathlib.Path(__file__).resolve().parents[3] / "docs" / "adr" / "0006-tui.md"
#: Block names may contain `-` and `.` (`layout-matrix`, `layout.queue`); a `\w`-only
#: class silently fails to match the hyphenated ones, and a silently skipped block is a
#: block that lies.
MARK = re.compile(
    r"<!-- BEGIN GENERATED: ([\w.\-]+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)


def _table(header: tuple[str, ...], rows: list[tuple[str, ...]]) -> str:
    out = ["| " + " | ".join(header) + " |",
           "|" + "|".join("---" for _ in header) + "|"]
    out += ["| " + " | ".join(c.replace("|", "\\|") for c in row) + " |" for row in rows]
    return "\n".join(out)


# ---------------------------------------------------------------------------
# views
# ---------------------------------------------------------------------------
def view_inventory() -> str:
    rows = []
    for v in VIEWS:
        rows.append((
            "**%s**" % v.title, "`%s`" % v.key, v.kind,
            " → ".join("`%s`" % p for p in v.focus_ring()) or "—",
            "%d×%d" % (v.ideal_cols, v.ideal_rows),
            "%d×%d" % (R.min_cols_view(v), v.min_rows),
            "%d×%d" % (R.floor_cols(v), R.floor_rows(v)),
            "`%s`" % v.owner,
        ))
    return _table(("View", "Key", "Kind", "Focus ring (pane 1 first)", "Drawn at",
                   "Min", "Ladder floor", "Owner"), rows)


def sizes() -> str:
    rows = []
    for v in VIEWS:
        rows.append((
            "**%s**" % v.title,
            "%d×%d" % (v.ideal_cols, v.ideal_rows),
            "%d×%d" % (R.min_cols_view(v), v.min_rows),
            "%d×%d" % (R.floor_cols(v), R.floor_rows(v)),
            "%d×%d" % (R.refuse_below_cols(v), R.refuse_below_rows(v)),
            ", ".join("`%s`" % c for c in v.chrome),
        ))
    return _table(("View", "Ideal (documented)", "Min: every pane at its documented "
                   "minimum", "Floor: ladder fully applied", "Refuses below",
                   "Chrome declared"), rows)


def layout(view_id: str) -> str:
    view = next(v for v in VIEWS if v.id == view_id)
    art = "\n".join(R.draw(view, view.ideal_cols, view.ideal_rows))
    ring = " → ".join("`%s` (%d)" % (p.name, p.focus_order) for p in view.panes
                     if p.role != "query")
    parts = [
        "```",
        art,
        "```",
        "",
        "*Focus ring:* %s. `<Tab>`/`<C-w>l` walk it, `<S-Tab>`/`<C-w>h` walk back, "
        "`<Enter>` drills, `<Esc>` pops, `<C-w>o` gives the focused pane the whole frame."
        % ring,
        "",
        _table(("From", "Key", "What happens"),
               [("`%s`" % d[0] if "<" in d[0] or " " not in d[0] else d[0],
                 d[1], d[2]) for d in view.drills]),
        "",
        "*Panes and what they read:*",
        "",
        _table(("Pane", "Order", "Kind", "Ideal cols", "Min cols", "Strip", "Reads from "
                "the delta model (`e2.11`)", "Owner"),
               [(("`%s`" % p.name), str(p.focus_order) + (" (query line)"
                                                          if p.role == "query" else ""),
                 p.kind, str(p.ideal_cols), str(p.min_cols),
                 "yes" if p.strip else "no",
                 ", ".join("`%s`" % r for r in p.reads), "`%s`" % p.ticket)
                for p in view.panes]),
    ]
    return "\n".join(parts)


def degradation_ladder() -> str:
    rows = []
    for step in R.LADDER:
        ex_c, ex_r = step.recover(next(v for v in VIEWS if v.id == "queue"))
        rows.append(("**%s**" % step.id, step.what,
                     "+%d" % ex_c if ex_c else "—", "+%d" % ex_r if ex_r else "—",
                     step.why, "`%s`" % step.ticket))
    rows.append(("**%s**" % R.FALLBACK.id, R.FALLBACK.what, "all of them", "—",
                 R.FALLBACK.why, "`%s`" % R.FALLBACK.ticket))
    rows.append(("**%s**" % R.REFUSED.id, R.REFUSED.what, "—", "—", R.REFUSED.why,
                 "`%s`" % R.REFUSED.ticket))
    return _table(("Step", "What it drops", "cols", "rows", "Why this one, in this place",
                   "Owner"), rows)


def degradation_per_view() -> str:
    rows = []
    for v in VIEWS:
        for step, at in R.ladder_boundary(v):
            rows.append(("**%s**" % v.title, step.id, R.step_what(v, step), at))
    return _table(("View", "Step", "What goes", "First fires at"), rows)


def layout_matrix() -> str:
    rows = []
    for v in VIEWS:
        cells = []
        for cols, rows_n in LAYOUT_MATRIX:
            f = R.fit(v, cols, rows_n)
            mark = {"complete": "✓ complete",
                    "degraded": "↓ %d step%s" % (len(f.applied), "" if len(f.applied) == 1
                                                 else "s"),
                    "single": "◇ single pane", "refused": "✗ refused"}[f.state]
            cells.append(mark)
        rows.append(("**%s**" % v.title, *cells))
    return _table(("View", *(("%d×%d" % s) for s in LAYOUT_MATRIX)), rows)


def layout_matrix_steps() -> str:
    """The same matrix, but naming the steps: this is the per-size degradation order
    `e5.5`'s screenshot tests are written against."""
    rows = []
    for v in VIEWS:
        for cols, rows_n in LAYOUT_MATRIX:
            f = R.fit(v, cols, rows_n)
            rows.append(("**%s**" % v.title, "%d×%d" % (cols, rows_n), f.state,
                         ", ".join(f.applied) or "—",
                         "; ".join(f.dropped) or "—"))
    return _table(("View", "Terminal", "State", "Steps applied", "What the reviewer loses"),
                  rows)


# ---------------------------------------------------------------------------
# keymap
# ---------------------------------------------------------------------------
def keymap_table() -> str:
    rows = []
    for mode in K.PRECEDENCE:
        binds = K.DEFAULTS.get(mode, {})
        if not binds:
            if mode in ("queue", "timeline", "ambient"):
                rows.append(("`%s`" % mode, "—", "—",
                             "inherits the shared motion set: no view-local bindings"))
            continue
        for chord, action in sorted(binds.items(), key=lambda kv: kv[0]):
            act = K.ACTIONS.get(action)
            # The action id is a code span, not a link: this table is rendered into two documents
            # (ADR-006 §7.3 and the docs-site cheatsheet) whose registry headings are spelled
            # differently, and a generated link that resolves in one of them is a dead link in the
            # other. `tools/check_docs_site.py` checks every fragment link in the tree, so the
            # dead one could not stay.
            rows.append(("`%s`" % mode, "`%s`" % chord, "`%s`" % action,
                         act.title if act else "(tab selection)"))
    return _table(("Mode", "Key", "Action", "Palette label"), rows)


def action_registry() -> str:
    index = K.binding_index()
    rows = []
    for action_id, act in sorted(K.ACTIONS.items()):
        keys = sorted({k.split(":", 1)[1] for k in index.get(action_id, [])})
        rows.append(("`%s`" % action_id, act.title, act.scope,
                     " ".join("`%s`" % k for k in keys) or "—",
                     "yes" if act.palette else "no",
                     ", ".join("`%s`" % m for m in act.modes), "`%s`" % act.ticket))
    return _table(("Action id", "Label", "Scope", "Default keys", "In palette",
                   "Modes", "Owner"), rows)


def degradation_notices() -> str:
    """The two notices, rendered by the same code the status line uses, so §6.4 cannot
    describe a string `e5.5` will not print. One narrow frame (abbreviated notice, because
    the full one would be clipped) and one refusal (a view that will not draw at all)."""
    queue = next(v for v in VIEWS if v.id == "queue")
    narrow, wide = 64, 200
    f_narrow, f_wide = R.fit(queue, narrow, 14), R.fit(queue, wide, 14)
    line_narrow = R.draw(queue, narrow, 14)[-1]
    line_wide = R.draw(queue, wide, 14)[-1]
    return "\n".join([
        "At 64 columns the notice counts the drops and points at `?`, because the full list "
        "would be the thing that got clipped:",
        "",
        "```",
        line_narrow,
        "  ↑ Queue at %d×14 — %s" % (narrow, f_narrow.summary()),
        "```",
        "",
        "At a width that can afford the list, the same line spells it out:",
        "",
        "```",
        line_wide,
        "  ↑ Queue at %d×14 — %s" % (wide, f_wide.summary()),
        "```",
        "",
        "And when a view will not draw at all, it says so instead of drawing badly:",
        "",
        "```",
        "\n".join(line.rstrip() for line in R.draw(queue, 30, 8)),
        "  ↑ Queue at 30×8 — %s" % R.fit(queue, 30, 8).summary(),
        "```",
    ])


def reserved_chords() -> str:
    rows = [("**terminal cannot deliver**", c, byte, "—", why)
            for c, byte, why in K.TERMINAL_UNUSABLE]
    rows += [("**reserved for user override**", c, "a byte, simply unbound", where, why)
             for c, why, where in K.RESERVED_FOR_USER]
    return _table(("Class", "Chord", "What the terminal delivers", "Free in", "Why"), rows)


def reserved_set() -> str:
    return ", ".join("`%s`" % c for c, _why, _where in K.RESERVED_FOR_USER)


def modes_table() -> str:
    rows = [("`%s`" % m.name, m.kind, "yes" if m.exclusive else "no", m.doc)
            for m in sorted(K.MODES, key=lambda m: K.PRECEDENCE.index(m.name))]
    return _table(("Mode", "Kind", "Exclusive", "Live when"), rows)


def tasks_table() -> str:
    out = []
    for t in K.TASKS:
        out.append("#### %s — %s\n" % (t.id, t.name))
        out.append("*The question that brings the reviewer here:* %s\n" % t.asks)
        out.append("```")
        out.append("   ".join(s.key for s in t.steps))
        out.append("```")
        out.append("")
        out.append(_table(("#", "Key", "Resolves in mode", "Action", "What it does"),
                          [(str(i + 1), "`%s`" % s.key, "`%s`" % s.mode,
                            "`%s`" % s.action, s.effect) for i, s in enumerate(t.steps)]))
        out.append("\n%s\n" % t.note)
    return "\n".join(out)


def hint_line() -> str:
    index = K.binding_index()
    rows = []
    for action_id in K.HINT_ACTIONS:
        act = K.ACTIONS[action_id]
        keys = sorted({k.split(":", 1)[1] for k in index.get(action_id, [])})
        rows.append(("`%s`" % action_id, act.title, " ".join("`%s`" % k for k in keys)))
    return _table(("Action", "Label", "Key"), rows)


def palette_dump() -> str:
    return _table(("Action id", "Palette label", "Current binding"),
                  [("`%s`" % a, t, "`%s`" % k if k != "(palette only)" else
                    "*(palette only)*") for a, t, k in K.palette_entries()])


# ---------------------------------------------------------------------------
# block registry and splice
# ---------------------------------------------------------------------------
def blocks() -> dict[str, str]:
    out = {
        "view-inventory": view_inventory(),
        "sizes": sizes(),
        "degradation-ladder": degradation_ladder(),
        "degradation-per-view": degradation_per_view(),
        "layout-matrix": layout_matrix(),
        "layout-matrix-steps": layout_matrix_steps(),
        "keymap": keymap_table(),
        "action-registry": action_registry(),
        "reserved-chords": reserved_chords(),
        "reserved-set": reserved_set(),
        "modes": modes_table(),
        "tasks": tasks_table(),
        "hint-line": hint_line(),
        "degradation-notices": degradation_notices(),
        "palette": palette_dump(),
    }
    for v in VIEWS:
        out["layout.%s" % v.id] = layout(v.id)
    return out


def splice(text: str, generated: dict[str, str]) -> tuple[str, list[str]]:
    problems: list[str] = []

    def sub(match: re.Match) -> str:
        name = match.group(1)
        if name not in generated:
            problems.append("generated block %r is no longer produced" % name)
            return match.group(0)
        body = generated[name]
        return ("<!-- BEGIN GENERATED: %s -->\n%s\n<!-- END GENERATED: %s -->"
                % (name, body, name))

    new = MARK.sub(sub, text)
    missing = [n for n in generated
               if "<!-- BEGIN GENERATED: %s -->" % n not in text]
    if missing:
        problems.append("document is missing generated blocks: %s" % ", ".join(sorted(missing)))
    return new, problems


def check() -> list[str]:
    text = DOC.read_text()
    generated = blocks()
    new, problems = splice(text, generated)
    if new != text:
        problems.insert(0, "%s is stale: run `python3 -m recommender.tui doc --write`"
                        % DOC.relative_to(DOC.parents[2]))
    return problems


def write() -> str:
    text = DOC.read_text()
    new, problems = splice(text, blocks())
    DOC.write_text(new)
    return "\n".join(problems)
