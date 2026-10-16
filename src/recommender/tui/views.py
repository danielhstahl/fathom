"""The view inventory: pane layout, focus order, minimum size, drill edges and
degradation ladder for each of Fathom's nine views. **This module is the normative
table of ADR-006 §5–§6**; the ASCII layouts in the ADR are rendered from it
(`python3 -m recommender.tui doc --write`), so a layout in the document *is* the layout
the layout engine produces at that size rather than a drawing that drifted.

Three rules this module exists to make checkable (ADR-006 §5.0):

1. **Every view is usable at 80x24.** `ideal_cols <= 80` and `ideal_rows <= 24` for all
   nine views, and the renderer is measured at exactly that size in the test suite.
2. **Degradation is an order, not a vibe.** Each view declares its ideal size, the size
   at which its documented pane set stops fitting, and a hard minimum; the ladder in
   :mod:`recommender.tui.render` is one fixed sequence applied until the view fits. If
   it does not fit, the view falls back rather than clipping, and says what it did
   (ADR-001 §7.2, "never hide silently").
3. **A view is a presentation of the delta model, never a second analysis.** Every pane
   names the `EpisodeDelta` fields it reads (`e2.11`), and no pane may show a number it
   cannot cite. That column is here, in the ADR, and the test asserts it is never empty.

**Content ordering rule.** A pane's `sample` lines are ordered by decreasing importance
and no line is wider than its pane at the documented size — the test enforces both.
Every ladder step that drops content drops it from the end, which is what makes "what do
I lose when the terminal shrinks" answerable without reading the renderer.
"""

from __future__ import annotations

from dataclasses import dataclass


#: A boxed view's chrome. Ambient and Search are borderless by design, so they own only
#: the status line and cannot sell what they never had (§6.2).
BOXED_CHROME: tuple[str, ...] = ("frame", "tabs", "strips", "status", "hint")
BORDERLESS_CHROME: tuple[str, ...] = ("status",)

@dataclass(frozen=True)
class Pane:
    """One pane of a view. `focus_order` is the position in the view's focus ring:
    `<Tab>` / `<C-w>l` walk it, `<Enter>` drills along it, and pane 1 is where focus
    lands when the view is entered (ADR-006 §6.0)."""

    name: str
    role: str                 # "primary" | "secondary" | "query"
    focus_order: int
    ideal_cols: int           # width in the documented (ideal) layout
    min_cols: int             # narrower than this and the pane drops its own columns
    collapse_to: int          # cols it keeps when the ladder collapses it to a strip
    kind: str                 # "list" | "body" | "canvas" | "treemap" | "table" | "field"
    reads: tuple[str, ...]    # fields of the delta model it renders (e2.11)
    ticket: str
    hard_min: int = 0         # absolute narrowest this pane may be drawn at all
    trim_rows: int = 0        # content rows this pane may drop (glance and preview panes)
    strip: bool = True        # has a one-line footer (legend / counts) the ladder drops
    sample: tuple[str, ...] = ()

    @property
    def interaction(self) -> str:
        """The interaction mode this pane takes focus into (`keymap.MODES`)."""
        return "list" if self.kind in ("list", "table", "treemap") else "body"


@dataclass(frozen=True)
class View:
    id: str
    key: str                  # the global key that opens it
    title: str
    kind: str                 # "view" | "overlay"
    question: str             # the question the reviewer arrives with
    ideal_cols: int           # the size ADR-006 draws the layout at
    ideal_rows: int
    min_rows: int             # smallest frame that still shows every pane
    panes: tuple[Pane, ...]
    drills: tuple[tuple[str, str, str], ...]      # (from, key, what happens)
    chrome: tuple[str, ...] = BOXED_CHROME
    #: Fewest content rows this view draws before refusing outright. Three for anything
    #: with a list in it; Ambient is two because its content is two lines by design, and
    #: that is what pays for ADR-001 §4.1's "fits a 40×3 tmux pane" claim.
    min_content_rows: int = 3
    owner: str = ""
    notes: str = ""

    @property
    def primary(self) -> Pane:
        return next(p for p in self.panes if p.role == "primary")

    @property
    def secondaries(self) -> tuple[Pane, ...]:
        return tuple(p for p in self.panes if p.role != "primary")

    def focus_ring(self) -> list[str]:
        return [p.name for p in sorted(self.panes, key=lambda p: p.focus_order)
                if p.role != "query"]


