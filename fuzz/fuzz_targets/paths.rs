//! Fuzz target: path *names* — the readers that never touch the disk (`e7.8`, `reco-41a`).
//!
//! Seed format: `path\0path\0…` — see [`fuzz/README.md`](../README.md). One seed tries several names,
//! because the interesting failures are about a name's shape and a list of them per seed costs the fuzzer
//! nothing. The driver is [`fathom_analyze::hardening::paths`], shared with
//! `crates/fathom-analyze/tests/hardening.rs`.
//!
//! This target is the sibling of the containment rule in `crates/fathom-cli/src/sandbox.rs`: that file
//! decides at write time whether a resolved path is inside the repository, and this one makes sure the
//! lexical readers agree about names that *look* like an escape — traversal, absolute paths, NUL and
//! control bytes, home prefixes, and backslashes on a platform that treats them as ordinary characters.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let fields: Vec<&str> = text.split('\0').collect();
    fathom_analyze::hardening::paths(&fields);
});
