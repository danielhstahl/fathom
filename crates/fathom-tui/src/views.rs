//! `fathom_tui::views` — the view registry: panes, focus rings, sizes and drill edges.
//!
//! This is ADR-006 §4 and §5 as data, generated from the same table the ADR prints
//! (`src/recommender/tui/views.py`; CI diffs the document with `recommender.tui doc --check`,
//! and `tests/layout.rs` pins what *this* copy derives). The shell owns the frame and the
//! focus model; the content of a pane belongs to the ticket named in its `ticket` field, and
//! the shell says so out loud rather than drawing a plausible-looking lie.
//!
//! Two rules the shape of this table enforces:
//!
//! * **A focus ring is data**, not a per-view habit: [`View::focus_ring`] is sorted by
//!   `focus_order`, pane 1 is where focus lands on entry, and a `query` row is the search
//!   field, which is never a focus stop (`<Tab>` in `prompt` walks *results*, not panes).
//! * **A pane names what it reads.** The `reads` list is the `EpisodeDelta` fields a pane may
//!   render (ADR-001 §7.10, `e2.11`): a pane that shows a number it cannot cite is a defect,
//!   and an empty `reads` would have been the same thing in a quieter font.

/// `frame`, `tabs`, `strips`, `status`, `hint` — the chrome rows a view may draw, as bits so
/// the ladder can sell them one at a time (ADR-006 §6.1).
pub const CH_FRAME: u8 = 1 << 0;
/// The tab bar.
pub const CH_TABS: u8 = 1 << 1;
/// The pane footer strips (legend, counts, the ordering explanation).
pub const CH_STRIPS: u8 = 1 << 2;
/// The status line — never a ladder candidate, because it is where the degradation notice lives.
pub const CH_STATUS: u8 = 1 << 3;
/// The hint line: the first thing the ladder sells.
pub const CH_HINT: u8 = 1 << 4;

/// Which of the nine views a state refers to (ADR-006 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewId {
    /// The home view: the ranked attention worklist.
    Queue,
    /// Chronological episode rail plus card.
    Timeline,
    /// File tree plus hunks.
    Diff,
    /// Directory treemap with churn heat.
    Structure,
    /// Module graph with the delta overlay.
    Graph,
    /// Manifest table plus transitive tree.
    Deps,
    /// Glanceable display mode.
    Ambient,
    /// The `/` overlay — a borrowed frame, not a mode.
    Search,
    /// The export preview.
    Artefact,
}

impl ViewId {
    /// All nine, in the order ADR-006 §4 lists them.
    pub const ALL: [ViewId; 9] = [
        ViewId::Queue,
        ViewId::Timeline,
        ViewId::Diff,
        ViewId::Structure,
        ViewId::Graph,
        ViewId::Deps,
        ViewId::Ambient,
        ViewId::Search,
        ViewId::Artefact,
    ];

    /// The registry id, as the config and the CLI spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ViewId::Queue => "queue",
            ViewId::Timeline => "timeline",
            ViewId::Diff => "diff",
            ViewId::Structure => "structure",
            ViewId::Graph => "graph",
            ViewId::Deps => "deps",
            ViewId::Ambient => "ambient",
            ViewId::Search => "search",
            ViewId::Artefact => "artefact",
        }
    }

    /// The global key that opens it (ADR-006 §7.2 K3: one letter each, plus `/` and `o`).
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            ViewId::Queue => "q",
            ViewId::Timeline => "t",
            ViewId::Diff => "d",
            ViewId::Structure => "s",
            ViewId::Graph => "g",
            ViewId::Deps => "p",
            ViewId::Ambient => "a",
            ViewId::Search => "/",
            ViewId::Artefact => "o",
        }
    }
}

/// Is this the primary pane, a secondary, or the search field's query row?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The pane spare width goes to first.
    Primary,
    /// The follow-up pane: the first to be squeezed, then dropped.
    Secondary,
    /// A one-row text field: never a focus stop, never resized horizontally.
    Query,
}

/// What kind of surface a pane is, which is what chooses its interaction mode (ADR-006 §1.1:
/// the pane type *is* the mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// A selectable list — `list` mode.
    List,
    /// A scrollable document — `body` mode.
    Body,
    /// The graph canvas — `body` mode.
    Canvas,
    /// The treemap, which is a list of rows and a picture — `list` mode.
    Treemap,
    /// The dependency table — `list` mode.
    Table,
    /// The search field.
    Field,
}

