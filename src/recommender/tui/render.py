"""The layout engine: pane widths, frame composition, and the **degradation ladder**.
This module renders the ASCII layouts that appear in ADR-006 §5 and computes the
minimum/floor sizes §6 documents, so both are arithmetic rather than opinion.

The ladder is one ordered sequence applied to every view (ADR-006 §6.0). Each step
recovers a measured number of rows and/or columns; the engine applies steps in order
until the view fits, which is what makes a per-view "degradation order" a derivable
fact instead of a paragraph:

    D1 hint line     D2 tab bar        D3 pane strips (legend/counts)
    D4 box frame     D5 secondary pane -> 16-col strip
    D6 panes below documented minimum (down to hard minimum)
    D7 compact rows  D8 trimmable content rows
    D9 single-pane fallback (secondary panes dropped, banner names them)
    D10 refuse and say so (the terminal is not a Fathom terminal)

Two rules the ladder must never break (ADR-001 §7.2, "never hide silently"): a step
that removes content is *announced* — the status line names what was dropped and the key
that brings it back — and no step clips a pane below its hard minimum, because a
truncated file path or a broken 8-character SHA is worse than an honest refusal.
"""

from __future__ import annotations

import unicodedata
from dataclasses import dataclass, field

from .views import CHROME_ORDER, Pane, View, VIEWS

BOX = {"tl": "┌", "tr": "┐", "bl": "└", "br": "┘", "h": "─", "v": "│",
       "tc": "┬", "bc": "┴", "lm": "├", "rm": "┤", "cross": "┼"}


# ---------------------------------------------------------------------------
# display width (`e5.5` owns the full grapheme story; this is the part the layout
# engine may not get wrong: a box that is one cell wide of the terminal wraps)
# ---------------------------------------------------------------------------
def disp_width(text: str) -> int:
    w = 0
    for ch in text:
        if unicodedata.combining(ch):
            continue
        ea = unicodedata.east_asian_width(ch)
        w += 2 if ea in ("W", "F") else 1
    return w


def fit_text(text: str, width: int, ellipsis: str = "…") -> str:
    """Truncate to `width` cells **without splitting a codepoint run** (e5.5)."""
    if disp_width(text) <= width:
        return text + " " * (width - disp_width(text))
    out = ""
    for ch in text:
        if disp_width(out + ch) > width - disp_width(ellipsis):
            break
        out += ch
    return out + ellipsis


# ---------------------------------------------------------------------------
# the ladder
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Step:
    id: str
    what: str
    why: str
    ticket: str

    def recover(self, view: View) -> tuple[int, int]:
        """(columns, rows) this step buys back, in *frame* rows.

        Every number here is a fact about the chrome table in §5 or the pane table in §5,
        not a guess: `D3.strips` is worth two rows because a strip is one row per pane and
        a boxed view has two panes that declare one.
        """
        return _RECOVER[self.id](view)


def _hint(v: View) -> tuple[int, int]:
    return (0, 1) if "hint" in v.chrome else (0, 0)


def _tabs(v: View) -> tuple[int, int]:
    return (0, 1) if "tabs" in v.chrome else (0, 0)


def _strips(v: View) -> tuple[int, int]:
    """One row: the row of pane footers (legend, counts, the ordering explanation) drawn
    side by side. Zero in a borderless view, which declares no footers — Ambient cannot
    sell what it never had, which is why its ladder in §6.2 is the shortest."""
    if "strips" not in v.chrome or not any(p.strip for p in v.panes):
        return (0, 0)
    return (0, 1)


def _frame(v: View) -> tuple[int, int]:
    """Dropping the box: two side borders (cols) and the bottom rule (row) come back; the
    top rule stays as the one-line pane header, so panes stay tellable apart (§6.1)."""
    if "frame" not in v.chrome:
        return (0, 0)
    return (2, 1)


def _secondary_strip(v: View) -> tuple[int, int]:
    return (sum(max(0, p.min_cols - p.collapse_to) for p in v.secondaries
                if p.role == "secondary"), 0)


def _cols_hard(v: View) -> tuple[int, int]:
    return (sum(max(0, p.min_cols - p.hard_min) for p in v.panes
                if p.role != "query"), 0)


def _compact_rows(v: View) -> tuple[int, int]:
    return (0, 3)


def _content_trim(v: View) -> tuple[int, int]:
    return (0, max(p.trim_rows for p in v.panes))


