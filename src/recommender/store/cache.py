"""ADR-004 §5 — cache keys, analyzer manifests and the invalidation-by-derivation rule.

The rule this module exists to make unavoidable:

> **A cached result is addressed by everything it actually depended on.** Therefore
> "invalidate X" is not an operation anyone performs: it is what happens automatically
> when the derived key for an input stops matching the key stored for it. There is no
> invalidation code path, and so no invalidation code path to forget.

Consequences a later ticket can cite: `derive_key` (§5.2) is the only place framing
happens, :class:`Analyzer` (§5.1) is the manifest whose columns *are* the invalidation
matrix of §6, and :func:`resolve` (§5.6) is the one function that counts hits and
misses — which is why `fathom index --stats` (e1.4) can print per-analyzer hit rates
without any analyzer cooperating.
"""

from __future__ import annotations

import hashlib
import json
import time
from dataclasses import dataclass, field

KEY_SCHEMA = "ck1"

#: The scope kinds a cache input may be addressed by (§5.3). The kind decides *how* the
#: input digest is computed; the digest itself is what goes into the key.
SCOPE_KINDS = (
    "repo",          # whole-repository fact: repo head set + config, for repo-level analyses
    "tree",          # a tree oid: structure delta, graph snapshot, scorecard
    "commit",        # one commit: normalization, attribution inputs
    "commit_pair",   # diff between two commits: hunk/diff analysis
    "path",          # (path, blob oid) — the per-file unit (e1.4's invalidation test)
    "path_pair",     # (path, old blob, new blob): rename detection, per-file diff analysis
    "episode",       # episode identity + fingerprint (ADR-003)
    "blob",          # a single blob: taxonomy, generated-file detection
    "ref",           # a ref and its tip: lineage facts
)


# ---------------------------------------------------------------------------
# framing (§5.2): length-prefixed, so a key can never be ambiguous
# ---------------------------------------------------------------------------


def _frame(parts: list[str]) -> bytes:
    """Length-prefix each field: ``len || bytes``.

    A separator-based key (`a + ":" + b`) collides the first time a field may itself
    contain the separator — `"noise:*" + ":" + "v2"` and `"noise" + ":" + "*:v2"`.
    Length prefixes make that structurally impossible, at the cost of four bytes.
    """
    out = bytearray()
    for p in parts:
        b = p.encode("utf-8")
        out += len(b).to_bytes(4, "big")
        out += b
    return bytes(out)


def _sha(parts: list[str]) -> str:
    return hashlib.sha256(_frame(parts)).hexdigest()


def derive_config_digest(config: dict, keys: list[str]) -> str:
    """Digest *only* the config a analyzer declared it consumes (§5.5).

    Values are canonicalised (sorted keys, compact separators) so that reordering a
    table in `fathom.toml` — or the config loader's own key order — is not an
    invalidation event.
    """
    picked = {}
    for k in sorted(keys):
        picked[k] = _get_path(config, k)
    return _sha([json.dumps(picked, sort_keys=True, separators=(",", ":"))])


def _get_path(config: dict, dotted: str):
    """`'risk.weights.test_ratio'` -> nested lookup; missing -> the sentinel `absent`.

    A key ending in `.` means "the whole subtree, canonically" (`layers.` = every layer
    rule). Without that rule a prefix key silently resolves to `absent` for every
    value of the subtree, and a config change that the analyzer *does* depend on
    invalidates nothing — the exact silent-miss failure §5.5 exists to prevent. That
    bug was caught by `no_change`/`config_layers` in `bench.py`, not by reading.
    """
    whole = dotted.endswith(".")
    cur = config
    for part in (dotted[:-1] if whole else dotted).split("."):
        if isinstance(cur, dict) and part in cur:
            cur = cur[part]
        else:
            return {"absent": dotted}
    if whole and not isinstance(cur, dict):
        return {"absent": dotted}
    return cur


