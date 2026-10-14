//! `fathom-mcp` — the door the coding agent looks through (ADR-002 §3, §4.3, §6.3; ADR-005 §4).
//!
//! **stdio is the default transport, and it is not a network feature.** It opens no socket, so the
//! privacy posture (ADR-001 §7.5) is not about which transport is in use; the capability gate for
//! anything a server may *write* lives in the JSON-RPC handler (ADR-005 §4.3), where choosing a
//! different transport cannot bypass it. `net-http-transport` is the feature that adds a listener, and
//! a listener is exactly as much a privacy decision as an outbound call, so it wears the same prefix and
//! needs the same consent key.
//!
//! One binary, one build: `fathom mcp serve` is a verb in the same process as the TUI (§4.3), so the
//! agent cannot end up reviewing against different bytes than the human. The tool surface, capability
//! gating and the shared review memory are `e6.4`/`e6.5`'s; what exists at the scaffold is the runtime
//! consent gate the privacy claim depends on, and the refusal that stands in for the server.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod error;
pub mod transport;
