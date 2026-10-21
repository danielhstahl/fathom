//! Fuzz target: the lockfile and manifest readers (`e7.8`, `reco-41a`).
//!
//! Seed format: `<file-name>\0<content>` — see [`fuzz/README.md`](../README.md). The driver is
//! [`fathom_analyze::hardening::lockfile`], the same function `crates/fathom-analyze/tests/hardening.rs`
//! runs the corpus and proptest seeds through.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    fathom_analyze::hardening::lockfile(data);
});