_RECOVER = {
    "D1.hint": _hint, "D2.tabs": _tabs, "D3.strips": _strips, "D4.frame": _frame,
    "D5.secondary_strip": _secondary_strip, "D6.cols_hard": _cols_hard,
    "D7.compact_rows": _compact_rows, "D8.content_trim": _content_trim,
}

#: The order. Fixed here once so that no view can invent a friendlier one for itself.
LADDER: tuple[Step, ...] = (
    Step("D1.hint", "hint line (the next-action reminder)",
         "It is the only line a learned user stopped reading, and it is the one line "
         "that carries no data.", "e5.4"),
    Step("D2.tabs", "tab bar",
         "`J`/`K` and `<C-w>1..9` still reach every tab, so the bar is a label.",
         "e1.5"),
    Step("D3.strips", "pane footer strips (legend, counts, ordering explanation)",
         "Legends are learnable once; the strip re-appears with `?` and in the export.",
         "e5.5"),
    Step("D4.frame", "box frame: side borders and the bottom rule",
         "Titles plus a column rule keep panes distinguishable without a full box.",
         "e5.5"),
    Step("D5.secondary_strip", "secondary panes squeezed to their strip width",
         "The secondary pane answers a follow-up question; a headline strip keeps it "
         "answerable, and `<C-w>l` or `<Enter>` gives it the frame.", "e5.5"),
    Step("D6.cols_hard", "panes below their documented minimum, to the hard minimum",
         "Between ideal and minimum a pane simply narrows; below minimum it starts "
         "dropping columns of its own, which is why this step is named separately.",
         "e5.5"),
    Step("D7.compact_rows", "compact rows in the primary list (one line per item)",
         "Buys three more visible episodes; the sparkline and relative time give way "
         "first (e1.6's row columns).", "e5.5"),
    Step("D8.content_trim", "trimmable body lines (glance mode, previews)",
         "Only panes that declare trimmable rows may use this: a glance card can show "
         "four facts instead of seven, an episode card must not.", "e5.1"),
)

#: Beyond the ladder: the two terminal states.
FALLBACK = Step("D9.single_pane", "single-pane fallback: secondary panes dropped",
                "The status line names the dropped panes and the key that restores "
                "them; content is moved, never deleted.", "e5.5")
REFUSED = Step("D10.refuse", "refuse to draw, and say so",
               "Below this the tool would be a wall of ellipses. The panel states the "
               "size it needs, the size it got, and the two keys that help.", "e5.5")


#: Drawing constants, so that the arithmetic in §3 and §6 is one set of numbers and not a
#: story. A border is one cell; panes are separated by one gutter column; a frame is two
#: borders plus the gutter between its two panes.
_FRAME_COLS = 1
_GUTTER = 1
#: Ambient has no frame to sell, so its refusal floor is its own line budget: the longest
#: line it may emit is 40 columns, which is what ADR-001 §4.1's "fits a 40×3 tmux pane"
#: promise costs. Below 40 columns there is no honest ambient line, so it refuses.
_AMBIENT_REFUSE_COLS = 40
#: Columns a status line must have before it can afford to spell the degradation notice out
#: in full. Below this the notice is abbreviated ("4 dropped") rather than clipped.
_NOTICE_COLS = 96
#: The default floor for a pane with a list in it: fewer content rows than this and a
#: view has nothing left to say. Per-view override: `View.min_content_rows`.
MIN_CONTENT_ROWS = 3


def chrome_rows(view: View, applied: tuple[str, ...] = ()) -> int:
    """Chrome rows still standing after the listed ladder steps. The status line is never
    a candidate for the ladder: it is where the degradation notice lives (§6.4).

    `view.min_rows` is a *frame* height — chrome included — so this function and `fit`
    speak the same unit. A boxed frame is a top rule and a bottom rule; after D4 the top
    rule survives as the pane header, which is the frame's one remaining row.
    """
    gone = set(applied)
    rows = 0
    if "frame" in view.chrome:
        rows += 1 if "D4.frame" in gone else 2
    if "tabs" in view.chrome and "D2.tabs" not in gone:
        rows += 1
    if "strips" in view.chrome and "D3.strips" not in gone:
        rows += 1
    if "hint" in view.chrome and "D1.hint" not in gone:
        rows += 1
    if "status" in view.chrome:
        rows += 1
    return rows


