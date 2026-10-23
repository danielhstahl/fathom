//! `cache.rs` — the analyzer manifest, the cache key, and the one place hits and misses exist
//! (ADR-004 §5, §5.6, §6).
//!
//! The rule this module exists to make unavoidable is ADR-004's, quoted once rather than paraphrased:
//!
//! > A cached analysis result is addressed by *everything it actually depended on*. Therefore
//! > "invalidate X" is not an operation anyone performs — it is what happens automatically when the
//! > derived key for an input stops matching the key stored under it. There is no invalidation code
//! > path, and so no invalidation code path to forget.
//!
//! So there is no `invalidate` function here. There is [`derive_key`], whose nine framed fields say what
//! a result depended on; [`resolve`], which is the only place a hit or a miss is decided and counted
//! (§5.6); and [`put`], which stores an answer — including a *failed* answer, because "this analyzer
//! fails for this input" is a fact worth keeping (§4.5). Everything else in the file is the manifest the
//! key is built from.
//!
//! # The manifest is data, and there are two copies of it on purpose
//!
//! [`ANALYZERS`] is the same v1 manifest ADR-004 §5.1 generates its invalidation matrix from, and the
//! same list the reference implementation keeps in `src/recommender/store/cache.py`. Two copies of one
//! table is normally a defect; here it is the point. The Python copy documents the contract, the Rust
//! copy runs it, and `tests/test_incremental_index_contract.py` fails the build if they ever disagree
//! about an id, a version, a scope kind, a config key or a dependency. The key digests themselves are
//! pinned in `tests/golden/adr004-cache-keys.json`, computed independently by both implementations.
//!
//! # What is deliberately not here
//!
//! * **Retention.** Nothing in this file deletes a cache row. The pruner (ADR-004 §8) is `e6.x`'s, and a
//!   dead row that is merely unreachable is worth keeping: it is what lets a rebuild say "the last time we
//!   computed this it cost 40 ms" (§6.2).
//! * **The writer lease** (§7.1). See [`crate::spine`]'s notes: today SQLite's own writer arbitration is
//!   the single-writer guarantee, and the lease exists to coordinate a run with a daemon that does not
//!   exist yet (`e6.3`).
//! * **Analyzers.** This module is the *addressing*, not the computation. The analyses that exist today
//!   are registered in `fathom-analyze` and reach the cache through [`resolve`] and [`put`] like any
//!   other consumer.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension as _, params};
use serde_json::Value;
use sha2::Digest as _;

use crate::error::Error;
use crate::spine::SCHEMA_VERSION;

/// The key-framing version (§5.2, field 1). A framing change makes every stored row unreachable instead
/// of misread, which is the only safe meaning of "version".
pub const KEY_SCHEMA: &str = "ck1";

/// Payloads up to this many bytes live in the `blob` row itself; larger ones go to `objects/` beside the
/// store (ADR-004 §3, §4.5). The number is `store.inline_blob_bytes` and it is a *storage* decision, so
/// a change is a schema note and not a tuning knob.
pub const INLINE_BLOB_BYTES: u64 = 4096;

/// The scope kinds a cache input may be addressed by (§5.3). The kind decides how the digest was
/// computed; the digest itself is what goes into the key.
pub const SCOPE_KINDS: [&str; 9] = [
    "repo",        // whole-repository fact: head set plus config
    "tree",        // a tree oid: structure, graph, scorecard
    "commit",      // one commit: normalization, attribution inputs
    "commit_pair", // diff between two commits: hunk and rename analysis
    "path",        // (path, blob) — the per-file unit
    "path_pair",   // (path, old blob, new blob): per-file diff analysis
    "episode",     // episode identity plus fingerprint (ADR-003)
    "blob",        // a single blob: taxonomy, generated-file detection
    "ref",         // a ref and its tip: lineage facts
];

/// One analyzer's entry in the manifest (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Analyzer {
    /// The stable name, used in cache rows, counters and `fathom index --stats`.
    pub id: &'static str,
    /// The bump a developer performs when the analyzer's code changes (§5.7).
    pub version: u32,
    /// The version of the *result shape* stored in `cache`/`delta`.
    pub schema_version: u32,
    /// One of [`SCOPE_KINDS`].
    pub scope_kind: &'static str,
    /// The `fathom.toml` paths it reads; a trailing `.` means the whole subtree (§5.5).
    pub config_keys: &'static [&'static str],
    /// The analyzers whose results it reads, and whose version bumps therefore cascade (§5.4).
    pub depends_on: &'static [&'static str],
    /// Globs over the files that implement it — the CI gate of §5.7 reads these.
    pub source_paths: &'static [&'static str],
    /// A rough cost, for progress reporting and for the §7.6 interrupt budget. Only a hint: the store
    /// measures what a run really cost in `cache.cost_ms`.
    pub est_cost_ms: u32,
}

impl Analyzer {
    /// The transitive closure of [`depends_on`](Self::depends_on), sorted, excluding self (§5.4).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the manifest names an analyzer that does not exist — a manifest that
    /// does not check out is a manifest that cannot answer "what changed", so it is an error and not a
    /// silent empty set.
    pub fn dependency_closure(&self) -> Result<Vec<&'static str>, Error> {
        dependency_closure(self.id)
    }
}

