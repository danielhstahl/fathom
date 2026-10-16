//! `fathom-tui` — the terminal application (ADR-002 §3, §5.2, §5.3; ADR-006 owns its information
//! architecture).
//!
//! A library, not a binary: `fathom-cli` owns the process, the panic hook and the exit code, so that
//! losing the screen cannot also lose the reason (ADR-002 §7.2).
//!
//! Two rules make this crate's shape, both of them boundary rather than style:
//!
//! 1. **No store handle, no self-computed score.** The TUI reads `fathom_analyze::insight` and nothing
//!    else, because a view that runs its own query is how the human and the MCP server end up with two
//!    different ideas of the same list — ADR-001 §7.10, which is a bug with a severity, not a smell.
//!    Lint `M02` enforces the path, and `E01` refuses the `fathom-store` edge the layering would allow.
//! 2. **No terminal-size or clock reads outside the frame loop.** Rendering takes a [`FrameContext`]
//!    carrying the size and the instant, so a view is testable headlessly at a fixed 80×24 (`e5.5`)
//!    and a golden render does not depend on the window it was captured in.
//!
//! The app, the keymap and the views are `e1.5`–`e1.7` and epic 5's. At the scaffold the crate
//! contributes the frame contract and the refusal it returns before there is anything to draw.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod app;
pub mod diff;
pub mod error;
pub mod event;
pub mod frame;
pub mod keymap;
pub mod layout;
pub mod render;
pub mod style;
pub mod terminal;
pub mod timeline;
pub mod views;