def min_cols_view(view: View) -> int:
    """The width at which the pane set still renders *as documented*: every pane at its
    documented minimum width. Derived from the pane table, never declared — and honest
    about the frame, so a borderless view is not charged for a border it never draws."""
    panes = [p for p in view.panes if p.role != "query"]
    return (sum(p.min_cols for p in panes) + _frame_cols(view, ())
            + (len(panes) - 1) * _GUTTER)


def step_what(view: View, step: Step) -> str:
    """A step's `what`, specialised to this view where the number is per-view. D5's strip
    width is each pane's own `collapse_to`, so the table must not print a single made-up
    number for every view."""
    if step.id == "D5.secondary_strip":
        strips = sorted({p.collapse_to for p in view.secondaries if p.role == "secondary"})
        if not strips:
            return "no secondary pane: this step cannot fire here"
        return "secondary pane%s squeezed to a %s-column strip" % (
            "s" if len(strips) > 1 else "", "/".join(str(w) for w in strips))
    return step.what


def ladder_rows(view: View) -> int:
    return sum(s.recover(view)[1] for s in LADDER)


def refuse_below_cols(view: View) -> int:
    """Below this width the view refuses.

    The number is the primary pane's hard minimum taken *without* its frame, because the
    frame is sold two steps earlier on the ladder: `Queue` refuses at 32 columns because
    a worklist narrower than that cannot show `sha · age · author kind · +n −n · risk`
    at all, and there is nothing left on the ladder to sell. Ambient has no frame to
    sell, so its floor is its own line budget — see §6.2.
    """
    if view.id == "ambient":
        return _AMBIENT_REFUSE_COLS
    return view.primary.hard_min


def refuse_below_rows(view: View) -> int:
    """Below this many *frame* rows the view refuses: the chrome left standing once the
    ladder has sold all of it (the status line, one row) plus the view's own floor for
    content. For `Queue` that is 4 — a frame with no room for a sentence — and for
    `Ambient` it is 3, which is ADR-001 §4.1's tmux claim, paid for in full."""
    return 1 + view.min_content_rows


def floor_cols(view: View) -> int:
    """The narrowest width at which the *documented pane set* still renders (D9 not
    needed). Clamped at the single-pane floor: a one-pane view has no secondary to give."""
    return max(min_cols_view(view) - sum(s.recover(view)[0] for s in LADDER),
               refuse_below_cols(view) + 1)


def floor_rows(view: View) -> int:
    return max(view.min_rows - ladder_rows(view), refuse_below_rows(view))


def ladder_boundary(view: View) -> list[tuple[Step, str]]:
    """The size at which each step first fires, computed by asking `fit`. This is the
    per-view degradation table ADR-006 §6.2 prints: nobody can keep those numbers in
    their head, and a table typed by hand is a table that lies."""
    out: list[tuple[Step, str]] = []
    for step in LADDER:
        at_c = at_r = None
        for c in range(view.ideal_cols, refuse_below_cols(view) - 1, -1):
            if step.id in fit(view, c, view.ideal_rows).applied:
                at_c = c
                break
        for r in range(view.ideal_rows, refuse_below_rows(view) - 1, -1):
            if step.id in fit(view, view.ideal_cols, r).applied:
                at_r = r
                break
        out.append((step, _fmt(at_c, at_r)))
    at = _first(view, FALLBACK.id)
    out.append((FALLBACK, "cols ≤ %d" % at[0] if at[0] else "—"))
    return out


def _first(view: View, step_id: str) -> tuple[int, int]:
    for c in range(view.ideal_cols, refuse_below_cols(view) - 1, -1):
        if step_id in fit(view, c, view.ideal_rows).applied:
            return (c, view.ideal_rows)
    return (0, 0)


def _fmt(at_c: int | None, at_r: int | None) -> str:
    bits = []
    if at_c:
        bits.append("cols ≤ %d" % at_c)
    if at_r:
        bits.append("rows ≤ %d" % at_r)
    return ", ".join(bits) or "—"