def _clip(text: str, width: int) -> str:
    """Clip mock content to the pane's documented width by dropping whole trailing
    words. `_p` applies it so the layouts can never show content that would not fit:
    a layout that lies about its own width is worse than a layout with less text."""
    import unicodedata

    def w(t: str) -> int:
        return sum(0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c)
                   in ("W", "F") else 1 for c in t)

    if w(text) <= width:
        return text
    words = text.rstrip().split(" ")
    while words and w(" ".join(words)) > width:
        words.pop()
    out = " ".join(words).rstrip()
    while out and w(out) + 1 > width:
        out = " ".join(out.split(" ")[:-1]).rstrip()
    if not out:
        out = text[:max(0, width - 1)]
    return out + "…" + " " * max(0, width - w(out) - 1)


def _p(name, role, order, ideal, minimum, collapse, kind, reads, ticket, sample,
       strip=True, hard=0, trim=0):
    return Pane(name, role, order, ideal, minimum, collapse, kind, tuple(reads), ticket,
                hard or (26 if role == "primary" else 20), trim, strip,
                tuple(_clip(line, ideal) for line in sample))


# ---------------------------------------------------------------------------
# 1. Queue — the home view
# ---------------------------------------------------------------------------
QUEUE = View(
    id="queue", key="q", title="Queue", kind="view", owner="e4.5",
    question="What is the next thing I have to look at, and why that one?",
    ideal_cols=80, ideal_rows=24, min_rows=18,
    panes=(
        _p("worklist", "primary", 1, 48, 36, 18, "list",
           ("episode.identity", "episode.created_at", "episode.attribution.kind",
            "risk.score", "risk.reasons[]", "blast_radius.direct_count", "review.state",
            "noise.share"), "e4.5",
           ("▸ e8f1a2 0:12 agent  +412 −90   risk 82",
            "        ✱ new cycle store→core→store (high)",
            "  9c4d10 0:41 agent  +58 −12    risk 61",
            "        ✱ +288 source, no test surface (med)",
            "  2be77f 1:07 human  +8 −3      risk 34",
            "  77aa0c 2:20 agent  +1 204 −90 risk 77",
            "        ✱ lockfile: 3 upgrades, 1 licence",
            "  40c9b3 3:55 agent  +96 −0     risk 58",
            "        ✱ new public API, ratchet not updated",
            "  c1f5e2 5:12 ?      +22 −22    risk 21",
            "        author kind unknown, not bucketed",
            "  08d4b1 6:02 agent  +3 −1      risk 12  snoozed",
            "  5f2c99 9:40 agent  +61 −61    risk 25 disp ×3"
            "  ··· 4 more open · 2 snoozed · 1 stale",
            "order: risk × staleness × blast radius (`w`)"),
           hard=32),
        _p("card", "secondary", 2, 29, 24, 16, "body",
           ("delta.structure.*", "delta.dependencies.*", "delta.non_code.*",
            "delta.tests.*", "blast_radius.*", "findings[].severity", "intent.*",
            "review.state"), "e2.10",
           ("e8f1a2 · 4 commits · agent",
            "0:12 ago · session 4 turn 12",
            "",
            "what  12 files: 9 src, 2 test,",
            "      1 lockfile · noise 6 %",
            "where src/store (7) · src/core (3)",
            "deps  +1 zstd direct · +3 trans",
            "shape +1 import edge · new cycle ✱",
            "tests +94 test vs +288 source",
            "reach 11 direct · 34 one-hop",
            "risk  82 = .4·size + .3·test + .3·blast",
            "      `w` vector · `i` intent",
            "",
            "x accept · f flag · - · z"),
           hard=20),
    ),
    drills=(("worklist · <Enter>", "card", "drill in: the episode card, pane 2"),
            ("worklist · x f - z", "ledger", "disposition; the worklist advances"),
            ("anywhere · d s g p", "that view", "same scope, different question"),
            ("card · <Esc>", "worklist", "pop back; the selection is preserved")),
    notes="Queue is the home view (ADR-001 §4.1). The worklist must explain its own "
          "ordering, so its strip names the ranking function and `w` opens the feature "
          "vector exactly as `fathom why` prints it (`e4.4`).")

