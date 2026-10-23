//! `fathom-mcp` — the door the coding agent looks through (ADR-002 §3, §4.3, §6.3; ADR-005 §4).
//!
//! **stdio is the default transport, and it is not a network feature.** It opens no socket, so the privacy
//! posture (ADR-001 §7.5) is not about which transport is in use; the capability gate for anything a server
//! may *write* lives in the JSON-RPC handler ([`server`], ADR-005 §4.3), where choosing a different
//! transport cannot bypass it. `net-http-transport` is the feature that adds a listener, and a listener is
//! exactly as much a privacy decision as an outbound call, so it wears the same prefix and needs the same
//! consent key ([`transport::check_consent`]).
//!
//! One binary, one build: `fathom mcp serve` is a verb in the same process as the TUI (§4.3), so the agent
//! cannot end up reviewing against different bytes than the human.
//!
//! # How a request travels
//!
//! ```text
//!   agent ──stdio──▶ rmcp ──▶ server::FathomMcp ──┬──▶ tools      is the tool here, and may this session see it
//!                                                 ├──▶ policy     writes off by default; a write needs a
//!                                                 │               token bound to the change
//!                                                 ├──▶ source     the composition root's read of the insight
//!                                                 │               façade — this crate holds no store handle
//!                                                 ├──▶ egress     secrets redacted, file bodies refused,
//!                                                 │               every leaf bounded
//!                                                 ├──▶ envelope   cost, bytes, degraded, unknowns, cursor
//!                                                 └──▶ audit      one line per gated call
//! ```
//!
//! Every arrow above is an acceptance criterion of `e6.4` made into a function, and the order is the point:
//! the gates run before the read, the filter runs after it, and nothing reaches the wire that has not been
//! through all of them.
//!
//! # What this crate deliberately is not
//!
//! It is not an analyst. [`source::Source`] is the entire interface to the repository, and it is a trait so
//! that this crate — which may not name `fathom-store` or `fathom-ingest`, and may not do IO at all
//! (ADR-002 §5, lint `W1`) — can hand the finished `EpisodeDelta` documents across rather than recompose
//! them. The agent and the human get one model because there is one analysis behind both doors, and lint
//! `M02`/`M03` is what keeps that sentence true.
//!
//! It is not a listener either, in a default build. See [`transport::serve`] for why the consent gate for
//! `net-http-transport` answers before any listener could exist.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod egress;
pub mod envelope;
pub mod error;
pub mod policy;
pub mod server;
pub mod source;
pub mod tools;
pub mod transport;