@dataclass
class Fit:
    """The outcome of asking the ladder whether a view fits. ADR-006 §6."""
    view: View
    cols: int
    rows: int
    applied: list[str] = field(default_factory=list)
    state: str = "complete"          # complete | degraded | single | refused
    deficit: tuple[int, int] = (0, 0)

    @property
    def dropped(self) -> list[str]:
        """What left the frame, in the order the ladder sold it, in this view's own
        numbers (D5's strip width is per-pane, so a shared string would be a guess)."""
        return [step_what(self.view, s) for s in LADDER if s.id in self.applied] + \
               ([FALLBACK.what] if "D9.single_pane" in self.applied else [])

    @property
    def notice(self) -> str:
        """The status-line notice. ADR-001 §7.2: suppressed information is the one
        failure mode a review tool is not allowed, so nothing the ladder sells may go
        unsaid."""
        if self.state == "complete":
            return ""
        if self.state == "refused":
            return ("LAYOUT REFUSED: %s needs %d×%d minimum, got %d×%d — `a` ambient, "
                    "`q` queue, or `fathom --plain`"
                    % (self.view.title, refuse_below_cols(self.view),
                       refuse_below_rows(self.view), self.cols, self.rows))
        return "LAYOUT %s: %s · <C-w>o" % (self.state, ", ".join(self.dropped))

    def summary(self) -> str:
        if self.state == "complete":
            return "complete"
        if self.state == "refused":
            return "refused below %d×%d" % (refuse_below_cols(self.view),
                                            refuse_below_rows(self.view))
        return "%s — %s" % (self.state, ", ".join(self.dropped))


def _frame_cols(view: View, applied: tuple[str, ...]) -> int:
    """Columns the frame itself costs right now: two while a box is drawn, zero once the box
    is sold — and zero always for a borderless view, which never had them."""
    if "frame" not in view.chrome or "D4.frame" in applied:
        return 0
    return 2 * _FRAME_COLS


def fit(view: View, cols: int, rows: int) -> Fit:
    """Ask the ladder, in order, until the view fits. `e1.5` calls exactly this on
    resize; the ADR's per-view table is its output.

    The terminal states are decided on facts, not on the running deficit: after the
    ladder has sold what it can sell, the primary pane must still reach its hard minimum
    width and three content rows. Below that, drawing is a lie — a worklist that cannot
    show `sha · age · author kind · +n −n · risk` is not a narrow worklist, it is an
    ellipsis with a header.
    """
    out = Fit(view, cols, rows)
    need_c = min_cols_view(view) - cols
    need_r = view.min_rows - rows          # both numbers are frame heights
    if need_c <= 0 and need_r <= 0:
        return out

    for step in LADDER:
        if need_c <= 0 and need_r <= 0:
            break
        saved_c, saved_r = step.recover(view)
        if (saved_c > 0 and need_c > 0) or (saved_r > 0 and need_r > 0):
            out.applied.append(step.id)
            need_c -= saved_c
            need_r -= saved_r

    # the single-pane fallback: secondary panes leave the frame entirely, and the status
    # line names them (§6.1 D9)
    if need_c > 0 or need_r > 0:
        saved_c = sum(p.collapse_to + 1 for p in view.secondaries if p.role == "secondary")
        if saved_c > 0:
            out.applied.append(FALLBACK.id)
            need_c -= saved_c
            out.state = "single"

    avail_c = cols - _frame_cols(view, tuple(out.applied))
    avail_r = rows - chrome_rows(view, tuple(out.applied))
    out.deficit = (max(0, need_c), max(0, need_r))
    if avail_c < view.primary.hard_min or avail_r < view.min_content_rows:
        out.state = "refused"
        out.applied.append(REFUSED.id)
    elif out.state != "single":
        out.state = "degraded"
    return out