TIMELINE = View(
    id="timeline", key="t", title="Timeline", kind="view", owner="e1.6",
    question="What happened, in what order, and which of it have I not read?",
    ideal_cols=80, ideal_rows=24, min_rows=16,
    panes=(
        _p("rail", "primary", 1, 42, 32, 18, "list",
           ("episode.identity", "episode.created_at", "episode.attribution.kind",
            "delta.structure.*", "review.state", "episode.members[].sha"), "e1.6",
           ("Tue 14 Oct · session 4 (pi) ── 6 episodes",
            "▸ e8f1a2 0:12 agent  +412 −90 ●●●●○○ 82 ⚑",
            "  9c4d10 0:41 agent  +58 −12  ●●●○○○ 61 ⚑",
            "  2be77f 1:07 human  +8 −3    ●○○○○○ 34 ✓",
            "Mon 13 Oct · session 3 (pi) ── 11 episodes",
            "  77aa0c 2:20 agent  +1 204 −90 ●●●●○○ 77 ✓",
            "  40c9b3 3:55 agent  +96 −0   ●●●○○○ 58 ⚑",
            "  c1f5e2 5:12 ?      +22 −22  ●○○○○○ 21 ?",
            "Sun 12 Oct ── 19 episodes  (gg/G jumps)",
            "  08d4b1 6:02 agent  +3 −1    ●○○○○○ 12 ✓",
            "  5f2c99 9:40 agent  +61 −61  ●●○○○○ 25 ×",
            "  ··· 17 more",
            "filter: area=any state=unread (F edits)"),
           hard=28),
        _p("card", "secondary", 2, 35, 26, 16, "body",
           ("delta.structure.*", "delta.dependencies.*", "delta.architecture.*",
            "findings[].{detector_id,severity}", "risk.*", "blast_radius.*"), "e2.10",
           ("e8f1a2 — make the store epoch-safe",
            "4 commits · agent 0.94 · 0:12 ago",
            "",
            "structure  +2 files: epoch.rs,",
            "           tests/test_epoch.py",
            "           rename cache.rs (0.92)",
            "shape      +1 edge · new cycle ✱",
            "           store→core→store (e3.5)",
            "deps       +1 zstd direct · +3 transitive",
            "tests      +94 test vs +288 src (0.33)",
            "surfaces   CI workflow · no migration",
            "findings   new-cycle (high)",
            "           untested-addition (med)",
            "           test-weakening (low)"),
           hard=22),
    ),
    drills=(("rail · <Enter>", "card", "drill in: card for the selected episode"),
            ("card · <Enter>", "Diff", "a second drill opens the diff at that episode"),
            ("<Esc>", "rail / previous view", "pop one level, then pop out")),
    notes="The rail groups by day or by agent session. Selection survives a re-index and "
          "re-attaches after a rebase (`e1.6`, ADR-003): the highlight follows the "
          "*episode*, not the row index.")

