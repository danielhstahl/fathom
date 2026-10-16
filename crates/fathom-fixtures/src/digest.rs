//! `digest.rs` — the one way this crate hashes anything.
//!
//! A fixture needs SHA-256 in three places: the content digests in the harness event log (ADR-005 §2.1
//! names `content_sha256_before` and friends), and the object/work-tree/event digests that make
//! "byte-identical on all three operating systems" a checked statement instead of a hope.
//!
//! It does not add `sha2` to this crate's dependency list to get it. `fathom-ingest` already publishes the
//! digest — `record::sha256_hex` is what computes `git_commit.message_hash` — and `fathom-fixtures` depends
//! on `fathom-ingest` already (ADR-002 §4). Reusing it costs nothing, keeps the corpus's published
//! dependency set exactly what the ADR publishes, and has the side benefit that the digest a fixture writes
//! and the digest the ingest writes are literally the same function, so the two can never disagree about
//! what a blob's hash is.

/// Lowercase hex SHA-256, from `fathom-ingest`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    fathom_ingest::record::sha256_hex(bytes)
}