@dataclass(frozen=True)
class Analyzer:
    """The manifest every analyzer registers (§5.1).

    `source_paths` is what the CI rule of §5.7 checks: touching a file matching one of
    those globs without bumping `version` fails the build, which is what turns "a code
    change in an analyzer invalidates exactly the right rows" from a hope into a gate.
    """

    id: str
    version: int
    scope_kind: str
    schema_version: int = 1
    config_keys: tuple[str, ...] = ()
    depends_on: tuple[str, ...] = ()
    source_paths: tuple[str, ...] = ()
    est_cost_ms: int = 5            # only used for progress reporting and budgeting (§7.6)

    def as_row(self) -> tuple:
        return (self.id, self.version, self.schema_version, self.scope_kind,
                json.dumps(list(self.config_keys)), json.dumps(list(self.depends_on)),
                json.dumps(list(self.source_paths)), 1)


#: The v1 analyzer set, named so that §6's matrix and `e2.x`/`e3.x` tickets cite the
#: same strings. `reco-f28` keeps this list in Rust; a mismatch is a bug in either.
ANALYZERS: tuple[Analyzer, ...] = (
    Analyzer("ingest.normalize", 1, "commit", config_keys=("ingest.size_cap_mib",),
             source_paths=("src/ingest/**",)),
    # e0.9's engine reads four config surfaces, so the digest covers all four (§5.5: the key
    # names everything the payload reads). Version 1 declared only the noise half, which was
    # honest while the analyzer was a stub; a change to the test-surface globs or to a class
    # assertion must now move these rows, which is why the version is 2.
    Analyzer("taxonomy.path", 2, "blob", config_keys=("noise.globs", "noise.builtins_version",
                                                      "test.globs", "taxonomy.classes."),
             source_paths=("src/analyze/taxonomy/**",)),
    Analyzer("diff.per_file", 1, "path_pair", config_keys=("diff.context_lines",),
             depends_on=("taxonomy.path",), source_paths=("src/analyze/diff/**",)),
    # Version 2: e2.4 (reco-a0v) moved rename pairing from "one threshold, git decides" to a tiered
    # engine whose output depends on twelve more keys. A change to any of them changes what a pairing
    # says, so §5.5's invalidation has to cover all of them — a cached fact under a 0.50 bar must not
    # survive a move to the measured 0.75.
    Analyzer("diff.rename_detect", 2, "commit_pair",
             config_keys=("rename.threshold", "rename.fact_bar", "rename.fact.allow_content",
                          "rename.guess_floor", "rename.candidates.top_k",
                          "rename.candidates.budget", "rename.candidates.copies_harder",
                          "rename.copies", "rename.folds", "rename.content.shingle_lines",
                          "rename.content.normalise", "rename.content.min_shared_shingles",
                          "rename.card.rows"),
             depends_on=("diff.per_file",), source_paths=("src/analyze/rename/**",)),
    Analyzer("episode.segment", 1, "commit_pair",
             config_keys=("episode.boundaries", "episode.time_gap_minutes"),
             depends_on=("ingest.normalize",), source_paths=("src/ingest/segment/**",)),
    Analyzer("structure.delta", 1, "tree", config_keys=("layers.", "noise.globs"),
             depends_on=("taxonomy.path", "diff.per_file"),
             source_paths=("src/analyze/structure/**",)),
    Analyzer("deps.resolve", 1, "tree", config_keys=("deps.ecosystems", "noise.globs"),
             depends_on=("taxonomy.path",), source_paths=("src/analyze/deps/**",)),
    Analyzer("non_code.surface", 1, "tree", config_keys=("non_code.patterns",),
             depends_on=("taxonomy.path", "diff.per_file"),
             source_paths=("src/analyze/noncode/**",)),
    Analyzer("test.surface", 1, "tree", config_keys=("test.globs",),
             depends_on=("taxonomy.path", "diff.per_file"),
             source_paths=("src/analyze/testsurface/**",)),
    Analyzer("graph.build", 1, "tree", config_keys=("layers.", "graph.ignored_dirs"),
             depends_on=("taxonomy.path",), source_paths=("src/analyze/graph/**",),
             est_cost_ms=45),
    Analyzer("graph.delta", 1, "tree", config_keys=("layers.",),
             depends_on=("graph.build",), source_paths=("src/analyze/graph/**",)),
    Analyzer("graph.cycles", 1, "tree", config_keys=("layers.", "ratchet."),
             depends_on=("graph.delta",), source_paths=("src/analyze/cycles/**",)),
    Analyzer("blast_radius", 1, "episode", config_keys=("blast.hops",),
             depends_on=("graph.delta",), source_paths=("src/analyze/blast/**",)),
    Analyzer("findings.detect", 1, "episode",
             config_keys=("detectors.enabled", "risk.weights"),
             depends_on=("structure.delta", "deps.resolve", "test.surface", "non_code.surface"),
             source_paths=("src/analyze/detectors/**",)),
    Analyzer("risk.score", 1, "episode", config_keys=("risk.weights", "risk.tau"),
             depends_on=("findings.detect", "blast_radius"),
             source_paths=("src/analyze/risk/**",)),
    Analyzer("delta.compose", 1, "episode", config_keys=(),
             depends_on=("structure.delta", "deps.resolve", "graph.delta", "test.surface",
                         "non_code.surface", "findings.detect", "risk.score"),
             source_paths=("src/analyze/delta/**",)),
)