DIFF = View(
    id="diff", key="d", title="Diff", kind="view", owner="e1.7",
    question="What exactly changed in this file, and is it what the card claims?",
    ideal_cols=80, ideal_rows=24, min_rows=15,
    panes=(
        _p("tree", "primary", 1, 28, 20, 16, "list",
           ("delta.structure.* (file list)", "path taxonomy class", "noise class",
            "per-file +/- counts", "review state"), "e1.7",
           ("12 files · +412 −90 · noise 24",
            "▾ src/store/ +388 −84",
            "    epoch.rs   new +120 −0",
            "    cache.rs   ~   +96 −80",
            "    index.rs   ~   +58 −4",
            "    schema.rs  ~   +14 −0",
            "▾ src/core/ +18 −6",
            "    types.rs   ~   +18 −6",
            "▾ tests/ +94 −0",
            "    test_epoch.rs new +94 −0",
            "  Cargo.lock gen +6 −0 [n]",
            "  store.h    mode 644→755",
            "  blob.png   bin 4 096 B (raw)",),
           hard=18),
        _p("hunks", "secondary", 2, 49, 30, 24, "body",
           ("hunk text", "evidence ids", "word-diff", "whitespace policy"), "e1.7",
           ("src/store/epoch.rs  new +120 −0   ] next hunk",
            "@@ -0,0 +1,10 @@",
            "+pub fn epoch_is_live(row: &Row, now: Epoch) -> bool {",
            "+    row.epoch == now.epoch && row.key == now.key",
            "+}",
            "+",
            "+/// Mark-dead, never delete (ADR-004 §6.2).",
            "+pub fn mark_dead(rows: &mut [Row], at: u64) {",
            "-    rows.retain(|r| r.epoch == at);",
            "+    for r in rows { r.dead_at = Some(at); }",
            "+}",
            "",
            "word-diff `W`: retain → for-loop (whitespace off)",
            "evidence ev-1 epoch.rs:1-10 · ev-9 graph 88 114",
            "noise: 1 file hidden by `lockfile churn` (b)"),
           hard=26),
    ),
    drills=(("tree · <Enter>", "hunks", "read that file's hunks (focus moves)"),
            ("hunks · [ ]", "hunks", "previous / next hunk"),
            ("hunks · E", "$EDITOR", "hand off at file:line (ADR-001 §8 non-goal 2)"),
            ("tree · b", "noise view", "the suppressed files, with the rule printed"),
            ("<Esc>", "tree / previous view", "one level up, then pop out")),
    notes="`\\` toggles split/unified. Split needs ~100 columns; below that the view "
          "degrades to unified rather than clipping a side in half (ADR-006 §6.3).")

STRUCTURE = View(
    id="structure", key="s", title="Structure", kind="view", owner="e2.3",
    question="Where did it land, and is the tree hottest where it already was?",
    ideal_cols=80, ideal_rows=24, min_rows=15,
    panes=(
        _p("treemap", "primary", 1, 52, 34, 20, "treemap",
           ("delta.structure.*", "churn heat per subtree", "per-subtree +/-",
            "noise.share"), "e2.3",
           ("┌ src/ ────────────────────── 82 % of change · heat ●●●● ┐",
            "│ ┌ store/ ─────── 64 % ●●●● ┐ ┌ core/ ──── 14 % ●● ──┐ │",
            "│ │ epoch.rs +120 (new)      │ │ types.rs +18 −6      │ │",
            "│ │ cache +176 · index +54   │ └──────────────────────┘ │",
            "│ │ schema +14               │ ┌ tests/ ──── 4 % ● ───┐ │",
            "│ └──────────────────────────┘ │ test_epoch +94       │ │",
            "│                              └───────────────────────┘│",
            "└───────────────────────────────────────────────────────┘",
            "▾ src/store  +388 −84  churn 0.71  files 4  (`<` sort)",
            "  src/core    +18 −6   churn 0.22  files 1",
            "  tests       +94 −0   churn 0.41  files 1",
            "  schemas      +6 −0   noise       files 1 (generated)"),
           hard=30),
        _p("detail", "secondary", 2, 25, 20, 16, "body",
           ("delta.structure.* (subtree rows)", "renames with confidence",
            "path taxonomy class", "hotspot rank"), "e2.3",
           ("selected src/store · lvl 2 of 4",
            "",
            "files   4 touched · 1 new",
            "rename  cache.rs → store/cache.rs",
            "        conf 0.92 (e2.4)",
            "lines   +388 −84 · noise 3 %",
            "churn   0.71 over 30 eps",
            "hotness rank 2 of 214",
            "moved   none · `[` up · `]` down"),
           hard=18),
    ),
    drills=(("treemap · <Enter>", "deeper", "drill one directory level (focus stays)"),
            ("[ / ]", "level", "aggregate up / down — this view's shrink mechanism"),
            ("<", "sort", "cycle churn · size · delta"),
            ("<Esc>", "level up", "one level out, then pop to the previous view")),
    notes="A 3 000-node tree stays legible because aggregation, not clipping, is how it "
          "shrinks (`e3.4`: “degrades by aggregation, not by clipping”). Heat is churn, "
          "and it always carries a glyph as well as a colour (`e5.6`).")