def pane_widths(view: View, cols: int, applied: tuple[str, ...]) -> list[tuple[Pane, int]]:
    panes = [p for p in view.panes if p.role != "query"]
    frame = 0 if ("D4.frame" in applied or "frame" not in view.chrome) else 2 * _FRAME_COLS
    if "D9.single_pane" in applied:
        # one pane, no gutters: it takes the whole inside of whatever is left
        return [(panes[0], max(1, cols - frame))]
    avail = cols - frame - (len(panes) - 1) * _GUTTER
    hard = "D6.cols_hard" in applied
    targets = []
    for p in panes:
        t = p.ideal_cols
        if "D5.secondary_strip" in applied and p.role == "secondary":
            t = min(t, p.collapse_to)
        targets.append(min(t, p.ideal_cols))
    # Floors, per pane, per step: a secondary that has been sold down to its documented
    # strip width (§5's `Strip` column) is *at* its floor in that form — D6 does not push
    # a strip below its own width, it only squeezes full panes to their hard minimum.
    floor = []
    for p in panes:
        if "D5.secondary_strip" in applied and p.role == "secondary":
            floor.append(p.collapse_to)
        else:
            floor.append(p.hard_min if hard else p.min_cols)
    # shrink: secondaries give width up first, then the primary pane
    while sum(targets) > avail:
        order = sorted(range(len(panes)),
                       key=lambda i: (0 if panes[i].role == "secondary" else 1,
                                      -targets[i]))
        moved = False
        for i in order:
            if targets[i] > floor[i]:
                targets[i] -= 1
                moved = True
                break
        if not moved:
            break
    # grow (ADR-006 §3): spare columns go to the primary pane until it is twice the
    # widest secondary, then split evenly. "It just stretches" is not a rule.
    while sum(targets) < avail:
        widest = max(t for p, t in zip(panes, targets) if p.role != "primary") \
            if len(panes) > 1 else 0
        primary = next(i for i, pn in enumerate(panes) if pn.role == "primary")
        if widest and targets[primary] < 2 * widest:
            targets[primary] += 1
        else:
            for i in range(len(panes)):
                if sum(targets) >= avail:
                    break
                targets[i] += 1
    return list(zip(panes, targets))


# ---------------------------------------------------------------------------
# rendering
# ---------------------------------------------------------------------------
def _body_lines(pane: Pane, width: int, rows: int, applied: tuple[str, ...],
                show_strip: bool) -> list[str]:
    """The pane's content, top-aligned and padded to `rows`.

    The last `sample` line of a pane that declares a strip is that strip's text (§6.1 D3),
    so it is drawn in the strip row, not in the body — a sample line that silently changed
    rows between the documented layout and a degraded one would make the ladder lie.
    """
    lines = list(pane.sample)
    if show_strip and pane.strip and len(lines) > 1:
        lines = lines[:-1]
    if "D8.content_trim" in applied and pane.trim_rows:
        lines = lines[:max(1, len(lines) - pane.trim_rows)]
    if "D7.compact_rows" in applied and pane.kind == "list":
        lines = [l for l in lines if "✱" not in l][:max(1, len(lines) - 3)]
    body = [fit_text(l, width) for l in lines[:rows]]
    while len(body) < rows:
        body.append(" " * width)
    return body


def _strip_line(pane: Pane, width: int) -> str:
    """A pane's footer strip: legend, counts, the ordering explanation (§5's chrome)."""
    if not pane.strip or len(pane.sample) < 2:
        return " " * width
    return fit_text(pane.sample[-1], width)


def _join_unboxed(segs: list[str], widths: list[int], cols: int) -> str:
    """Side-by-side panes with no frame: a column rule between them, and nothing else — no
    leading space, because a borderless view has no column to spend on decoration and
    Ambient's 40-column budget is the whole point of §5.7."""
    if len(segs) == 1:
        return fit_text(segs[0], cols)
    return fit_text((" " + (" " + BOX["v"] + " ").join(segs)).rstrip(), cols)