ANALYZER_INDEX = {a.id: a for a in ANALYZERS}


def register_analyzers(conn, analyzers=ANALYZERS) -> None:
    """Write the manifest into the store (§5.1). Idempotent; versions are updated."""
    for a in analyzers:
        conn.execute(
            "INSERT INTO analyzer(id,version,schema_version,scope_kind,config_keys,"
            "depends_on,source_paths,enabled) VALUES(?,?,?,?,?,?,?,?) "
            "ON CONFLICT(id) DO UPDATE SET version=excluded.version,"
            "schema_version=excluded.schema_version, scope_kind=excluded.scope_kind,"
            "config_keys=excluded.config_keys, depends_on=excluded.depends_on,"
            "source_paths=excluded.source_paths", a.as_row())


# ---------------------------------------------------------------------------
# §5.4 dependency closure
# ---------------------------------------------------------------------------


def check_manifest(analyzers=ANALYZERS) -> list[str]:
    """Structural checks on the manifest: unknown deps, self-deps and cycles (§5.4)."""
    idx = {a.id: a for a in analyzers}
    problems: list[str] = []
    for a in analyzers:
        if a.scope_kind not in SCOPE_KINDS:
            problems.append(f"{a.id}: unknown scope kind {a.scope_kind!r}")
        for d in a.depends_on:
            if d not in idx:
                problems.append(f"{a.id}: depends on unknown analyzer {d!r}")
            if d == a.id:
                problems.append(f"{a.id}: depends on itself")
    # colour-sort cycle detection
    state = {a.id: 0 for a in analyzers}

    def visit(nid: str, stack: list[str]) -> None:
        if state.get(nid, 1) == 2:
            return
        if state.get(nid) == 1:
            problems.append("dependency cycle: " + " -> ".join(stack + [nid]))
            return
        state[nid] = 1
        for d in idx[nid].depends_on:
            if d in idx:
                visit(d, stack + [nid])
        state[nid] = 2

    for a in analyzers:
        visit(a.id, [])
    return problems


def dependency_closure(analyzer_id: str, analyzers=ANALYZERS) -> list[str]:
    """Transitive `depends_on` closure, sorted, excluding the analyzer itself."""
    idx = {a.id: a for a in analyzers}
    seen: set[str] = set()
    stack = list(idx[analyzer_id].depends_on)
    while stack:
        n = stack.pop()
        if n in seen or n not in idx:
            continue
        seen.add(n)
        stack.extend(idx[n].depends_on)
    seen.discard(analyzer_id)
    return sorted(seen)


def dep_digest(analyzer_id: str, versions: dict[str, int], analyzers=ANALYZERS) -> str:
    """Digest over the *transitive* dependency versions (§5.4).

    This is the whole cascade mechanism: bump `taxonomy.path` and every analyzer that
    (transitively) depends on it derives a new key, without one line of invalidation
    code and without knowing the bump happened.

    For the language-gated family the "version" is the derived one — bump plus feature
    digest (ADR-002 §10.2, ADR-004 §5.1) — so a feature change cascades here exactly as a
    hand-typed bump does.
    """
    pairs = [f"{aid}:{effective_version(aid, versions, analyzers)}"
             for aid in dependency_closure(analyzer_id, analyzers)]
    return _sha(pairs)