GRAPH = View(
    id="graph", key="g", title="Graph", kind="view", owner="e3.4",
    question="Did this turn make the shape of the system worse?",
    ideal_cols=80, ideal_rows=24, min_rows=16,
    panes=(
        _p("canvas", "primary", 1, 54, 36, 20, "canvas",
           ("delta.architecture.*", "module graph nodes/edges", "cycle set",
            "layering violation delta", "blast_radius.*"), "e3.4",
           ("layered · hop 2 · delta overlay ON `!` · L2 `[`/`]`",
            "  ┌ fathom-cli ┐        ┌ fathom-analyze ┐",
            "  │  main.rs   │──────▶│ insight        │",
            "  └─────┬──────┘        └───────┬────────┘",
            "        │ ╳ new edge            │",
            "  ┌─────▼──────┐        ┌───────▼────────┐",
            "  │fathom-store│╌╌╌╌╌╌╌│ fathom-core    │",
            "  └────────────┘ ghosted (removed edge)",
            "        ✱ new cycle: store → core → store",
            "",
            "edges +1 −1 · cycles +1 · layering +0 · debt 0",
            "nodes 214 of 1 902 shown · aggregated at L2 (`]`)"),
           hard=30),
        _p("detail", "secondary", 2, 23, 20, 16, "body",
           ("node/edge evidence ids", "delta.architecture.*", "cycle debt",
            "layering rules", "public API delta"), "e3.4",
           ("fathom-store · selected",
            "",
            "fan-in   4 (+1 this episode)",
            "fan-out  7 (+0)",
            "cycles   1 new · debt 0",
            "layering L2→L3 ok",
            "public   +2 exported fns",
            "         semver minor (e3.7)",
            "evidence lib.rs:14 · graph 88 114"),
           hard=18),
    ),
    drills=(("canvas · <Enter> node", "detail", "focus the detail pane on that node"),
            ("canvas · <Enter> edge", "Diff", "the file-and-line that produced the edge"),
            ("[ / ]", "aggregation", "module level up/down (3 000 nodes → legible)"),
            ("< / >", "hops", "blast-radius hop limit"),
            ("!", "overlay", "show only this episode's delta overlay"),
            ("<Esc>", "up", "one level out of the selection")),
    notes="`graph-layout` is a **non-default feature** (ADR-002 §6.2): without it the "
          "view renders the aggregated node table and says “graph layout not compiled "
          "in”, never a blank canvas.")

DEPS = View(
    id="deps", key="p", title="Deps", kind="view", owner="e2.6",
    question="What does the code now depend on that it did not?",
    ideal_cols=80, ideal_rows=24, min_rows=15,
    panes=(
        _p("manifest", "primary", 1, 40, 32, 18, "table",
           ("delta.dependencies.*", "licence set delta", "advisory hits (opt-in)",
            "manifest/lockfile evidence"), "e2.6",
           ("rust · Cargo.lock · +1 dir +3 trans  `[`/`]`",
            "▸ zstd   0.13.2  NEW       direct     MIT",
            "  serde  1.0.219 ←1.0.210 direct  Apache/MIT",
            "  libc   0.2.162 ←0.2.160  transitive",
            "  adler  1.0.2  NEW       transitive zstd",
            "  miniz  0.3.8  NEW       transitive zstd",
            "  serde_json 1.0.134 ←.133 transitive",
            "",
            "licence +0 new (MIT already present)",
            "integrity 0 · source switches 0 · removed 0",
            "advisories off — no network consent (e6.8)"),
           hard=28),
        _p("tree", "secondary", 2, 37, 26, 16, "list",
           ("resolved dependency graph", "reverse dependencies", "lockfile line ids"),
           "e2.6",
           ("zstd 0.13.2 · why is it here? (u: trans)",
            "",
            "  fathom-store Cargo.toml:28 [direct]",
            "   └─ zstd 0.13.2               [NEW]",
            "       ├─ adler 1.0.2           [NEW]",
            "       └─ miniz 0.3.8           [NEW]",
            "",
            "reverse store::blobs, store::wal (2)",
            "evidence Cargo.lock:412-418, :977"),
           hard=22),
    ),
    drills=(("manifest · <Enter>", "tree", "why is this dependency here"),
            ("[ / ]", "ecosystem", "next / previous ecosystem in scope"),
            ("u", "filter", "show transitive-only changes only"),
            ("<Esc>", "manifest", "one level up")),
    notes="Advisory and provenance lookup is network-consent gated (ADR-001 §4.6 "
          "*network consent*), so the pane prints `off` with the key that turns it on "
          "rather than an empty column.")