impl PaneKind {
    /// The interaction mode this pane focuses into (ADR-006 §1.1).
    #[must_use]
    pub const fn interaction(self) -> crate::keymap::Mode {
        match self {
            PaneKind::List | PaneKind::Treemap | PaneKind::Table => crate::keymap::Mode::List,
            PaneKind::Field => crate::keymap::Mode::Prompt,
            PaneKind::Body | PaneKind::Canvas => crate::keymap::Mode::Body,
        }
    }
}

/// One pane of a view (ADR-006 §5's pane tables).
#[derive(Debug, Clone, Copy)]
pub struct Pane {
    /// The pane's name, as the layout and the status line print it.
    pub name: &'static str,
    /// Primary, secondary or the query row.
    pub role: Role,
    /// Position in the focus ring; pane 1 is where focus lands on entry.
    pub focus_order: u8,
    /// The surface kind, which chooses the interaction mode.
    pub kind: PaneKind,
    /// Width in the documented (ideal) layout.
    pub ideal_cols: u16,
    /// Narrower than this and the pane starts dropping its own columns.
    pub min_cols: u16,
    /// Width it keeps when `D5` squeezes it to a headline strip.
    pub strip_width: u16,
    /// Below this it cannot be drawn at all — the number behind a refusal.
    pub hard_min: u16,
    /// Body rows this pane may drop under `D8`; zero means "every line is load-bearing".
    pub trim_rows: u16,
    /// Does it have a footer strip for `D3` to sell?
    pub strip: bool,
    /// The ticket that owns this pane's content.
    pub ticket: &'static str,
    /// The `EpisodeDelta` fields it renders — the citation rule (ADR-001 §7.10).
    pub reads: &'static [&'static str],
}

impl Pane {
    /// The interaction mode this pane takes focus into.
    #[must_use]
    pub const fn interaction(&self) -> crate::keymap::Mode {
        self.kind.interaction()
    }
}

/// One of the nine views (ADR-006 §4, §5).
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// Registry id.
    pub id: &'static str,
    /// The global key that opens it.
    pub key: &'static str,
    /// Title, as the tab bar prints it.
    pub title: &'static str,
    /// `"view"` or `"overlay"` — an overlay borrows the frame and leaves the view behind it.
    pub kind: &'static str,
    /// The ticket that owns the view's content.
    pub owner: &'static str,
    /// The question the reviewer arrived with; printed in the help overlay.
    pub question: &'static str,
    /// The size ADR-006 §5 draws the layout at — always ≤ 80×24 (§3).
    pub ideal_cols: u16,
    /// Ideal height.
    pub ideal_rows: u16,
    /// Smallest frame that still shows every pane, chrome included.
    pub min_rows: u16,
    /// Fewest content rows before the view refuses; three for anything with a list, two for
    /// ambient, which is two lines by design (ADR-001 §4.1's `40×3` claim).
    pub min_content_rows: u16,
    /// Which chrome this view draws (ADR-006 §5's `Chrome` column).
    pub chrome: u8,
    /// Its panes, in declaration order.
    pub panes: &'static [Pane],
    /// The named drill edges, for the help overlay (ADR-006 §5's drill tables).
    pub drills: &'static [(&'static str, &'static str, &'static str)],
}