# ---------------------------------------------------------------------------
# §5.1/§5.2 — the version, and the part of it that comes from the build
# ---------------------------------------------------------------------------

#: The store's name for "the build never said what it was compiled with". Spelled out rather
#: than blank so that "not told" and "told: no languages" are different keys.
UNSPECIFIED_FEATURE_TAG = "build-features-unspecified"

#: The analyzers whose output a compiled-in `lang-*` feature can change (ADR-002 §6.4, §10.2).
#: The graph and structure family: their extractors are the gated ones. Everything else reads
#: git and text, which no cargo feature turns on or off.
FEATURE_SENSITIVE_PREFIXES = ("graph.", "structure.")


def feature_sensitive(analyzer_id: str) -> bool:
    """Does a compiled-in `lang-*` feature change what this analyzer produces?"""
    return analyzer_id.startswith(FEATURE_SENSITIVE_PREFIXES)


def effective_version(analyzer_id: str, versions: dict, analyzers=ANALYZERS) -> str:
    """The version as the key and the manifest print it (ADR-004 §5.1).

    `semver` for everything, plus `+<feature digest>` for the language-gated family — put in
    the version rather than into a tenth key field, because a framing change would make every
    stored row unreachable and a feature flag is not a framing change.

    `versions` may carry a string under `__feature_tag__` (the same slot convention as the
    integer `__store_schema__` override); an analyzer absent from the manifest takes version 0.
    """
    index = {a.id: a for a in analyzers}
    version = versions.get(analyzer_id, getattr(index.get(analyzer_id), "version", 0))
    if feature_sensitive(analyzer_id):
        return f"{version}+{versions.get('__feature_tag__', UNSPECIFIED_FEATURE_TAG)}"
    return str(version)


# ---------------------------------------------------------------------------
# §5.2 the key
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CacheKey:
    raw: str                       # 'ck1:<64 hex>'
    analyzer_id: str
    input_kind: str
    input_hash: str
    analyzer_version: int = 0
    dep_digest: str = ""
    config_digest: str = ""
    result_schema: int = 0

    def __str__(self) -> str:  # pragma: no cover - convenience
        return self.raw


def derive_key(*, analyzer_id: str, input_hash: str, input_kind: str | None = None,
               versions: dict[str, int] | None = None, config: dict | None = None,
               analyzers=ANALYZERS) -> CacheKey:
    """The canonical cache key: `ck1:sha256(framed(9 fields))` (§5.2).

    The fields, and why each one is load-bearing:

    1. ``ck1``              key-framing version — lets an old store's rows be ignored
                            after a framing change instead of misread as hits;
    2. ``analyzer_id``      which analyzer produced it;
    3. ``analyzer_version`` the bump a developer performs when the code changes (§5.7);
    4. ``dep_digest``       transitive dependency versions — the cascade (§5.4);
    5. ``config_digest``    only the config the analyzer declared (§5.5);
    6. ``schema_version``   the shape of the stored result (e2.11);
    7. ``input_kind``       so a `path` digest can never be mistaken for a `tree` digest;
    8. ``input_hash``       the scope digest itself (§5.3);
    9. ``code_version``     the *store* schema version: a schema change re-frames keys
                            so a v1 row can never answer a v2 query.
    """
    versions = versions or {}
    analyzers = analyzers
    idx = {a.id: a for a in analyzers}
    a = idx[analyzer_id]
    kind = input_kind or a.scope_kind
    parts = [
        KEY_SCHEMA,
        analyzer_id,
        effective_version(analyzer_id, versions, analyzers),
        dep_digest(analyzer_id, versions, analyzers),
        derive_config_digest(config or {}, list(a.config_keys)),
        str(a.schema_version),
        kind,
        input_hash,
        str(versions.get("__store_schema__", 1)),
    ]
    return CacheKey(raw=KEY_SCHEMA + ":" + _sha(parts), analyzer_id=analyzer_id,
                    input_kind=kind, input_hash=input_hash,
                    analyzer_version=int(versions.get(analyzer_id, a.version)),
                    dep_digest=parts[3], config_digest=parts[4],
                    result_schema=a.schema_version)


# ---------------------------------------------------------------------------
# §5.6 resolve: hit, miss, negative cache, LRU touch
# ---------------------------------------------------------------------------