AMBIENT = View(
    id="ambient", key="a", title="Ambient", kind="view", owner="e5.1",
    question="Anything I need to look at right now? (glance, do not study)",
    ideal_cols=80, ideal_rows=12, min_rows=10,
    chrome=BORDERLESS_CHROME, min_content_rows=2,
    panes=(
        _p("glance", "primary", 1, 80, 40, 40, "body",
           ("episode.identity", "risk.score", "review.state", "queue counts",
            "index epoch (ADR-004 §6.3)", "caught-up state"), "e5.1",
           ("latest e8f1a2 · agent · 0:12 · risk 82 ✱",
            "queue  7 open · 2 snoozed · 1 over 24 h",
            "index  epoch 41 · warm · 3 412 eps · 0:03 ago",
            "",
            "look now  e8f1a2 adds a cycle and 288 untested source lines",
            "",
            "        <Enter> open it · q queue · t timeline · ? help",
            "        quiet: nothing new since 0:03 · no action needed"),
           strip=False, hard=40, trim=5),
    ),
    drills=(("<Enter>", "card", "the one keypress ADR-001 §4.1 promises"),
            ("q / t", "view", "leave glance mode")),
    notes="Ambient is a *display mode*, not a second data path: it renders the same "
          "delta model with the chrome dropped. It is the floor every other view "
          "degrades toward, the only view that must work at 80×12 (ADR-001 §4.1), and "
          "it goes quiet and dim when `caught_up` rather than staying chatty.")

SEARCH = View(
    id="search", key="/", title="Search", kind="overlay", owner="e5.2",
    question="Where is the thing I am thinking of?",
    ideal_cols=72, ideal_rows=14, min_rows=12,
    chrome=BORDERLESS_CHROME,
    panes=(
        _p("query", "query", 1, 72, 48, 0, "field",
           ("(input) free text plus filter tokens author: path: risk> dep:",), "e5.2",
           ("/  store epoch   author:agent risk>70▏"), strip=False),
        _p("results", "primary", 2, 46, 30, 18, "list",
           ("episode summaries", "paths", "symbol names", "dependency names",
            "finding types"), "e5.2",
           ("episodes (3) ─ stable order (corpus n=3 412)",
            "▸ e8f1a2 0:12 agent +412 −90 “epoch-safe” 82",
            "  40c9b3 3:55 agent +96 −0  ratchet epoch 58",
            "  11d0aa 2 d  human +4 −1  epoch typo docs 12",
            "symbols (2) ── fathom-store::epoch_is_live new",
            "  fathom-store::IndexEpoch fn · 2 uses",
            "findings (1) ─ new-cycle store→core (high)",
            "paths (4) ──── src/store/epoch.rs new",
            "<C-n>/<C-p> walk · <Enter> jumps into the view"),
           hard=26),
        _p("preview", "secondary", 3, 23, 20, 16, "body",
           ("episode card excerpt", "match explanation", "evidence locator"), "e5.2",
           ("why this matched:",
            " store = subject (title 0.9)",
            " epoch = store/epoch.rs",
            " author:agent ✓ risk>70",
            "",
            "3 hits · scored in 4 ms",
            "deterministic (e5.2)"),
           hard=18),
    ),
    drills=(("<C-n> / <C-p>", "results", "walk results without leaving the field"),
            ("<Enter>", "target view", "deep link: right view, right selection"),
            ("<Esc>", "dismiss", "the view behind keeps its selection")),
    notes="An overlay: it floats over the view it was opened from, and that view keeps "
          "its selection so `<Esc>` returns you exactly where you were. `n`/`N` walk "
          "matches after it closes.")

