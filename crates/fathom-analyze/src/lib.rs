//! `fathom-analyze` — every analysis, and the one façade (ADR-002 §3, §4, §5.3).
//!
//! **IO-free by choice, not by accident.** Analysis reads through `fathom-ingest` and `fathom-store`
//! and is a pure function of (input, config), which is what makes golden tests and ADR-004 §5's cache
//! keys mean something. Four teams own modules here (§9) and none of them shares a file with another.
//!
//! [`insight`] and [`export`] are the narrow waist: they are the only modules `fathom-tui` and
//! `fathom-mcp` may name (§5.3, lint `M02`), so "how many episodes does a page hold" has exactly one
//! answer in the whole product. Everything else in this crate is a promise the surfaces may not reach
//! past — which is the difference between ADR-001 §7.10 ("one reality") as a value and as a rule.
//!
//! The analyses are landing ticket by ticket: [`deps`] is `e2.5` (dependency manifests and lockfiles),
//! and what is still missing is named by the ticket that owns it — epic 2's other deltas, epic 3
//! (architecture), epic 4 (agents, risk, the queue) and `e5.7` (the artefact). What is here is the façade's contract: its schema version, its pagination
//! rule and the typed failure it returns before it has an answer.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

/// What this build was compiled with, as far as the cache is concerned (ADR-002 §6.4, §10.2). Gated with
/// the store's cache module, because the thing it derives is a cache version.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod build;
pub mod cancel;
/// The comparison of any two endpoints (`e5.3`, `reco-ifo`): the spec, the labelled endpoints, and the
/// composition of the per-episode delta documents between them. No second delta engine — see the module.
pub mod compare;
pub mod deps;
/// The agent pathology detectors (`e4.3`): pure functions over the episode-shaped delta model, one
/// catalogue, one suppression key per finding. Named `detectors` and not `findings` because that is the
/// spelling `fathom-store`'s cache id `findings.detect` and the config keys `detectors.*` already use.
pub mod detectors;
pub mod error;
pub mod export;
/// The named fuzz entry points (`e7.8`, `reco-41a`): one call per hostile-input family, shared by the
/// cargo-fuzz targets in `fuzz/` and the corpus/proptest tests, so the nightly run and the per-commit run
/// cannot drift apart.
pub mod hardening;
pub mod insight;

pub mod drift;
/// The review ledger: sign-offs as committed markdown under `.fathom/reviews/`, the four digests that let a
/// decision survive a rewrite, the staleness verdict, and the mirror index (`e4.6`, `reco-rlf`).
pub mod ledger;
/// The episode narrative (`e4.7`, `reco-2rx`): the deterministic, cited prose a human skims, composed from
/// the delta document and nothing else. Its own self-check is that no claim arrives without evidence.
pub mod narrative;
/// The attention queue (`e4.5`, `reco-wsj`): the ordering, the four triage acts and the retrospective.
/// Its types reach the surfaces through [`insight`], which is the only door `fathom-tui` may open.
pub mod queue;
/// The spine-only risk ranking (`reco-c05`), and the ordering `e4.4`'s scorer replaces.
pub mod risk;
/// The declared scope of an episode, measured against the paths the change touched (`e4.2`). The
/// scope-creep detector is `e4.3`'s; this is the input it reads.
pub mod scope;
/// The one search box (`e5.2`, `reco-mkc`): the corpus of every kind of thing a reviewer searches for, the
/// deterministic scorer, and the filter grammar. The store owns the document; this owns the ranking.
pub mod search;
/// The per-file structural model (`e3.1`, `reco-zoz`): definitions, exports, imports and their
/// resolved targets, implemented traits and inherited bases, the scopes that cannot be enumerated and
/// the wildcard edges, for the three grammars the `e0.12` checkpoint put first. Always compiled; the
/// grammars themselves are behind ADR-002 §6.5's `lang-*` features, and a file whose grammar is
/// absent comes back saying so rather than coming back missing.
pub mod structure;

/// The module graph (`e3.2`, `reco-8s4`): modules and exported symbols as nodes, imports/uses/
/// implementations/inheritance as edges, layer tags from `[layers]`, stored as one delta per revision
/// with a shared anchor every `graph.chain_full_every`, and queried by dependency, dependents, path,
/// layer subgraph and changed edges. The store half is SQLite-gated, like [`index`].
pub mod graph;

/// The episode step of an index run: commits in, reviewable episodes out (`e1.3`).
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod episodes;

/// The incremental index: what the cache is *for_ (ADR-004 §6). Present only when a SQLite backend is
/// compiled in, because an index with nowhere to store its rows is not an index.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod index;

/// The meeting point of `fathom-ingest`'s records and `fathom-store`'s tables (ADR-002 §3).
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod sink;