/// The v1 manifest (ADR-004 §5.1), named so that the invalidation matrix and the `e2.x`/`e3.x` tickets
/// cite the same strings.
///
/// Only three of these have an implementation in Rust today — `ingest.normalize` (`e1.2`),
/// `diff.per_file` and `structure.delta` (`e1.4`) — and the rest are declared so that a dependency
/// digest, a counter name or a CI glob is written correctly the first time the analyzer arrives. A
/// declared analyzer with no implementation is a name without a producer, which is exactly what §5.4's
/// digest needs in order to cascade on its behalf later.
pub const ANALYZERS: &[Analyzer] = &[
    Analyzer {
        id: "ingest.normalize",
        version: 1,
        schema_version: 1,
        scope_kind: "commit",
        config_keys: &["ingest.size_cap_mib"],
        depends_on: &[],
        source_paths: &["crates/fathom-ingest/src/normalize.rs"],
        est_cost_ms: 2,
    },
    Analyzer {
        // e0.9's engine reads four config surfaces, so the digest covers all four: §5.5's rule is that
        // the key names everything the payload reads. Version 1 declared only the noise half, which was
        // honest while the analyzer was a stub; a change to the test-surface globs or to a class
        // assertion must now move these rows, which is why the version is 2.
        id: "taxonomy.path",
        version: 2,
        schema_version: 1,
        scope_kind: "blob",
        config_keys: &[
            "noise.globs",
            "noise.builtins_version",
            "test.globs",
            "taxonomy.classes.",
        ],
        depends_on: &[],
        source_paths: &["crates/fathom-analyze/src/taxonomy/**"],
        est_cost_ms: 1,
    },
    Analyzer {
        id: "diff.per_file",
        version: 1,
        schema_version: 1,
        scope_kind: "path_pair",
        config_keys: &["diff.context_lines"],
        depends_on: &["taxonomy.path"],
        source_paths: &["crates/fathom-ingest/src/diff.rs"],
        est_cost_ms: 1,
    },
    Analyzer {
        id: "diff.rename_detect",
        // Version 2: `e2.4` (`reco-a0v`) made pairing a tiered engine whose answer depends on twelve
        // more config keys, so §5.5's invalidation has to cover all of them — a pairing cached under a
        // 0.50 fact bar must not survive the move to the s4-measured 0.75.
        version: 2,
        schema_version: 1,
        scope_kind: "commit_pair",
        config_keys: &[
            "rename.threshold",
            "rename.fact_bar",
            "rename.fact.allow_content",
            "rename.guess_floor",
            "rename.candidates.top_k",
            "rename.candidates.budget",
            "rename.candidates.copies_harder",
            "rename.copies",
            "rename.folds",
            "rename.content.shingle_lines",
            "rename.content.normalise",
            "rename.content.min_shared_shingles",
            "rename.card.rows",
        ],
        depends_on: &["diff.per_file"],
        source_paths: &["crates/fathom-analyze/src/rename/**"],
        est_cost_ms: 3,
    },
    Analyzer {
        id: "episode.segment",
        version: 1,
        schema_version: 1,
        scope_kind: "commit_pair",
        config_keys: &["episode.boundaries", "episode.time_gap_minutes"],
        depends_on: &["ingest.normalize"],
        source_paths: &["crates/fathom-ingest/src/segment/**"],
        est_cost_ms: 2,
    },
    Analyzer {
        // Version 2: the payload is a chain of deltas anchored every 32 trees, not a copy of the whole
        // inventory on every row (`reco-66q`, measured on three real repositories). The shape changed, so
        // every v1 row is unreachable — which is the correct invalidation, not a migration: §9.5 says a
        // derived row is recomputed rather than rewritten, and the recomputation is what the lane does.
        id: "structure.delta",
        version: 2,
        schema_version: 1,
        scope_kind: "tree",
        config_keys: &["layers.", "noise.globs"],
        depends_on: &["taxonomy.path", "diff.per_file"],
        source_paths: &[
            "crates/fathom-analyze/src/structure/**",
            "crates/fathom-analyze/src/index.rs",
        ],
        est_cost_ms: 4,
    },
    Analyzer {
        id: "deps.resolve",
        version: 1,
        schema_version: 1,
        scope_kind: "tree",
        config_keys: &["deps.ecosystems", "noise.globs"],
        depends_on: &["taxonomy.path"],
        source_paths: &["crates/fathom-analyze/src/deps/**"],
        est_cost_ms: 6,
    },
    Analyzer {
        id: "non_code.surface",
        version: 1,
        schema_version: 1,
        scope_kind: "tree",
        config_keys: &["non_code.patterns"],
        depends_on: &["taxonomy.path", "diff.per_file"],
        source_paths: &["crates/fathom-analyze/src/noncode/**"],
        est_cost_ms: 3,
    },
    Analyzer {
        id: "test.surface",
        version: 1,
        schema_version: 1,
        scope_kind: "tree",
        config_keys: &["test.globs"],
        depends_on: &["taxonomy.path", "diff.per_file"],
        source_paths: &["crates/fathom-analyze/src/testsurface/**"],
        est_cost_ms: 3,
    },
    Analyzer {
        id: "graph.build",
        version: 1,
        schema_version: 1,
        scope_kind: "tree",
        config_keys: &["layers.", "graph.ignored_dirs"],
        depends_on: &["taxonomy.path"],
        source_paths: &["crates/fathom-analyze/src/graph/**"],
        est_cost_ms: 45,
    },
    Analyzer {
        id: "graph.delta",
        version: 1,
        schema_version: 1,
        scope_kind: "tree",
        config_keys: &["layers."],
        depends_on: &["graph.build"],
        source_paths: &["crates/fathom-analyze/src/graph/**"],
        est_cost_ms: 8,
    },
    Analyzer {
        id: "graph.cycles",
        version: 1,
        schema_version: 1,
        scope_kind: "tree",
        config_keys: &["layers.", "ratchet."],
        depends_on: &["graph.delta"],
        source_paths: &["crates/fathom-analyze/src/cycles/**"],
        est_cost_ms: 5,
    },
    Analyzer {
        id: "blast_radius",
        version: 1,
        schema_version: 1,
        scope_kind: "episode",
        config_keys: &["blast.hops"],
        depends_on: &["graph.delta"],
        source_paths: &["crates/fathom-analyze/src/blast/**"],
        est_cost_ms: 4,
    },
    Analyzer {
        id: "findings.detect",
        version: 1,
        schema_version: 1,
        scope_kind: "episode",
        config_keys: &["detectors.enabled", "risk.weights"],
        depends_on: &[
            "structure.delta",
            "deps.resolve",
            "test.surface",
            "non_code.surface",
        ],
        source_paths: &["crates/fathom-analyze/src/detectors/**"],
        est_cost_ms: 9,
    },
    Analyzer {
        id: "risk.score",
        version: 1,
        schema_version: 1,
        scope_kind: "episode",
        config_keys: &["risk.weights", "risk.tau"],
        depends_on: &["findings.detect", "blast_radius"],
        source_paths: &["crates/fathom-analyze/src/risk/**"],
        est_cost_ms: 2,
    },
    Analyzer {
        // It *is* `EpisodeDelta` (e2.11), which is why it depends on everything the delta composes.
        id: "delta.compose",
        version: 1,
        schema_version: 1,
        scope_kind: "episode",
        config_keys: &[],
        depends_on: &[
            "structure.delta",
            "deps.resolve",
            "graph.delta",
            "test.surface",
            "non_code.surface",
            "findings.detect",
            "risk.score",
        ],
        source_paths: &["crates/fathom-analyze/src/delta/**"],
        est_cost_ms: 5,
    },
];

/// Look one analyzer up by id.
#[must_use]
pub fn analyzer(id: &str) -> Option<&'static Analyzer> {
    ANALYZERS.iter().find(|entry| entry.id == id)
}

/// Versions that differ from the manifest, for a caller that is testing a bump or running with a
/// different store schema (§5.2 fields 3, 4 and 9).
///
/// The point of the type is that a bump is an *argument*, never an edit: the same code that answers a
/// question about today's manifest answers one about "what if `taxonomy.path` were version 3", which is
/// what makes the §6 matrix testable instead of narratable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Versions {
    /// Per-analyzer overrides, keyed by analyzer id.
    pub analyzer: BTreeMap<String, u32>,
    /// The store schema version (key field 9). Defaults to [`crate::spine::SCHEMA_VERSION`].
    pub store_schema: Option<i64>,
    /// The compiled-in feature set this build was made with, as a short digest (ADR-002 §10.2, ADR-004 §5.1).
    ///
    /// `None` means the caller never said, and the graph/structure analyzers then stamp their version with
    /// [`UNSPECIFIED_FEATURE_TAG`] rather than pretending a feature set they did not measure. The crate that
    /// knows the build's features (`fathom-analyze`) supplies it at every real call site; a store-level test
    /// or a bump experiment can override it deliberately.
    pub feature_tag: Option<String>,
}

/// What a feature-sensitive analyzer's version says when the build never declared its feature set. Spelled
/// out rather than left blank, because "we were not told" and "we were told the empty set" have to be
/// distinguishable in a cache key.
pub const UNSPECIFIED_FEATURE_TAG: &str = "build-features-unspecified";

/// Which analyzers' output can change with the compiled-in feature set (ADR-002 §6.4, §10.2).
///
/// The graph and structure family, because those are the analyzers whose extractors are gated per language
/// (`lang-rust`, `lang-python`, …): a build with `lang-rust` off produces a *different* module graph from
/// one with it on, and a cached row must not cross that boundary in either direction. Everything else in the
/// manifest — normalisation, taxonomy, diffs, episodes — reads git and text, which no cargo feature turns
/// on or off.
#[must_use]
pub fn feature_sensitive(id: &str) -> bool {
    // One place decides, so no caller gets to improvise which analyzers are language-gated.
    id.starts_with("graph.") || id.starts_with("structure.")
}

impl Versions {
    /// The manifest's own versions: nothing overridden.
    #[must_use]
    pub fn current() -> Self {
        Versions::default()
    }

