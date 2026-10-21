//! `fathom-tui` — the terminal application (ADR-002 §3, §5.2, §5.3; ADR-006 owns its information
//! architecture).
//!
//! A library, not a binary: `fathom-cli` owns the process, the panic hook and the exit code, so that
//! losing the screen cannot also lose the reason (ADR-002 §7.2).
//!
//! Two rules make this crate's shape, both of them boundary rather than style:
//!
//! 1. **No store handle, no self-computed score.** The TUI reads `fathom_analyze::insight` — and, since
//!    `e5.2`'s search box, `fathom_analyze::search` for a corpus the composition root already read — and
//!    nothing else, because a view that runs its own query is how the human and the MCP server end up with
//!    two different ideas of the same list — ADR-001 §7.10, which is a bug with a severity, not a smell.
//!    Lint `M02` enforces the path, and `E01` refuses the `fathom-store` edge the layering would allow.
//!    What the search exception may not become is a licence: the shell may *score* a corpus it was handed
//!    and may not build one, and `M03` is the lint that keeps the difference real.
//!    The risk numbers the rail and the queue print arrived the same way `reco-c05` found everything
//!    else: `fathom-cli` reads the spine, asks `fathom_analyze::risk` for the ranking, and hands the
//!    shell the value and its reasons in the row. Nothing here sorts or scores an episode from its own
//!    reading of the fields — the queue's order is a sort of a number it was given.
//! 2. **No terminal-size or clock reads outside the frame loop.** Rendering takes a [`FrameContext`]
//!    carrying the size and the instant, so a view is testable headlessly at a fixed 80×24 (`e5.5`)
//!    and a golden render does not depend on the window it was captured in.
//!
//! The app, the keymap and the views are `e1.5`–`e1.7` and epic 5's. At the scaffold the crate
//! contributes the frame contract and the refusal it returns before there is anything to draw.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod ambient;
pub mod app;
pub mod colour;
/// Compare mode's shell side (`e5.3`, `reco-ifo`): the provider that answers a spec, and the panel `c`
/// draws the answer into. The composition itself is `fathom_analyze::compare`'s.
pub mod compare;
pub mod diff;
/// Every pane's empty state, as one batch of copy (`e5.9`, ADR-006 §10.3).
pub mod empty;
pub mod error;
pub mod event;
pub mod frame;
pub mod index;
pub mod keymap;
pub mod layout;
pub mod materialize;
/// The progress row: a percentage and a time estimate that converges (`e5.9`, ADR-006 §10.3).
pub mod progress;
pub mod queue;
pub mod render;
/// The `/` box: the corpus it scores against, how its rows read, and where `<Enter>` goes (`e5.2`,
/// `reco-mkc`). The scoring itself belongs to `fathom_analyze::search`; this module is the overlay.
pub mod search;
pub mod session;
pub mod style;
pub mod terminal;
pub mod text;
pub mod theme;
pub mod timeline;
pub mod views;
/// How an open frame learns the repository moved (`reco-bpc`): the watcher the composition root installs,
/// the update it hands over, and the two states the frame is allowed to claim about its own freshness.
pub mod watch;
/// The Queue view's ranked worklist (`reco-c05`): a list, not a scorer.
pub mod worklist;