def draw(view: View, cols: int | None = None, rows: int | None = None,
           applied: tuple[str, ...] | None = None) -> list[str]:
    """Draw `view` at `cols`×`rows`. Every returned line is exactly `cols` cells wide,
    which is the property the test suite asserts and `e5.5`'s screenshot tests reuse."""
    cols = cols or view.ideal_cols
    rows = rows or view.ideal_rows
    f = fit(view, cols, rows)
    use = tuple(applied) if applied is not None else tuple(f.applied)

    if REFUSED.id in use:
        return _refused(view, cols, rows)

    out: list[str] = []
    if "tabs" in view.chrome and "D2.tabs" not in use:
        out.append(_tabs_line(view, cols))
    query_rows = sum(1 for p in view.panes if p.role == "query")
    for p in view.panes:
        if p.role == "query":
            out.append(fit_text(p.sample[0], cols))
    # `chrome_rows` is the whole non-content budget: frame, tab bar, the footer strip row,
    # status, hint. Whatever is left is body, and the drawn frame is then exactly `rows`
    # tall — the property `test_drawn_layouts_fit_their_documented_size` holds us to.
    body_rows = max(1, rows - chrome_rows(view, use) - query_rows)
    widths = pane_widths(view, cols, use)
    # A borderless view never draws a box, so it never has D4's two columns to sell
    boxed = "frame" in view.chrome and "D4.frame" not in use
    show_strip = "strips" in view.chrome and "D3.strips" not in use

    if boxed:
        segs = [fit_text("─ " + pane.name, w) for pane, w in widths]
        out.append(fit_text(BOX["tl"] + BOX["tc"].join(segs) + BOX["tr"], cols))
    else:
        segs = [fit_text(p.name, w) for p, w in widths]
        out.append(_join_unboxed(segs, [w for _p, w in widths], cols))

    bodies = [_body_lines(p, w, body_rows, use, show_strip) for p, w in widths]
    for i in range(body_rows):
        seg = [bodies[j][i] for j, (_pane, _w) in enumerate(widths)]
        if boxed:
            out.append((BOX["v"] + BOX["v"].join(seg) + BOX["v"])[:cols])
        else:
            out.append(_join_unboxed(seg, [w for _p, w in widths], cols))

    if show_strip:
        seg = [_strip_line(p, w) for p, w in widths]
        if boxed:
            out.append((BOX["v"] + BOX["v"].join(seg) + BOX["v"])[:cols])
        else:
            out.append(_join_unboxed(seg, [w for _p, w in widths], cols))

    if boxed:
        foot = BOX["bl"] + BOX["bc"].join(BOX["h"] * w for _pane, w in widths) + BOX["br"]
        out.append(fit_text(foot, cols))

    if "status" in view.chrome:
        out.append(fit_text(_status_line(view, f, cols), cols))
    if "hint" in view.chrome and "D1.hint" not in use:
        out.append(fit_text(_hint_line(view), cols))
    return out[:rows]


def _refused(view: View, cols: int, rows: int) -> list[str]:
    """Short lines on purpose: a refusal panel that truncates its own advice is worse than
    none. Every line here fits the widths at which it can appear, and the advice is
    generated from the registry rather than typed, so "press `a`" cannot outlive its number.
    """
    ambient = next(v for v in VIEWS if v.id == "ambient")
    lines = [
        "too small for %s" % view.title,
        "need %d×%d, have %d×%d"
        % (refuse_below_cols(view), refuse_below_rows(view), cols, rows),
        "",
        "`a` ambient (%d×%d)" % (refuse_below_cols(ambient), refuse_below_rows(ambient)),
        "`q` queue · `,` commands",
        "`fathom --plain` for text",
        "",
        "nothing was clipped.",
    ]
    out = [fit_text(l, cols) for l in lines[:max(1, rows)]]
    while len(out) < rows:
        out.append(fit_text("", cols))
    return out


def _tabs_line(view: View, cols: int) -> str:
    tabs = "1 ▣ Queue   2 Timeline   3 Diff · (T pins this view)"
    right = "fathom/pilot · main"
    pad = max(1, cols - disp_width(tabs) - disp_width(right))
    return fit_text(tabs + " " * pad + right, cols)


def _status_line(view: View, f: Fit, cols: int = 80) -> str:
    """The status line is the last chrome standing, and a degraded frame leads with the
    degradation notice: ADR-001 §7.2 says hidden suppression is the one failure a review
    tool is not allowed, and a notice that can be truncated away is a hidden suppression.

    So the notice comes first and is abbreviated to what fits rather than clipped: at a width
    that can afford it the line spells out what was dropped, at 80 columns it counts the
    drops and points at `?`. Either way the reader is told before they are shown.
    """
    bits: list[str] = []
    if f.state != "complete":
        full = "LAYOUT %s: %s · <C-w>o, ?" % (f.state, ", ".join(f.dropped))
        short = "LAYOUT %s · %d dropped · <C-w>o, ?" % (f.state, len(f.applied))
        bits.append(full if disp_width(full) + 20 <= cols else short)
    bits += ["⏺ epoch 41", "3 412 eps", "queue 7 open", "idx warm"]
    return " · ".join(bits)


def _hint_line(view: View) -> str:
    return ("q queue · t timeline · d diff · s structure · g graph · p deps · "
            "/ search · , commands · ? help")


__all__ = ["Fit", "LADDER", "FALLBACK", "REFUSED", "Step", "disp_width", "fit_text",
           "fit", "draw", "ladder_boundary", "pane_widths", "min_cols_view",
           "floor_cols", "floor_rows", "refuse_below_cols", "refuse_below_rows",
           "step_what",
           "chrome_rows"]