    /// Override one analyzer's version, as if its code had changed and its version had been bumped.
    #[must_use]
    pub fn bumped(mut self, id: &str, version: u32) -> Self {
        let _ = self.analyzer.insert(id.to_owned(), version);
        self
    }

    /// Override the store schema version, as if a migration had run.
    #[must_use]
    pub fn with_store_schema(mut self, schema: i64) -> Self {
        self.store_schema = Some(schema);
        self
    }

    /// Say what this build was compiled with (ADR-002 §10.2). `fathom-analyze` calls this on every real
    /// `Versions`, because it is the crate that carries the `lang-*` features; the store cannot see them.
    #[must_use]
    pub fn with_feature_tag(mut self, tag: impl Into<String>) -> Self {
        self.feature_tag = Some(tag.into());
        self
    }

    /// The feature-set digest in force, or the explicit "nobody said" tag.
    #[must_use]
    pub fn feature_tag(&self) -> &str {
        self.feature_tag
            .as_deref()
            .unwrap_or(UNSPECIFIED_FEATURE_TAG)
    }

    /// The version as the key and the manifest print it (ADR-004 §5.1).
    ///
    /// `semver` for everything, plus `+<feature digest>` for the analyzers whose output a compiled-in
    /// feature can change. This is the whole of ADR-002 §10.2's amendment: the key framing gains **no new
    /// field** — field 3 just carries a more honest value — so a row written by a `lang-rust` build can
    /// never answer a lookup made by one without it, and the difference shows up as a version bump that
    /// cascades through `dep_digest` (§5.4) rather than as a stale hit.
    #[must_use]
    pub fn effective_version(&self, id: &str) -> String {
        let version = self.version_of(id);
        if feature_sensitive(id) {
            format!("{version}+{}", self.feature_tag())
        } else {
            version.to_string()
        }
    }

    /// The version in force for `id`: the override if there is one, else the manifest.
    #[must_use]
    pub fn version_of(&self, id: &str) -> u32 {
        self.analyzer
            .get(id)
            .copied()
            .or_else(|| analyzer(id).map(|entry| entry.version))
            .unwrap_or(0)
    }

    /// The store schema version in force.
    #[must_use]
    pub fn store_schema(&self) -> i64 {
        self.store_schema.unwrap_or(SCHEMA_VERSION)
    }
}

/// The configuration an analyzer declared it reads, as the plain table `fathom.toml` decodes to.
///
/// Canonicalisation is the whole job: reordering `fathom.toml` must not be an invalidation event
/// (§5.5), so the digest is taken over sorted keys with compact separators, and the *values* keep their
/// own order (a list of globs in a different order is a different list).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config(pub Value);

impl Config {
    /// An empty configuration: every declared key resolves to `absent`.
    #[must_use]
    pub fn empty() -> Self {
        Config(Value::Object(serde_json::Map::new()))
    }

    /// Is it the empty table?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.as_object().is_none_or(serde_json::Map::is_empty)
    }
}

/// Look up one declared config key (§5.5).
///
/// A dotted key walks nested tables; a key ending in `.` means "the whole subtree, canonically" —
/// `layers.` covers every layer rule. Without that rule a prefix key silently resolves to `absent` for
/// every value of the subtree, and a config change an analyzer genuinely depends on invalidates
/// nothing. That bug was found by measuring the matrix (ADR-004 §13.2, `config_layers` invalidated 0
/// rows), not by reading the code, which is the ADR's whole argument for measuring invalidation.
fn config_value(config: &Config, dotted: &str) -> Value {
    let (path, whole_subtree) = match dotted.strip_suffix('.') {
        Some(stripped) => (stripped, true),
        None => (dotted, false),
    };
    let mut current = &config.0;
    for part in path.split('.') {
        match current.get(part) {
            Some(found) => current = found,
            None => return absent(dotted),
        }
    }
    if whole_subtree && !current.is_object() {
        return absent(dotted);
    }
    current.clone()
}

fn absent(key: &str) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("absent".to_owned(), Value::String(key.to_owned()));
    Value::Object(map)
}

/// The §5.5 config digest: only the declared keys, canonically serialised.
#[must_use]
pub fn config_digest(config: &Config, keys: &[&str]) -> String {
    let mut picked = BTreeMap::new();
    for key in keys {
        picked.insert(*key, config_value(config, key));
    }
    // `serde_json`'s default `Map` is a `BTreeMap`, so `to_string` is already sorted-key and compact —
    // the same canonical form Python reaches for with `sort_keys=True, separators=(",", ":")`.
    let canonical = serde_json::to_string(&picked).unwrap_or_else(|_| String::from("{}"));
    sha256_hex(&frame(&[canonical.as_bytes()]))
}

/// The transitive closure of an analyzer's dependencies, sorted, excluding itself (§5.4).
///
/// # Errors
///
/// [`Error::StoreCorrupt`] if an id in the closure is unknown to the manifest.
pub fn dependency_closure(id: &str) -> Result<Vec<&'static str>, Error> {
    let Some(root) = analyzer(id) else {
        return Err(unknown_analyzer(id));
    };
    let mut seen: Vec<&'static str> = Vec::new();
    let mut stack: Vec<&'static str> = root.depends_on.to_vec();
    while let Some(current) = stack.pop() {
        if current == id || seen.contains(&current) {
            continue;
        }
        let entry = analyzer(current).ok_or_else(|| unknown_analyzer(current))?;
        seen.push(current);
        stack.extend(entry.depends_on);
    }
    seen.sort_unstable();
    Ok(seen)
}

/// The digest over the *transitive* dependency versions (§5.4).
///
/// This is the whole cascade mechanism: bump `taxonomy.path` and every analyzer that (transitively)
/// depends on it derives a new key, with no invalidation code and without knowing the bump happened.
///
/// # Errors
///
/// As [`dependency_closure`].
pub fn dep_digest(id: &str, versions: &Versions) -> Result<String, Error> {
    let parts: Vec<String> = dependency_closure(id)?
        .into_iter()
        .map(|dep| format!("{dep}:{}", versions.effective_version(dep)))
        .collect();
    let framed: Vec<&[u8]> = parts.iter().map(String::as_bytes).collect();
    Ok(sha256_hex(&frame(&framed)))
}

/// A cache key, and the fields it was built from (§5.2).
///
/// The parts are kept beside the digest so a miss can be *explained*: `fathom index --stats` prints
/// `key_changed` (a row for this input under a different key) precisely because the key is a function of
/// visible inputs and not an opaque hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKey {
    /// `ck1:<64 hex>`, the primary key of a `cache` row.
    pub raw: String,
    /// Which analyzer produced the row.
    pub analyzer_id: &'static str,
    /// The scope kind the input digest was computed under (§5.3).
    pub input_kind: String,
    /// The scope digest itself.
    pub input_hash: String,
    /// Field 3.
    pub analyzer_version: u32,
    /// Field 4.
    pub dep_digest: String,
    /// Field 5.
    pub config_digest: String,
    /// Field 6.
    pub result_schema: u32,
}

