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
/// The review memory: the annotations, decisions, questions and follow-ups a review produces, as committed
/// markdown under `.fathom/reviews/notes.md`, re-linked by the same ladder the ledger rides and mirrored
/// into `decision_note` (`e6.5`, `reco-552`).
pub mod notes;
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

/// The public API surface (`e3.7`, `reco-p5u`): what each package lets the outside world name, in each
/// language's own access rules; the diff of two revisions classified breaking / additive / deprecating
/// (with the indeterminate class a scope that cannot be enumerated forces); the semver recommendation
/// per affected package with the one change that drove it; and the episode stamp on every change. Reads
/// [`structure`]; writes nothing.
pub mod api;

/// The module graph (`e3.2`, `reco-8s4`): modules and exported symbols as nodes, imports/uses/
/// implementations/inheritance as edges, layer tags from `[layers]`, stored as one delta per revision
/// with a shared anchor every `graph.chain_full_every`, and queried by dependency, dependents, path,
/// layer subgraph and changed edges. The store half is SQLite-gated, like [`index`].
pub mod graph;

/// The cycles (`e3.5`, `reco-8md`): strongly-connected components *maintained* across episodes rather
/// than recomputed per one, the shortest offender path for each (module level, with an opt-in
/// symbol-level view), the debt ledger that keeps a baselined cycle from ever reading as a new violation,
/// and the two numbers the scorecard trends — cycles and nodes inside cycles. Reads the graph of
/// [`graph`]; writes nothing.
pub mod cycles;

/// The layering rules (`e3.6`, `reco-cfp`): the intended architecture declared in `fathom.toml` —
/// allowed and forbidden dependencies, no upward references, no cycles, no cycles inside one layer, and
/// boundary rules — validated at load, evaluated at the granularity the extractor guarantees, and
/// reported as a per-episode delta against the committed baseline so a reviewer sees what this turn did
/// rather than what the repository already owed. Reads the graph of [`graph`] and the cycles of
/// [`cycles`]; writes nothing.
pub mod layering;

/// The architecture scorecard (`e3.9`, `reco-al1`): five dimensions — boundaries, coupling, acyclicity,
/// API hygiene, hotspot pressure — combined by a documented, hand-reproducible formula into one 0–100
/// health score per revision, trended across the revisions the index holds with bounded memory, and
/// reported against the reviewer's last review by an identity ladder that survives a rebase. Reads
/// [`layering`], [`cycles`], [`graph`] and [`hotspots`]; writes only the class-R `scorecard` rows, and only
/// through [`scorecard::lane`].
pub mod scorecard;

/// The ratchet's write path (`e3.10`, `reco-5ht`): the plan that turns `.fathom/ratchet.toml` over — rows
/// leaving on their own when the violation they forgave is gone, rows entering only through a deliberate act
/// with an owner, an expiry and a written reason — and the one lane read that says what a revision currently
/// owes. Reads [`cycles`] and [`layering`]; the file itself is read and written by the composition root,
/// which is the only party in the workspace with a clock and a filesystem.
pub mod ratchet;

/// The hotspots (`e3.8`, `reco-wky`): which modules architectural attention pays off in, ranked by six
/// explainable metrics — fan-in, fan-out, instability, size, the `e3.1` complexity proxy and `e3.3`'s
/// churn velocity over a configurable window — with every metric's contribution printed, the churn ×
/// complexity quadrant and the god-module shape named, and a separate list of dead-code candidates that
/// states how long each has gone unreferenced and admits every way it could be wrong. Reads [`graph`] and
/// [`structure`]; writes nothing.
pub mod hotspots;

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