/// The nine views — ADR-006 §4's inventory table.
pub const VIEWS: &[View] = &[
    View {
        id: "queue",
        key: "q",
        title: "Queue",
        kind: "view",
        owner: "e4.5",
        question: "What is the next thing I have to look at, and why that one?",
        ideal_cols: 80,
        ideal_rows: 24,
        min_rows: 18,
        min_content_rows: 3,
        chrome: CH_FRAME | CH_TABS | CH_STRIPS | CH_STATUS | CH_HINT,
        panes: &[
            Pane {
                name: "worklist",
                role: Role::Primary,
                focus_order: 1,
                kind: PaneKind::List,
                ideal_cols: 48,
                min_cols: 36,
                strip_width: 18,
                hard_min: 32,
                trim_rows: 0,
                strip: true,
                ticket: "e4.5",
                reads: &[
                    "episode.identity",
                    "episode.created_at",
                    "episode.attribution.kind",
                    "risk.score",
                    "risk.reasons[]",
                    "blast_radius.direct_count",
                    "review.state",
                    "noise.share",
                ],
            },
            Pane {
                name: "card",
                role: Role::Secondary,
                focus_order: 2,
                kind: PaneKind::Body,
                ideal_cols: 29,
                min_cols: 24,
                strip_width: 16,
                hard_min: 20,
                trim_rows: 0,
                strip: true,
                ticket: "e2.10",
                reads: &[
                    "delta.structure.*",
                    "delta.dependencies.*",
                    "delta.non_code.*",
                    "delta.tests.*",
                    "blast_radius.*",
                    "findings[].severity",
                    "intent.*",
                    "review.state",
                ],
            },
        ],
        drills: &[
            (
                "worklist · <Enter>",
                "card",
                "drill in: the episode card, pane 2",
            ),
            (
                "worklist · x f - z",
                "ledger",
                "disposition; the worklist advances",
            ),
            (
                "anywhere · d s g p",
                "that view",
                "same scope, different question",
            ),
            (
                "card · <Esc>",
                "worklist",
                "pop back; the selection is preserved",
            ),
        ],
    },
    View {
        id: "timeline",
        key: "t",
        title: "Timeline",
        kind: "view",
        owner: "e1.6",
        question: "What happened, in what order, and which of it have I not read?",
        ideal_cols: 80,
        ideal_rows: 24,
        min_rows: 16,
        min_content_rows: 3,
        chrome: CH_FRAME | CH_TABS | CH_STRIPS | CH_STATUS | CH_HINT,
        panes: &[
            Pane {
                name: "rail",
                role: Role::Primary,
                focus_order: 1,
                kind: PaneKind::List,
                ideal_cols: 42,
                min_cols: 32,
                strip_width: 18,
                hard_min: 28,
                trim_rows: 0,
                strip: true,
                ticket: "e1.6",
                reads: &[
                    "episode.identity",
                    "episode.created_at",
                    "episode.attribution.kind",
                    "delta.structure.*",
                    "review.state",
                    "episode.members[].sha",
                ],
            },
            Pane {
                name: "card",
                role: Role::Secondary,
                focus_order: 2,
                kind: PaneKind::Body,
                ideal_cols: 35,
                min_cols: 26,
                strip_width: 16,
                hard_min: 22,
                trim_rows: 0,
                strip: true,
                ticket: "e2.10",
                reads: &[
                    "delta.structure.*",
                    "delta.dependencies.*",
                    "delta.architecture.*",
                    "findings[].{detector_id,severity}",
                    "risk.*",
                    "blast_radius.*",
                ],
            },
        ],
        drills: &[
            (
                "rail · <Enter>",
                "card",
                "drill in: card for the selected episode",
            ),
            (
                "card · <Enter>",
                "Diff",
                "a second drill opens the diff at that episode",
            ),
            (
                "<Esc>",
                "rail / previous view",
                "pop one level, then pop out",
            ),
        ],
    },
    View {
        id: "diff",
        key: "d",
        title: "Diff",
        kind: "view",
        owner: "e1.7",
        question: "What exactly changed in this file, and is it what the card claims?",
        ideal_cols: 80,
        ideal_rows: 24,
        min_rows: 15,
        min_content_rows: 3,
        chrome: CH_FRAME | CH_TABS | CH_STRIPS | CH_STATUS | CH_HINT,
        panes: &[
            Pane {
                name: "tree",
                role: Role::Primary,
                focus_order: 1,
                kind: PaneKind::List,
                ideal_cols: 28,
                min_cols: 20,
                strip_width: 16,
                hard_min: 18,
                trim_rows: 0,
                strip: true,
                ticket: "e1.7",
                reads: &[
                    "delta.structure.* (file list)",
                    "path taxonomy class",
                    "noise class",
                    "per-file +/- counts",
                    "review state",
                ],
            },
            Pane {
                name: "hunks",
                role: Role::Secondary,
                focus_order: 2,
                kind: PaneKind::Body,
                ideal_cols: 49,
                min_cols: 30,
                strip_width: 24,
                hard_min: 26,
                trim_rows: 0,
                strip: true,
                ticket: "e1.7",
                reads: &[
                    "hunk text",
                    "evidence ids",
                    "word-diff",
                    "whitespace policy",
                ],
            },
        ],
        drills: &[
            (
                "tree · <Enter>",
                "hunks",
                "read that file's hunks (focus moves)",
            ),
            ("hunks · [ ]", "hunks", "previous / next hunk"),
            (
                "hunks · E",
                "$EDITOR",
                "hand off at file:line (ADR-001 §8 non-goal 2)",
            ),
            (
                "tree · b",
                "noise view",
                "the suppressed files, with the rule printed",
            ),
            (
                "<Esc>",
                "tree / previous view",
                "one level up, then pop out",
            ),
        ],
    },
    View {
        id: "structure",
        key: "s",
        title: "Structure",
        kind: "view",
        owner: "e2.3",
        question: "Where did it land, and is the tree hottest where it already was?",
        ideal_cols: 80,
        ideal_rows: 24,
        min_rows: 15,
        min_content_rows: 3,
        chrome: CH_FRAME | CH_TABS | CH_STRIPS | CH_STATUS | CH_HINT,
        panes: &[
            Pane {
                name: "treemap",
                role: Role::Primary,
                focus_order: 1,
                kind: PaneKind::Treemap,
                ideal_cols: 52,
                min_cols: 34,
                strip_width: 20,
                hard_min: 30,
                trim_rows: 0,
                strip: true,
                ticket: "e2.3",
                reads: &[
                    "delta.structure.*",
                    "churn heat per subtree",
                    "per-subtree +/-",
                    "noise.share",
                ],
            },
            Pane {
                name: "detail",
                role: Role::Secondary,
                focus_order: 2,
                kind: PaneKind::Body,
                ideal_cols: 25,
                min_cols: 20,
                strip_width: 16,
                hard_min: 18,
                trim_rows: 0,
                strip: true,
                ticket: "e2.3",
                reads: &[
                    "delta.structure.* (subtree rows)",
                    "renames with confidence",
                    "path taxonomy class",
                    "hotspot rank",
                ],
            },
        ],
        drills: &[
            (
                "treemap · <Enter>",
                "deeper",
                "drill one directory level (focus stays)",
            ),
            (
                "[ / ]",
                "level",
                "aggregate up / down — this view's shrink mechanism",
            ),
            ("<", "sort", "cycle churn · size · delta"),
            (
                "<Esc>",
                "level up",
                "one level out, then pop to the previous view",
            ),
        ],
    },
    View {
        id: "graph",
        key: "g",
        title: "Graph",
        kind: "view",
        owner: "e3.4",
        question: "Did this turn make the shape of the system worse?",
        ideal_cols: 80,
        ideal_rows: 24,
        min_rows: 16,
        min_content_rows: 3,
        chrome: CH_FRAME | CH_TABS | CH_STRIPS | CH_STATUS | CH_HINT,
        panes: &[
            Pane {
                name: "canvas",
                role: Role::Primary,
                focus_order: 1,
                kind: PaneKind::Canvas,
                ideal_cols: 54,
                min_cols: 36,
                strip_width: 20,
                hard_min: 30,
                trim_rows: 0,
                strip: true,
                ticket: "e3.4",
                reads: &[
                    "delta.architecture.*",
                    "module graph nodes/edges",
                    "cycle set",
                    "layering violation delta",
                    "blast_radius.*",
                ],
            },
            Pane {
                name: "detail",
                role: Role::Secondary,
                focus_order: 2,
                kind: PaneKind::Body,
                ideal_cols: 23,
                min_cols: 20,
                strip_width: 16,
                hard_min: 18,
                trim_rows: 0,
                strip: true,
                ticket: "e3.4",
                reads: &[
                    "node/edge evidence ids",
                    "delta.architecture.*",
                    "cycle debt",
                    "layering rules",
                    "public API delta",
                ],
            },
        ],
        drills: &[
            (
                "canvas · <Enter> node",
                "detail",
                "focus the detail pane on that node",
            ),
            (
                "canvas · <Enter> edge",
                "Diff",
                "the file-and-line that produced the edge",
            ),
            (
                "[ / ]",
                "aggregation",
                "module level up/down (3 000 nodes → legible)",
            ),
            ("< / >", "hops", "blast-radius hop limit"),
            ("!", "overlay", "show only this episode's delta overlay"),
            ("<Esc>", "up", "one level out of the selection"),
        ],
    },
    View {
        id: "deps",
        key: "p",
        title: "Deps",
        kind: "view",
        owner: "e2.6",
        question: "What does the code now depend on that it did not?",
        ideal_cols: 80,
        ideal_rows: 24,
        min_rows: 15,
        min_content_rows: 3,
        chrome: CH_FRAME | CH_TABS | CH_STRIPS | CH_STATUS | CH_HINT,
        panes: &[
            Pane {
                name: "manifest",
                role: Role::Primary,
                focus_order: 1,
                kind: PaneKind::Table,
                ideal_cols: 40,
                min_cols: 32,
                strip_width: 18,
                hard_min: 28,
                trim_rows: 0,
                strip: true,
                ticket: "e2.6",
                reads: &[
                    "delta.dependencies.*",
                    "licence set delta",
                    "advisory hits (opt-in)",
                    "manifest/lockfile evidence",
                ],
            },
            Pane {
                name: "tree",
                role: Role::Secondary,
                focus_order: 2,
                kind: PaneKind::List,
                ideal_cols: 37,
                min_cols: 26,
                strip_width: 16,
                hard_min: 22,
                trim_rows: 0,
                strip: true,
                ticket: "e2.6",
                reads: &[
                    "resolved dependency graph",
                    "reverse dependencies",
                    "lockfile line ids",
                ],
            },
        ],
        drills: &[
            ("manifest · <Enter>", "tree", "why is this dependency here"),
            ("[ / ]", "ecosystem", "next / previous ecosystem in scope"),
            ("u", "filter", "show transitive-only changes only"),
            ("<Esc>", "manifest", "one level up"),
        ],
    },
    View {
        id: "ambient",
        key: "a",
        title: "Ambient",
        kind: "view",
        owner: "e5.1",
        question: "Anything I need to look at right now? (glance, do not study)",
        ideal_cols: 80,
        ideal_rows: 12,
        min_rows: 10,
        min_content_rows: 2,
        chrome: CH_STATUS,
        panes: &[Pane {
            name: "glance",
            role: Role::Primary,
            focus_order: 1,
            kind: PaneKind::Body,
            ideal_cols: 80,
            min_cols: 40,
            strip_width: 40,
            hard_min: 40,
            trim_rows: 5,
            strip: false,
            ticket: "e5.1",
            reads: &[
                "episode.identity",
                "risk.score",
                "review.state",
                "queue counts",
                "index epoch (ADR-004 §6.3)",
                "caught-up state",
            ],
        }],
        drills: &[
            ("<Enter>", "card", "the one keypress ADR-001 §4.1 promises"),
            ("q / t", "view", "leave glance mode"),
        ],
    },
    View {
        id: "search",
        key: "/",
        title: "Search",
        kind: "overlay",
        owner: "e5.2",
        question: "Where is the thing I am thinking of?",
        ideal_cols: 72,
        ideal_rows: 14,
        min_rows: 12,
        min_content_rows: 3,
        chrome: CH_STATUS,
        panes: &[
            Pane {
                name: "query",
                role: Role::Query,
                focus_order: 1,
                kind: PaneKind::Field,
                ideal_cols: 72,
                min_cols: 48,
                strip_width: 0,
                hard_min: 20,
                trim_rows: 0,
                strip: false,
                ticket: "e5.2",
                reads: &["(input) free text plus filter tokens author: path: risk> dep:"],
            },
            Pane {
                name: "results",
                role: Role::Primary,
                focus_order: 2,
                kind: PaneKind::List,
                ideal_cols: 46,
                min_cols: 30,
                strip_width: 18,
                hard_min: 26,
                trim_rows: 0,
                strip: true,
                ticket: "e5.2",
                reads: &[
                    "episode summaries",
                    "paths",
                    "symbol names",
                    "dependency names",
                    "finding types",
                ],
            },
            Pane {
                name: "preview",
                role: Role::Secondary,
                focus_order: 3,
                kind: PaneKind::Body,
                ideal_cols: 23,
                min_cols: 20,
                strip_width: 16,
                hard_min: 18,
                trim_rows: 0,
                strip: true,
                ticket: "e5.2",
                reads: &[
                    "episode card excerpt",
                    "match explanation",
                    "evidence locator",
                ],
            },
        ],
        drills: &[
            (
                "<C-n> / <C-p>",
                "results",
                "walk results without leaving the field",
            ),
            (
                "<Enter>",
                "target view",
                "deep link: right view, right selection",
            ),
            ("<Esc>", "dismiss", "the view behind keeps its selection"),
        ],
    },
    View {
        id: "artefact",
        key: "o",
        title: "Artefact",
        kind: "view",
        owner: "e5.7",
        question: "What would my review record say, before I make it permanent?",
        ideal_cols: 72,
        ideal_rows: 16,
        min_rows: 14,
        min_content_rows: 3,
        chrome: CH_FRAME | CH_TABS | CH_STRIPS | CH_STATUS | CH_HINT,
        panes: &[
            Pane {
                name: "preview",
                role: Role::Primary,
                focus_order: 1,
                kind: PaneKind::Body,
                ideal_cols: 49,
                min_cols: 34,
                strip_width: 20,
                hard_min: 28,
                trim_rows: 0,
                strip: true,
                ticket: "e5.7",
                reads: &[
                    "review ledger entries",
                    "episode cards",
                    "findings with evidence",
                    "operator-action checklist",
                    "sign-off block",
                ],
            },
            Pane {
                name: "scope",
                role: Role::Secondary,
                focus_order: 2,
                kind: PaneKind::List,
                ideal_cols: 20,
                min_cols: 20,
                strip_width: 16,
                hard_min: 18,
                trim_rows: 0,
                strip: true,
                ticket: "e5.7",
                reads: &["episode range", "dispositions", "export target", "format"],
            },
        ],
        drills: &[
            (
                "<Enter>",
                "write",
                "write the file; the path is printed, not guessed",
            ),
            ("[ / ]", "range", "narrow / widen the episode range"),
            ("=", "format", "cycle md / html / json"),
            ("<Esc>", "up", "leave the preview"),
        ],
    },
];