impl std::fmt::Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Derive the cache key (§5.2).
///
/// The nine fields, and why each is load-bearing:
///
/// 1. `ck1` — the framing version: an old store's rows become unreachable, never misread.
/// 2. `analyzer_id` — which analyzer produced it.
/// 3. `analyzer_version` — the bump a developer performs when the code changes (§5.7).
/// 4. `dep_digest` — transitive dependency versions, the cascade (§5.4).
/// 5. `config_digest` — only the config the analyzer declared (§5.5).
/// 6. `result_schema` — the shape of the stored result (e2.11's version).
/// 7. `input_kind` — so a `path` digest can never answer a `tree` lookup.
/// 8. `input_hash` — the scope digest (§5.3).
/// 9. `store_schema` — a v1 row cannot answer a v2 query, which is what makes §9.5's
///    "migrations never migrate derived data" true by construction.
///
/// # Errors
///
/// [`Error::StoreCorrupt`] if the analyzer is unknown to the manifest or its dependency graph names an
/// analyzer that does not exist.
pub fn derive_key(
    analyzer_id: &str,
    input_hash: &str,
    input_kind: Option<&str>,
    versions: &Versions,
    config: &Config,
) -> Result<CacheKey, Error> {
    let entry = analyzer(analyzer_id).ok_or_else(|| unknown_analyzer(analyzer_id))?;
    let kind = input_kind.unwrap_or(entry.scope_kind);
    if !SCOPE_KINDS.contains(&kind) {
        return Err(Error::StoreCorrupt {
            what: format!("{kind} is not a cache scope kind (§5.3)"),
        });
    }
    let dep = dep_digest(analyzer_id, versions)?;
    let config = config_digest(config, entry.config_keys);
    let fields: Vec<String> = vec![
        KEY_SCHEMA.to_owned(),
        entry.id.to_owned(),
        versions.effective_version(entry.id),
        dep.clone(),
        config.clone(),
        entry.schema_version.to_string(),
        kind.to_owned(),
        input_hash.to_owned(),
        versions.store_schema().to_string(),
    ];
    let hashed: Vec<&[u8]> = fields.iter().map(String::as_bytes).collect();
    Ok(CacheKey {
        raw: format!("{KEY_SCHEMA}:{}", sha256_hex(&frame(&hashed))),
        analyzer_id: entry.id,
        input_kind: kind.to_owned(),
        input_hash: input_hash.to_owned(),
        analyzer_version: versions.version_of(entry.id),
        dep_digest: dep,
        config_digest: config,
        result_schema: entry.schema_version,
    })
}

/// What a lookup decided (§5.6). Not a boolean: the reason is the diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// A stored answer, and its epoch is current.
    Hit,
    /// A stored *failure* for this input: an answer, and a cheap one (§4.5).
    NegativeHit,
    /// No row for this input at all — the cache is cold, which is not a defect.
    Cold,
    /// A row exists for this input under a *different* key: something was bumped. This is the counter
    /// that separates "the cache is cold" from "an analyzer or a config changed", and §5.6's reason for
    /// keeping the parts of a key at all.
    KeyChanged,
    /// A row exists under this key but was built in an older epoch: the epoch moved, so it is unreachable
    /// until it is rebuilt (§6.3).
    EpochStale,
}

impl Reason {
    /// Whether the lookup found something to use.
    #[must_use]
    pub const fn is_hit(self) -> bool {
        matches!(self, Reason::Hit | Reason::NegativeHit)
    }

    /// The counter name suffix (`hit`, `cold`, `bump`, …) used in the `counter` table, matching the
    /// reference implementation's counter spellings so a dashboard reads the same either side.
    #[must_use]
    pub const fn counter(self) -> &'static str {
        match self {
            Reason::Hit => "hit",
            Reason::NegativeHit => "negative_hit",
            Reason::Cold => "cold",
            Reason::KeyChanged => "bump",
            Reason::EpochStale => "epoch",
        }
    }

    /// A human phrase for `fathom index --stats`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Reason::Hit => "hit",
            Reason::NegativeHit => "negative hit",
            Reason::Cold => "cold miss",
            Reason::KeyChanged => "key changed",
            Reason::EpochStale => "epoch stale",
        }
    }
}

/// The answer to one lookup: what was found, or exactly why nothing was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The key that was looked up.
    pub key: CacheKey,
    /// What the lookup decided.
    pub reason: Reason,
    /// The payload, when there was one and it could be read back.
    pub payload: Option<Vec<u8>>,
    /// A stored failure, on a negative hit.
    pub error: Option<String>,
    /// The payload's size in bytes, whether or not it was read back.
    pub bytes: u64,
}

impl Outcome {
    /// Whether there was an answer to use.
    #[must_use]
    pub fn hit(&self) -> bool {
        self.reason.is_hit()
    }
}

/// The §5 framing: a four-byte big-endian length before every field.
///
/// A separator-based key (`a + ":" + b`) collides the first time a field may itself contain the
/// separator — `noise:*` + `:` + `v2` against `noise` + `:` + `*:v2`. Length prefixes make that
/// structurally impossible, at the cost of four bytes a field.
#[must_use]
pub fn frame(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|part| part.len() + 4).sum());
    for part in parts {
        out.extend_from_slice(&(part.len() as u64).to_be_bytes()[4..]);
        out.extend_from_slice(part);
    }
    out
}

/// sha256, lowercase hex — the same digest `e1.2` uses for `path_hash` and `message_hash`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// The digest of a `path` scope: `sha256(path_hash ‖ blob_oid)` (§5.3).
///
/// The *blob*, not the path: content-addressing the file is what makes "editing one file invalidates
/// only analyses that consume that file" a property of the key instead of a bookkeeping rule someone has
/// to remember (§6, row two).
#[must_use]
pub fn path_digest(path_hash: &str, blob: &str) -> String {
    sha256_hex(&frame(&[path_hash.as_bytes(), blob.as_bytes()]))
}

/// The digest of a `path_pair` scope: `(path, old blob, new blob)` (§5.3).
///
/// An absent side is the empty string, which is a distinct input from any blob id and so cannot collide
/// with a real one.
#[must_use]
pub fn path_pair_digest(path_hash: &str, old_blob: &str, new_blob: &str) -> String {
    sha256_hex(&frame(&[
        path_hash.as_bytes(),
        old_blob.as_bytes(),
        new_blob.as_bytes(),
    ]))
}

/// The digest of an `episode` scope: identity ‖ fingerprint (§5.3).
#[must_use]
pub fn episode_digest(identity: &str, fingerprint: &str) -> String {
    sha256_hex(&frame(&[identity.as_bytes(), fingerprint.as_bytes()]))
}

/// Every structural problem with a manifest: unknown scope kinds, unknown or self dependencies, and
/// cycles (§5.4).
///
/// A cycle makes the dependency digest unsatisfiable, and an unsatisfiable digest is indistinguishable
/// from a cache that never hits — which is why this is checked at startup and not discovered in the field.
#[must_use]
pub fn manifest_problems() -> Vec<String> {
    let mut problems = Vec::new();
    for entry in ANALYZERS {
        if !SCOPE_KINDS.contains(&entry.scope_kind) {
            problems.push(format!(
                "{}: unknown scope kind {:?}",
                entry.id, entry.scope_kind
            ));
        }
        for dep in entry.depends_on {
            if analyzer(dep).is_none() {
                problems.push(format!("{}: depends on unknown analyzer {dep:?}", entry.id));
            }
            if *dep == entry.id {
                problems.push(format!("{}: depends on itself", entry.id));
            }
        }
    }
    // Colour-sort cycle detection (white: unvisited, grey: on the current path, black: done). A cycle is
    // reported once per edge that closes it, with the path that closes it, because "there is a cycle"
    // without the path is a report nobody can act on.
    let mut state: BTreeMap<&'static str, u8> =
        ANALYZERS.iter().map(|entry| (entry.id, 0_u8)).collect();
    for entry in ANALYZERS {
        visit(entry.id, &mut Vec::new(), &mut state, &mut problems);
    }
    problems
}