@dataclass
class ResolveResult:
    key: CacheKey
    hit: bool
    result_sha: str | None = None
    error: str | None = None
    reason: str = "miss"
    bytes: int = 0
    stale: bool = False
    counters: dict[str, int] = field(default_factory=dict)


def resolve(conn, *, analyzer_id: str, input_hash: str, epoch: int,
            input_kind: str | None = None, versions: dict[str, int] | None = None,
            config: dict | None = None, now_ms: int | None = None,
            store_schema: int = 1) -> ResolveResult:
    """Look a cached result up, and account for the lookup (§5.6).

    Miss reasons are returned, not logged-and-forgotten: `fathom index --stats` prints
    them per analyzer, which is the difference between "the cache is slow" and "the
    epoch moved under us" as diagnosable states.
    """
    now_ms = now_ms if now_ms is not None else int(time.time() * 1000)
    key = derive_key(analyzer_id=analyzer_id, input_hash=input_hash, input_kind=input_kind,
                     versions={**(versions or {}), "__store_schema__": store_schema},
                     config=config)
    row = conn.execute("SELECT result_sha, error, epoch FROM cache WHERE cache_key=?",
                       (key.raw,)).fetchone()
    if row is None:
        # A same-input row under a different key is the *diagnostic* case: it proves the
        # miss came from a version/config bump, not from new input.
        twin = conn.execute(
            "SELECT cache_key FROM cache WHERE analyzer_id=? AND input_hash=? LIMIT 1",
            (analyzer_id, input_hash)).fetchone()
        return ResolveResult(key=key, hit=False,
                             reason="key_changed" if twin else "cold",
                             counters={"cache.miss": 1,
                                       "cache.miss.%s" % analyzer_id: 1,
                                       ("cache.bump.%s" % analyzer_id) if twin
                                       else ("cache.cold.%s" % analyzer_id): 1})
    if row["epoch"] != epoch:
        return ResolveResult(key=key, hit=False, reason="epoch_stale",
                             counters={"cache.miss": 1, "cache.epoch": 1})
    conn.execute("UPDATE cache SET last_used_utc=?, hits=hits+1 WHERE cache_key=?",
                 (now_ms, key.raw))
    if row["error"] is not None:
        return ResolveResult(key=key, hit=True, error=row["error"], reason="negative_hit",
                             counters={"cache.negative_hit": 1})
    blob = conn.execute("SELECT bytes FROM blob WHERE sha256=?", (row["result_sha"],)).fetchone()
    return ResolveResult(key=key, hit=True, result_sha=row["result_sha"], reason="hit",
                         bytes=blob["bytes"] if blob else 0,
                         counters={"cache.hit": 1, "cache.hit.%s" % analyzer_id: 1})


def put(conn, *, key: CacheKey, result_sha: str | None, epoch: int, cost_ms: int,
        analyzer: Analyzer, input_hash: str, result_bytes: int = 0,
         error: str | None = None, now_ms: int | None = None) -> None:
    """Write a cache row. `result_sha=None` with `error` set is a *negative* entry (§5.6):
    "this analyzer fails for this input" is an answer, and re-deriving it every re-index
    would make a broken analyzer expensive instead of merely visible."""
    now_ms = now_ms if now_ms is not None else int(time.time() * 1000)
    conn.execute(
        "INSERT INTO cache(cache_key,analyzer_id,input_kind,input_hash,analyzer_version,"
        "dep_digest,config_digest,result_sha,result_bytes,error,built_utc,last_used_utc,"
        "hits,cost_ms,epoch) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) "
        "ON CONFLICT(cache_key) DO UPDATE SET result_sha=excluded.result_sha,"
        "result_bytes=excluded.result_bytes,error=excluded.error,"
        "built_utc=excluded.built_utc,last_used_utc=excluded.last_used_utc",
        (key.raw, key.analyzer_id, key.input_kind, input_hash, key.analyzer_version,
         key.dep_digest, key.config_digest,
         result_sha, result_bytes, error, now_ms, now_ms, 0, cost_ms, epoch))


# ---------------------------------------------------------------------------
# §6: the matrix, as data the tests can assert against
# ---------------------------------------------------------------------------