impl View {
    /// Look up a view by id.
    #[must_use]
    pub fn by_id(id: ViewId) -> &'static View {
        VIEWS
            .iter()
            .find(|v| v.id == id.name())
            .unwrap_or(&VIEWS[0])
    }

    /// The primary pane.
    #[must_use]
    pub fn primary(&self) -> &'static Pane {
        self.panes
            .iter()
            .find(|p| p.role == Role::Primary)
            .unwrap_or(&self.panes[0])
    }

    /// The secondary panes — the ones the ladder squeezes and then drops.
    #[must_use]
    pub fn secondaries(&self) -> Vec<&'static Pane> {
        self.panes
            .iter()
            .filter(|p| p.role == Role::Secondary)
            .collect()
    }

    /// The query row, if this view has one (`Search` does).
    #[must_use]
    pub fn query(&self) -> Option<&'static Pane> {
        self.panes.iter().find(|p| p.role == Role::Query)
    }

    /// The focus ring: names in `focus_order`, query rows excluded (ADR-006 §1.1's
    /// `<Tab>`/`<C-w>l` walk).
    #[must_use]
    pub fn focus_ring(&self) -> Vec<&'static str> {
        let mut ring: Vec<(u8, &str)> = self
            .panes
            .iter()
            .filter(|p| p.role != Role::Query)
            .map(|p| (p.focus_order, p.name))
            .collect();
        ring.sort_by_key(|(order, _)| *order);
        ring.into_iter().map(|(_order, name)| name).collect()
    }

    /// Does this view draw a piece of chrome?
    #[must_use]
    pub const fn has_chrome(&self, bit: u8) -> bool {
        self.chrome & bit != 0
    }

    /// The interaction mode focus lands in when the view is entered: pane 1's mode.
    #[must_use]
    pub fn entry_interaction(&self) -> crate::keymap::Mode {
        self.primary().interaction()
    }

    /// The pane with a given name, if this view has it.
    #[must_use]
    pub fn pane(&self, name: &str) -> Option<&'static Pane> {
        self.panes.iter().find(|p| p.name == name)
    }
}

/// Look up a view by id.
#[must_use]
pub fn view(id: ViewId) -> &'static View {
    View::by_id(id)
}

/// The view a global key opens (ADR-006 §7.2 K3), or `None` when the key is not a view key.
#[must_use]
pub fn view_for_key(key: &str) -> Option<ViewId> {
    ViewId::ALL.iter().copied().find(|v| view(*v).key == key)
}