fn visit(
    id: &'static str,
    stack: &mut Vec<&'static str>,
    state: &mut BTreeMap<&'static str, u8>,
    out: &mut Vec<String>,
) {
    match state.get(id).copied() {
        Some(2) | None => return,
        Some(1) => {
            let mut path = stack.clone();
            path.push(id);
            out.push(format!("dependency cycle: {}", path.join(" -> ")));
            return;
        }
        Some(_) => {}
    }
    if let Some(slot) = state.get_mut(id) {
        *slot = 1;
    }
    stack.push(id);
    if let Some(entry) = analyzer(id) {
        for dep in entry.depends_on {
            visit(dep, stack, state, out);
        }
    }
    stack.pop();
    if let Some(slot) = state.get_mut(id) {
        *slot = 2;
    }
}

fn unknown_analyzer(id: &str) -> Error {
    Error::StoreCorrupt {
        what: format!("{id} is not in the analyzer manifest (§5.1)"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn config(pairs: &[(&str, Value)]) -> Config {
        let mut map = serde_json::Map::new();
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        Config(Value::Object(map))
    }

    #[test]
    fn the_manifest_is_a_dag_of_known_analyzers() {
        assert_eq!(
            manifest_problems(),
            Vec::<String>::new(),
            "the shipped manifest must be cycle-free and reference-free"
        );
        assert!(
            ANALYZERS.len() >= 16,
            "ADR-004 §5.1's table is the manifest; dropping a row from it is a contract change"
        );
    }

    #[test]
    fn framing_is_unambiguous_where_a_separator_would_collide() {
        // The collision §5.2 names: `noise:*` + `v2` against `noise` + `*:v2`. Joined, identical; framed,
        // different, because the length prefix says where each field ends.
        let left = frame(&[b"noise:*", b"v2"]);
        let right = frame(&[b"noise", b"*:v2"]);
        assert_ne!(left, right);
        assert_ne!(
            sha256_hex(&left),
            sha256_hex(&right),
            "and the digests of the two framings differ too, which is the property the key relies on"
        );
    }

    #[test]
    fn a_dependency_bump_moves_the_digest_of_everything_downstream_of_it() {
        let before = dep_digest("findings.detect", &Versions::current()).unwrap();
        let after = dep_digest(
            "findings.detect",
            &Versions::current().bumped("taxonomy.path", 99),
        )
        .unwrap();
        assert_ne!(before, after, "taxonomy.path is four levels up its DAG");
        // `episode.segment` depends on `ingest.normalize` and on nothing else, so a taxonomy bump must
        // leave its digest alone — the half of §5.4 that a naive "rehash everything" implementation gets
        // wrong in the other direction.
        assert_eq!(
            dep_digest("episode.segment", &Versions::current()).unwrap(),
            dep_digest(
                "episode.segment",
                &Versions::current().bumped("taxonomy.path", 99)
            )
            .unwrap(),
            "episode.segment is outside taxonomy.path's closure, so it must not move"
        );
        // And an analyzer with no dependencies at all has the empty closure's digest, whatever anyone else
        // was bumped to.
        assert_eq!(
            dep_digest("ingest.normalize", &Versions::current()).unwrap(),
            dep_digest(
                "ingest.normalize",
                &Versions::current().bumped("graph.build", 7)
            )
            .unwrap(),
            "no dependencies, nothing to cascade"
        );
    }

    #[test]
    fn a_config_change_hits_only_the_analyzers_that_declared_it() {
        let noise = serde_json::json!({"globs": ["generated/**"], "builtins_version": 1});
        let with_cargo = config(&[
            ("noise", noise.clone()),
            ("deps", serde_json::json!({"ecosystems": ["cargo"]})),
        ]);
        let with_every_ecosystem = config(&[
            ("noise", noise.clone()),
            (
                "deps",
                serde_json::json!({"ecosystems": ["cargo", "npm", "crates"]}),
            ),
        ]);
        let taxonomy = derive_key(
            "taxonomy.path",
            "x",
            None,
            &Versions::current(),
            &with_cargo,
        )
        .unwrap();
        let unrelated = derive_key(
            "taxonomy.path",
            "x",
            None,
            &Versions::current(),
            &with_every_ecosystem,
        )
        .unwrap();
        assert_eq!(
            taxonomy.config_digest, unrelated.config_digest,
            "deps.ecosystems is not in taxonomy.path's declared keys, so no value it can take moves it"
        );
        // And the converse half: the same change *does* move `deps.resolve`, which did declare it.
        assert_ne!(
            derive_key("deps.resolve", "t", None, &Versions::current(), &with_cargo)
                .unwrap()
                .config_digest,
            derive_key(
                "deps.resolve",
                "t",
                None,
                &Versions::current(),
                &with_every_ecosystem
            )
            .unwrap()
            .config_digest,
            "an analyzer that declared the key must feel the change"
        );
        let changed = derive_key(
            "taxonomy.path",
            "x",
            None,
            &Versions::current(),
            &config(&[(
                "noise",
                serde_json::json!({"globs": ["vendor/**"], "builtins_version": 1}),
            )]),
        )
        .unwrap();
        assert_ne!(
            taxonomy.config_digest, changed.config_digest,
            "a declared key that changed must move the digest"
        );
    }

    #[test]
    fn a_prefix_key_covers_the_whole_subtree() {
        // §5.5's trap, asserted: `layers.` must move when any layer rule moves.
        let a = config(&[("layers", serde_json::json!({"ui": ["a/**"]}))]);
        let b = config(&[(
            "layers",
            serde_json::json!({"ui": ["a/**"], "core": ["b/**"]}),
        )]);
        let before = config_digest(&a, &["layers."]);
        assert_ne!(before, config_digest(&b, &["layers."]));
        assert_eq!(
            config_digest(&Config::empty(), &["layers."]),
            config_digest(
                &config(&[("layers", Value::String("nope".into()))]),
                &["layers."]
            ),
            "a prefix key over a non-table is `absent`, which is what the rule says rather than a bug"
        );
    }

    #[test]
    fn reordering_the_config_is_not_an_invalidation() {
        let one = config(&[
            ("noise", serde_json::json!({"globs": ["a", "b"]})),
            ("diff", serde_json::json!({"context_lines": 3})),
        ]);
        let two = config(&[
            ("diff", serde_json::json!({"context_lines": 3})),
            ("noise", serde_json::json!({"globs": ["a", "b"]})),
        ]);
        assert_eq!(
            derive_key("diff.per_file", "h", None, &Versions::current(), &one).unwrap(),
            derive_key("diff.per_file", "h", None, &Versions::current(), &two).unwrap(),
            "the same table written in a different order is the same key"
        );
    }

    #[test]
    fn a_store_schema_bump_makes_every_row_unreachable() {
        let now = derive_key(
            "structure.delta",
            "t",
            None,
            &Versions::current(),
            &Config::empty(),
        )
        .unwrap();
        let after = derive_key(
            "structure.delta",
            "t",
            None,
            &Versions::current().with_store_schema(SCHEMA_VERSION + 1),
            &Config::empty(),
        )
        .unwrap();
        assert_ne!(
            now.raw, after.raw,
            "§9.5: a migration invalidates, it does not migrate"
        );
    }

    #[test]
    fn a_scope_kind_that_does_not_exist_is_refused() {
        let err = derive_key(
            "structure.delta",
            "t",
            Some("nonsense"),
            &Versions::current(),
            &Config::empty(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("scope kind"), "{err}");
    }

    #[test]
    fn an_unknown_analyzer_is_refused_rather_than_guessed_at() {
        assert!(
            derive_key(
                "not.an.analyzer",
                "x",
                None,
                &Versions::current(),
                &Config::empty()
            )
            .unwrap_err()
            .to_string()
            .contains("analyzer manifest")
        );
        assert!(dependency_closure("still.not.an.analyzer").is_err());
    }

    #[test]
    fn scope_digests_separate_path_from_pair_from_episode() {
        let path = path_digest("p", "b");
        let pair = path_pair_digest("p", "", "b");
        let episode = episode_digest("e", "f");
        assert_ne!(path, pair);
        assert_ne!(path, episode);
        assert_eq!(
            path,
            path_digest("p", "b"),
            "a digest that is not a function of its inputs is not a cache key"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The store side: `analyzer`, `cache`, `blob`, `blob_ref` and `counter` (ADR-004 §4.5, §4.8), and
// the two operations every analyzer goes through.
// ---------------------------------------------------------------------------------------------

/// Migration 2: the analysis cache. Verbatim from ADR-004 Appendix A §4.5 and §4.8, with two extra
/// indexes (§8.6's LRU wants `last_used_utc`, and the pruner wants `epoch`) and nothing else added.
pub const MIGRATION_2: &str = "
CREATE TABLE IF NOT EXISTS analyzer (
  id            TEXT PRIMARY KEY,          -- 'structure.delta', 'deps.resolve', 'graph.build'
  version       INTEGER NOT NULL,          -- bumped in code; CI checks the bump (e0.11)
  schema_version INTEGER NOT NULL,         -- version of the result shape stored in `delta`/`cache`
  scope_kind    TEXT NOT NULL CHECK (scope_kind IN
                   ('repo','tree','commit','commit_pair','path_pair','path','episode','blob','ref')),
  config_keys   TEXT NOT NULL,             -- JSON array of fathom.toml paths consumed, [] allowed
  depends_on    TEXT NOT NULL,             -- JSON array of analyzer ids, acyclic (§5.4)
  source_paths  TEXT NOT NULL,             -- JSON globs; touching them without a version bump fails CI
  enabled       INTEGER NOT NULL DEFAULT 1
) STRICT;

CREATE TABLE IF NOT EXISTS cache (
  cache_key   TEXT PRIMARY KEY,            -- 'ck1:' || sha256(...) per §5.2
  analyzer_id TEXT NOT NULL REFERENCES analyzer(id),
  input_kind  TEXT NOT NULL,
  input_hash  TEXT NOT NULL,               -- the scope digest: tree/blob/episode/...
  analyzer_version INTEGER NOT NULL,
  dep_digest  TEXT NOT NULL,               -- digest of the transitive dependency versions (§5.4)
  config_digest TEXT NOT NULL,             -- digest of only the declared config keys (§5.5)
  result_sha  TEXT,                        -- -> blob.sha256; NULL when the run errored
  result_bytes INTEGER NOT NULL DEFAULT 0,
  error       TEXT,                        -- stored failure: a miss is not the only negative answer
  built_utc   INTEGER NOT NULL,
  last_used_utc INTEGER NOT NULL,
  hits        INTEGER NOT NULL DEFAULT 0,
  cost_ms     INTEGER NOT NULL,
  epoch       INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS cache_input ON cache(input_kind, input_hash, analyzer_id);
CREATE INDEX IF NOT EXISTS cache_epoch ON cache(epoch);
CREATE INDEX IF NOT EXISTS cache_lru ON cache(last_used_utc);

CREATE TABLE IF NOT EXISTS blob (
  sha256  TEXT PRIMARY KEY,
  kind    TEXT NOT NULL,                   -- delta|graph_chunk|diff_summary|analysis|artifact
  bytes   INTEGER NOT NULL,
  storage TEXT NOT NULL CHECK (storage IN ('inline','file','omitted')),
  data    BLOB,                            -- non-null iff storage='inline'
  created_utc INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS blob_ref (
  owner_kind TEXT NOT NULL,                -- cache|delta|finding|graph_snapshot
  owner_id   TEXT NOT NULL,
  blob_sha   TEXT NOT NULL REFERENCES blob(sha256),
  PRIMARY KEY (owner_kind, owner_id, blob_sha)
) WITHOUT ROWID, STRICT;

CREATE TABLE IF NOT EXISTS counter (
  name TEXT PRIMARY KEY,                   -- 'cache.miss.graph.build', 'ingest.bad_lines', ...
  value INTEGER NOT NULL,
  updated_utc INTEGER NOT NULL
) STRICT;
";

/// The two spellings of a payload's home in `blob.storage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobStorage {
    /// In the row.
    Inline,
    /// Under `objects/` beside the store file.
    File,
}

impl BlobStorage {
    /// The column's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            BlobStorage::Inline => "inline",
            BlobStorage::File => "file",
        }
    }
}

/// Where an analysis payload's home is, derived from its size — one rule, so no caller invents its own.
#[must_use]
pub const fn blob_storage(bytes: u64) -> BlobStorage {
    if bytes <= INLINE_BLOB_BYTES {
        BlobStorage::Inline
    } else {
        BlobStorage::File
    }
}

impl crate::spine::Store {
    /// Write the compiled-in manifest into `analyzer`, updating versions in place (§5.1).
    ///
    /// Run on every open: the manifest is code, and a store that kept a stale copy of it would answer a
    /// lookup with yesterday's version — the one failure mode §5.7's CI gate exists to prevent,
    /// reintroduced by storage instead of by a forgotten bump.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the manifest cannot be written, or if it is structurally broken — the
    /// cycle and unknown-dependency checks of §5.4 run here, because a store built on an unsatisfiable
    /// dependency graph is a store whose cache will never hit, and that is worth failing loudly over.
    pub fn register_analyzers(&mut self) -> Result<usize, Error> {
        let problems = manifest_problems();
        if !problems.is_empty() {
            return Err(Error::StoreCorrupt {
                what: format!("the analyzer manifest is broken: {}", problems.join("; ")),
            });
        }
        let mut written = 0_usize;
        for entry in ANALYZERS {
            let config_keys = serde_json::to_string(entry.config_keys)
                .map_err(|err| corrupt_json(err, "config keys"))?;
            let depends_on = serde_json::to_string(entry.depends_on)
                .map_err(|err| corrupt_json(err, "depends_on"))?;
            let source_paths = serde_json::to_string(entry.source_paths)
                .map_err(|err| corrupt_json(err, "source_paths"))?;
            written += self
                .conn_mut()
                .execute(
                    "INSERT INTO analyzer(id, version, schema_version, scope_kind, config_keys,
                                          depends_on, source_paths, enabled)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
                     ON CONFLICT(id) DO UPDATE SET version = excluded.version,
                        schema_version = excluded.schema_version, scope_kind = excluded.scope_kind,
                        config_keys = excluded.config_keys, depends_on = excluded.depends_on,
                        source_paths = excluded.source_paths",
                    params![
                        entry.id,
                        i64::from(entry.version),
                        i64::from(entry.schema_version),
                        entry.scope_kind,
                        config_keys,
                        depends_on,
                        source_paths
                    ],
                )
                .map_err(|err| corrupt("registering analyzers", &err))?;
        }
        Ok(written)
    }

    /// Look a cached result up, and account for the lookup (§5.6).
    ///
    /// This is the only place in the product where a hit and a miss exist, which is why `fathom index
    /// --stats` can print per-analyzer rates without a single analyzer cooperating. The reason comes back
    /// rather than being logged and forgotten: `cold` and `bump` are the difference between "the cache is
    /// empty" and "something was bumped", and only one of those is worth a person's afternoon.
    ///
    /// Counters are accumulated, not written here — see [`crate::spine::Store::bump_counter`].
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the manifest or the store cannot be read, or if the analyzer is unknown.
    pub fn resolve(
        &mut self,
        analyzer_id: &str,
        input_hash: &str,
        input_kind: Option<&str>,
        versions: &Versions,
        config: &Config,
        now: i64,
    ) -> Result<Outcome, Error> {
        let key = derive_key(analyzer_id, input_hash, input_kind, versions, config)?;
        self.bump_counter(&format!("cache.lookup.{analyzer_id}"), 1);
        let found = self
            .connection()
            .query_row(
                "SELECT result_sha, error, epoch, result_bytes FROM cache WHERE cache_key = ?1",
                params![key.raw],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|err| corrupt("looking up a cached analysis", &err))?;
        let epoch = self.epoch()?;
        let Some((result_sha, error, row_epoch, result_bytes)) = found else {
            // A row for the same input under a *different* key is the diagnostic case: it proves the miss
            // came from a version or config move rather than from new input (§5.6).
            //
            // The predicate carries `input_kind`, for two reasons and only one of them is speed.
            // `input_hash` is a digest taken *within* a scope kind: a blob digest and a tree digest are
            // different namespaces that can name the same bytes, so a twin sought across kinds can be found
            // in an unrelated row and file a cold miss as a version move — the one thing this diagnostic
            // exists to distinguish. And `cache_input(input_kind, input_hash, analyzer_id)`, the index built
            // for exactly this question, has `input_kind` as its leading column: without it SQLite scans
            // every cache row it has (`SCAN cache USING COVERING INDEX cache_input`), and because this runs
            // once per *miss* — once per file row of every commit of every cold index — the run's cost goes
            // quadratic in the number of rows the run is itself adding. Measured on a 31 899-row cache from
            // a real repository: 2.4 ms per miss, and every cold index makes tens of thousands of misses
            // (`reco-66q`). With the kind in the predicate it is a seek.
            let twin: bool = self
                .connection()
                .query_row(
                    "SELECT count(*) FROM cache
                     WHERE input_kind = ?1 AND input_hash = ?2 AND analyzer_id = ?3",
                    params![key.input_kind, input_hash, analyzer_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;
            let reason = if twin {
                Reason::KeyChanged
            } else {
                Reason::Cold
            };
            self.count_miss(analyzer_id, reason);
            return Ok(Outcome {
                key,
                reason,
                payload: None,
                error: None,
                bytes: 0,
            });
        };
        if row_epoch != epoch {
            self.count_miss(analyzer_id, Reason::EpochStale);
            return Ok(Outcome {
                key,
                reason: Reason::EpochStale,
                payload: None,
                error: None,
                bytes: 0,
            });
        }
        self.bump_counter(
            &format!(
                "cache.{}.{analyzer_id}",
                reason_of(error.is_some()).counter()
            ),
            1,
        );
        self.bump_counter(
            if error.is_some() {
                "cache.negative_hit"
            } else {
                "cache.hit"
            },
            1,
        );
        self.bump_counter(&format!("cache.bytes.{analyzer_id}"), result_bytes);
        // The touch is what an LRU needs and what §8.6's protection set is measured against. It is a
        // write, and it is deliberately the only one a hit costs.
        let _ = self.conn_mut().execute(
            "UPDATE cache SET hits = hits + 1, last_used_utc = ?2 WHERE cache_key = ?1",
            params![key.raw, now],
        );
        let payload = match (&result_sha, error.as_ref()) {
            (Some(sha), None) => self.read_blob(sha.as_str())?,
            _ => None,
        };
        Ok(Outcome {
            key,
            reason: reason_of(error.is_some()),
            payload,
            error,
            bytes: result_bytes as u64,
        })
    }

    /// Store an answer — including a *failed* answer, which is an answer (§4.5, §5.6).
    ///
    /// Without negative entries a broken analyzer is re-run on every re-index and its failure stays
    /// invisible: expensive and silent, the two properties this store is not allowed to have.
    ///
    /// The upsert updates `epoch` as well as the payload. The reference implementation's `put()` left
    /// `epoch` out of its `ON CONFLICT` clause, which means a row rebuilt in a new epoch stayed marked with
    /// the old one — a rebuild that could not heal its own rows. That is a bug in the contract, and the
    /// Rust copy fixes it rather than reproducing it.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if anything fails; the row is then absent, not half-written.
    pub fn put(
        &mut self,
        key: &CacheKey,
        payload: &[u8],
        error: Option<&str>,
        cost_ms: u64,
        now: i64,
    ) -> Result<(), Error> {
        let epoch = self.epoch()?;
        // A stored failure has no payload: the answer *is* the error, and storing the empty bytes that
        // produced it would give a reader something to mistrust.
        let sha = if error.is_some() {
            None
        } else {
            Some(sha256_hex(payload))
        };
        let known = sha.as_ref().is_some_and(|sha| {
            self.connection()
                .query_row(
                    "SELECT count(*) FROM blob WHERE sha256 = ?1",
                    params![sha],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0
        });
        // An oversized payload's *file* has to be on disk before the rows that name it, and it cannot be
        // inside the transaction. That leaves exactly one window in which a `kill -9` can strand a file:
        // the content-addressed bytes are written and the row naming them is not. A stranded file is
        // unreachable garbage — no row can name it, so nothing can read it — and §8.3's retention is what
        // removes it. A stranded *row* would be worse, which is why the rows go in one transaction below.
        if let (Some(sha), false) = (&sha, known) {
            self.stage_object(sha, payload)?;
        }
        let tx = self.immediate_transaction()?;
        if let (Some(sha), false) = (&sha, known) {
            let storage = blob_storage(u64::try_from(payload.len()).unwrap_or(u64::MAX));
            tx.execute(
                "INSERT INTO blob(sha256, kind, bytes, storage, data, created_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(sha256) DO NOTHING",
                params![
                    sha,
                    "analysis",
                    i64::try_from(payload.len()).unwrap_or(i64::MAX),
                    storage.as_str(),
                    matches!(storage, BlobStorage::Inline).then(|| payload.to_vec()),
                    now
                ],
            )
            .map_err(|err| corrupt("writing a blob", &err))?;
        }
        if let Some(sha) = &sha {
            tx.execute(
                "INSERT INTO blob_ref(owner_kind, owner_id, blob_sha) VALUES ('cache', ?1, ?2)
                 ON CONFLICT(owner_kind, owner_id, blob_sha) DO NOTHING",
                params![key.raw, sha],
            )
            .map_err(|err| corrupt("referencing a blob", &err))?;
        }
        tx.execute(
            "INSERT INTO cache(cache_key, analyzer_id, input_kind, input_hash,
                               analyzer_version, dep_digest, config_digest, result_sha,
                               result_bytes, error, built_utc, last_used_utc, hits, cost_ms,
                               epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, ?13, ?14)
             ON CONFLICT(cache_key) DO UPDATE SET result_sha = excluded.result_sha,
                result_bytes = excluded.result_bytes, error = excluded.error,
                built_utc = excluded.built_utc, last_used_utc = excluded.last_used_utc,
                cost_ms = excluded.cost_ms, epoch = excluded.epoch",
            params![
                key.raw,
                key.analyzer_id,
                key.input_kind,
                key.input_hash,
                i64::from(key.analyzer_version),
                key.dep_digest,
                key.config_digest,
                sha,
                i64::try_from(payload.len()).unwrap_or(i64::MAX),
                error,
                now,
                now,
                i64::try_from(cost_ms).unwrap_or(i64::MAX),
                epoch
            ],
        )
        .map_err(|err| corrupt("writing a cache row", &err))?;
        tx.commit()
            .map_err(|err| corrupt("writing a cache row", &err))?;
        self.bump_counter(&format!("cache.put.{}", key.analyzer_id), 1);
        self.bump_counter(
            &format!("cache.cost_ms.{}", key.analyzer_id),
            i64::try_from(cost_ms).unwrap_or(i64::MAX),
        );
        Ok(())
    }

    /// Whether a key is present at all, for a caller that wants to ask without reading the payload.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn has_cached(&self, key: &CacheKey) -> Result<bool, Error> {
        self.connection()
            .query_row(
                "SELECT count(*) FROM cache WHERE cache_key = ?1",
                params![key.raw],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .map_err(|err| corrupt("checking a cache key", &err))
    }

    /// Store a payload, content-addressed, and return `(sha256, bytes, storage)` (§4.5).
    ///
    /// Dedupe is a consequence of the addressing rather than a policy: two analyses of byte-identical
    /// content store one payload, because the address *is* the content.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails, or if a `file`-class payload has nowhere to go (an
    /// in-memory store has no `objects/` directory, and pretending otherwise would lose bytes silently).
    pub fn put_blob(
        &mut self,
        kind: &str,
        bytes: &[u8],
        now: i64,
    ) -> Result<(String, u64, BlobStorage), Error> {
        let sha = sha256_hex(bytes);
        let len = bytes.len() as u64;
        let storage = blob_storage(len);
        let known: bool = self
            .connection()
            .query_row(
                "SELECT count(*) FROM blob WHERE sha256 = ?1",
                params![sha],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .map_err(|err| corrupt("checking a blob", &err))?;
        if !known {
            match storage {
                BlobStorage::Inline => {
                    self.conn_mut()
                        .execute(
                            "INSERT INTO blob(sha256, kind, bytes, storage, data, created_utc)
                         VALUES (?1, ?2, ?3, 'inline', ?4, ?5)
                         ON CONFLICT(sha256) DO NOTHING",
                            params![
                                sha,
                                kind,
                                i64::try_from(len).unwrap_or(i64::MAX),
                                bytes,
                                now
                            ],
                        )
                        .map_err(|err| corrupt("writing a blob", &err))?;
                }
                BlobStorage::File => {
                    let path = self.object_path(&sha).ok_or(Error::StoreCorrupt {
                        what: String::from(
                            "this store has no directory to put an oversized payload in \
                             (an in-memory store cannot hold a file-class blob)",
                        ),
                    })?;
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|err| corrupt("creating objects/", &io_error(err)))?;
                    }
                    // Content-addressed, so a partial file is impossible to confuse with a real one: write
                    // beside it and rename, the same discipline §9.3 uses for the store itself.
                    let temp = path.with_extension("tmp");
                    std::fs::write(&temp, bytes)
                        .map_err(|err| corrupt("writing an object", &io_error(err)))?;
                    std::fs::rename(&temp, &path)
                        .map_err(|err| corrupt("moving an object into place", &io_error(err)))?;
                    self.conn_mut()
                        .execute(
                            "INSERT INTO blob(sha256, kind, bytes, storage, data, created_utc)
                             VALUES (?1, ?2, ?3, 'file', NULL, ?4)
                             ON CONFLICT(sha256) DO NOTHING",
                            params![sha, kind, i64::try_from(len).unwrap_or(i64::MAX), now],
                        )
                        .map_err(|err| corrupt("writing a blob", &err))?;
                }
            }
        }
        Ok((sha, len, storage))
    }

    /// Put an oversized payload's bytes on disk, ready for a row to name them.
    ///
    /// Write-beside-and-rename, the same discipline §9.3 uses for the store file itself: a file that a
    /// `kill -9` caught halfway is never visible under the name a reader would use.
    fn stage_object(&self, sha: &str, bytes: &[u8]) -> Result<(), Error> {
        if matches!(
            blob_storage(u64::try_from(bytes.len()).unwrap_or(u64::MAX)),
            BlobStorage::Inline
        ) {
            return Ok(());
        }
        let path = self.object_path(sha).ok_or(Error::StoreCorrupt {
            what: String::from(
                "this store has no directory to put an oversized payload in \
                 (an in-memory store cannot hold a file-class blob)",
            ),
        })?;
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| corrupt("creating objects/", &io_error(err)))?;
        }
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, bytes).map_err(|err| corrupt("writing an object", &io_error(err)))?;
        std::fs::rename(&temp, &path)
            .map_err(|err| corrupt("moving an object into place", &io_error(err)))?;
        Ok(())
    }

    /// Read a payload back, or `None` when it is gone — a missing blob is a miss, not a corruption, and
    /// the caller re-derives.
    ///
    /// Reads go through the process's bounded payload cache first ([`Store::set_ceilings`]), because a
    /// monorepo's analysis payloads are content-addressed and therefore deduplicated: a thousand files
    /// with the same generated stub are a thousand lookups of one blob. The cache returns a copy and
    /// never a borrow — the win is not paying SQLite or `objects/` again, not avoiding a memcpy — and
    /// the ceiling bounds what that costs.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] only if the row exists and cannot be read at all.
    pub fn read_blob(&self, sha: &str) -> Result<Option<Vec<u8>>, Error> {
        let key = sha.to_owned();
        if let Some(cached) = self.payloads.get_with(&key, |bytes| (**bytes).clone()) {
            return Ok(Some(cached));
        }
        let found = self.read_blob_uncached(sha)?;
        if let Some(bytes) = &found {
            self.payloads.insert(
                key,
                std::sync::Arc::new(bytes.clone()),
                u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                "analysis",
            );
        }
        Ok(found)
    }

    /// The store's own answer for one payload, cache not consulted.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the row exists and cannot be read at all.
    fn read_blob_uncached(&self, sha: &str) -> Result<Option<Vec<u8>>, Error> {
        let row = self
            .connection()
            .query_row(
                "SELECT storage, data FROM blob WHERE sha256 = ?1",
                params![sha],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
            )
            .optional()
            .map_err(|err| corrupt("reading a blob", &err))?;
        let Some((storage, data)) = row else {
            return Ok(None);
        };
        if let Some(data) = data {
            return Ok(Some(data));
        }
        match storage.as_str() {
            "file" => Ok(self
                .object_path(sha)
                .and_then(|path| std::fs::read(path).ok())),
            // `omitted` is a deliberate absence (ADR-004 §7.7's size cap): the row exists so a surface can
            // say "artifact not stored: over the size cap" instead of implying nothing was found.
            _ => Ok(None),
        }
    }

    /// Where a `file`-class payload lives: `objects/<aa>/<rest>` beside the store file (§3).
    #[must_use]
    pub fn object_path(&self, sha: &str) -> Option<std::path::PathBuf> {
        let (prefix, rest) = sha.split_at(2);
        Some(self.fathom_dir()?.join("objects").join(prefix).join(rest))
    }

    /// The directory the store file sits in — `.fathom/`, in the layout §3 describes, wherever the caller
    /// actually put the database.
    #[must_use]
    pub fn fathom_dir(&self) -> Option<std::path::PathBuf> {
        self.path()?.parent().map(std::path::Path::to_path_buf)
    }
}

fn reason_of(errored: bool) -> Reason {
    if errored {
        Reason::NegativeHit
    } else {
        Reason::Hit
    }
}

impl crate::spine::Store {
    /// The counter increments one miss costs: the per-analyzer name (what `--stats` prints) and the total
    /// (what a one-line summary reads).
    fn count_miss(&mut self, analyzer_id: &str, reason: Reason) {
        self.bump_counter("cache.miss", 1);
        self.bump_counter(&format!("cache.miss.{analyzer_id}"), 1);
        self.bump_counter(&format!("cache.{}.{analyzer_id}", reason.counter()), 1);
    }
}

/// Wrap an I/O failure as a rusqlite failure so every helper here can report one error type.
fn io_error(err: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(err))
}

fn corrupt_json(err: serde_json::Error, what: &str) -> Error {
    Error::StoreCorrupt {
        what: format!("encoding {what}: {err}"),
    }
}

fn corrupt(what: &str, err: &rusqlite::Error) -> Error {
    Error::StoreCorrupt {
        what: format!("{what}: {err}"),
    }
}
