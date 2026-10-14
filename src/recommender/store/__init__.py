"""ADR-004 reference implementation: the storage, indexing and cache contract.

This package is a *contract fixture*, in the sense `src/recommender/harness/` is one
(ADR-005 §11.5): it exists to make the clauses of [ADR-004](../../../../docs/adr/0004-storage.md)
checkable rather than aspirational. It is stdlib-only and it is not the Rust store
(`fathom-store`, ADR-002 `reco-f28`); the DDL in `schema.py` and the key derivation in
`cache.py` are the parts that are normative.

Four of the modules are load-bearing for the ADR's clauses: `schema.py` (DDL, pragmas,
durability classes, the content digest), `cache.py` (the manifest and key derivation),
`git.py` (the deterministic fixture and a miniature index, so recovery is testable) and
`retention.py` (§8's windows and §9's runbook, as functions). `bench.py` produces
`benchmarks/reports/adr004-baseline.json`, which is where §13's numbers come from.
"""

from .cache import (
    ANALYZERS,
    Analyzer,
    CacheKey,
    derive_config_digest,
    derive_key,
    resolve,
)
from .retention import (DEFAULT_RETENTION, class_m_blobs, diagnose, doctor, prune,
                        put_capped, quarantine, repair_fts)
from .schema import (
    DURABILITY,
    PRAGMAS_WRITER,
    PRAGMAS_READER,
    SCHEMA_VERSION,
    STORE_LAYOUT,
    SqliteStore,
    check_compile_options,
    connect,
    create_store,
)

__all__ = [
    "ANALYZERS",
    "Analyzer",
    "CacheKey",
    "DURABILITY",
    "PRAGMAS_READER",
    "PRAGMAS_WRITER",
    "SCHEMA_VERSION",
    "STORE_LAYOUT",
    "SqliteStore",
    "check_compile_options",
    "connect",
    "create_store",
    "class_m_blobs",
    "DEFAULT_RETENTION",
    "diagnose",
    "doctor",
    "prune",
    "put_capped",
    "quarantine",
    "repair_fts",
    "derive_config_digest",
    "derive_key",
    "resolve",
]