ARTEFACT = View(
    id="artefact", key="o", title="Artefact", kind="view", owner="e5.7",
    question="What would my review record say, before I make it permanent?",
    ideal_cols=72, ideal_rows=16, min_rows=14,
    panes=(
        _p("preview", "primary", 1, 49, 34, 20, "body",
           ("review ledger entries", "episode cards", "findings with evidence",
            "operator-action checklist", "sign-off block"), "e5.7",
           ("# Review 77aa0c..e8f1a2 — 6 episodes · md (`=`)",
            "",
            "| episode | risk | disposition | reviewer |",
            "| ------- | ---- | ----------- | -------- |",
            "| 77aa0c  | 77   | accepted    | d.s.     |",
            "| 40c9b3  | 58   | flagged     | d.s.     |",
            "| c1f5e2  | 21   | dismissed: expected | d.s.|",
            "",
            "Signed off 5 · dismissed 1 · stale 0 · author ? 1",
            "Operator actions (1): deploy migration 0042",
            "Evidence 41 links · byte-stable · no abs paths"),
           hard=28),
        _p("scope", "secondary", 2, 20, 20, 16, "list",
           ("episode range", "dispositions", "export target", "format"), "e5.7",
           ("range 77aa0c..e8f1a2 `[`/`]`".replace(" `[`/`]`",""),
            "  ☑ 77aa0c accepted",
            "  ☑ 40c9b3 flagged",
            "  ☐ c1f5e2 no disposition",
            "format md·html·json `=`",
            "to .fathom/export/review.md",
            "byte-stable (e5.7 AC)"),
           hard=18),
    ),
    drills=(("<Enter>", "write", "write the file; the path is printed, not guessed"),
            ("[ / ]", "range", "narrow / widen the episode range"),
            ("=", "format", "cycle md / html / json"),
            ("<Esc>", "up", "leave the preview")),
    notes="The preview uses the same template as the CLI export — `export` is the narrow "
          "waist (ADR-002 §5.3) — so a preview that disagrees with the written file is a "
          "defect in `e5.7`, not a difference between surfaces.")


VIEWS: tuple[View, ...] = (QUEUE, TIMELINE, DIFF, STRUCTURE, GRAPH, DEPS, AMBIENT,
                           SEARCH, ARTEFACT)

#: The view keys, in keymap order. Asserted against `keymap.DEFAULTS` by the tests, so
#: the ticket's fixed list (`d s g p q a`) can never silently lose a view.
VIEW_KEYS: dict[str, str] = {v.key: v.id for v in VIEWS if v.kind == "view"}

#: Screenshot sizes `e5.5`'s matrix must cover, straight from its acceptance criteria.
LAYOUT_MATRIX: tuple[tuple[int, int], ...] = ((40, 10), (80, 24), (100, 30), (120, 40),
                                              (160, 50), (240, 60))

#: The chrome, in the order the ladder may take it away (ADR-006 §6). This is a declared
#: order, not a vibe: the sequence is "the line a learned user stopped reading first".
CHROME_ORDER: tuple[tuple[str, str, str], ...] = (
    ("hint", "hint line: the next likely actions", "e5.4"),
    ("tabs", "tab bar: the pinned views", "e1.5"),
    ("strips", "pane footer strips: legend, counts, ordering explanation", "e5.5"),
    ("frame", "box frame: top and bottom rules, side borders", "e5.5"),
    ("status", "status line: index epoch, counts, degradation notice", "e5.5"),
)