#: §6's invalidation matrix, as machine-readable rows. `invalidates` names the *key
#: field* that changes — there is no "rows deleted" column on purpose, because nothing
#: on the event path deletes: dead rows are reaped by the pruner (§8.1).
INVALIDATION_MATRIX: tuple[tuple[str, str, str, str, str], ...] = (
    # cause, changed input, key field that moves, scope of recomputation, not touched
    ("new commit on HEAD", "tree oid, new commit oid",
     "input_hash", "analyzers scoped to the new tree/commit/path only",
     "every analysis of every unchanged tree and unchanged file"),
    ("working-tree edit, one file", "(path, blob) of that file",
     "input_hash", "per-file analyses whose scope digest names that blob; the provisional episode",
     "every other file's analyses, every committed tree's snapshots"),
    ("analyzer code change (version bump)", "that analyzer only",
     "analyzer_version", "that analyzer's rows, for every input it had cached",
     "every other analyzer, including its own dependents until they run again"),
    ("dependency analyzer bump", "transitive dep versions",
     "dep_digest", "each dependent analyzer, cascading up the DAG",
     "analyzers outside the closure; the inputs themselves"),
    ("fathom.toml noise globs changed", "config keys consuming them",
     "config_digest", "only analyzers whose `config_keys` include the changed key",
     "analyzers that never declared that key (e.g. `deps.ecosystems` untouched)"),
    ("risk weights changed", "risk.weights, risk.tau",
     "config_digest", "risk.score and delta.compose only",
     "structure, deps, graph: the *facts* do not change when the *ranking* does"),
    ("layer rules changed", "layers.*",
     "config_digest + dep_digest", "graph.build, graph.delta, graph.cycles, structure.delta",
     "deps.resolve, test.surface"),
    ("path taxonomy table change (e0.9)", "taxonomy.path version",
     "dep_digest", "everything downstream of taxonomy.path",
     "ingest.normalize, episode.segment (attribution does not read taxonomy)"),
    ("episode segmentation rule change", "episode.segment version",
     "analyzer_version", "episode identities, then every episode-scoped analyzer",
     "commit and commit_file rows: the git spine is segmentation-independent"),
    ("delta schema (EpisodeDelta) bump", "schema_version of delta.compose",
     "schema_version", "stored deltas and anything embedding them; findings re-derived",
     "the ledger: a schema bump never invalidates a human act"),
    ("SQL schema migration", "store schema version",
     "code_version", "every cache row becomes unreachable and is pruned; nothing is migrated (§9.1)",
     "class M mirrors: re-mirrored from the committed files, not migrated"),
    ("history rewrite (amend/rebase/squash)", "commit oids, episode identities",
     "n/a (identity relink)", "episode identities and episode-scoped results for the rewritten set",
     "ledger entries: they re-link by fingerprint (e4.6); the store is not the thing that breaks"),
    ("branch deleted / ref moved away", "reachable tree set",
     "n/a (reachability)", "nothing immediately; unreferenced rows age out under §8.2",
     "anything an unreviewed episode or a ledger entry still references"),
    ("hook event appended", "event seq range",
     "n/a (append-only)", "episode boundary for that turn, then episode-scoped analyzers",
     "commit-scoped analyses: an event cannot change what a commit contains"),
    ("events.jsonl truncated/rotated", "event_span.present",
     "n/a (guard)", "nothing: §8.3 *refuses* to prune a mirror whose source span is gone",
     "the mirror, which is precisely the point of the guard"),
    ("`fathom index --rebuild`", "index epoch",
     "epoch", "everything, into a new epoch file (§9.3)",
     "class M files: untouched by construction"),
    ("clock skew / backwards wall clock", "wall-clock ordering",
     "n/a", "nothing: ordering is by `seq` and `lineage_seq`, never by wall clock (§6.7)",
     "the timestamps themselves, which are reported with `created_source`"),
    ("kill -9 mid-index", "durable commit boundary",
     "n/a (WAL replay)", "the unfinished transaction vanishes; the run resumes from the last "
     "durable commit and recovery stamps the dead run `crashed` (§9.2)",
     "already-committed rows, and the ledger (written outside the store transaction)"),
)
