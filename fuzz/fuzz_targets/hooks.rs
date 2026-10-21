//! Fuzz target: the hook-event reader (`e7.8`, `reco-41a`).
//!
//! Seed format: the raw bytes of one or more lines of `.fathom/events.jsonl` — see
//! [`fuzz/README.md`](../README.md). The driver is [`fathom_ingest::hardening::hook_event`], the same
//! function `crates/fathom-ingest/tests/hooks_hardening.rs` runs the corpus and proptest seeds through.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    fathom_ingest::hardening::hook_event(data);
});
