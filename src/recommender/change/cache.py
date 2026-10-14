"""Per-blob caching: the key, the framing, and an in-memory store with canonical encoding.

AC 4 is "classification is cached per blob and stable across runs", and the two halves are
one claim: a cache key that names everything the answer depends on is what makes the answer
reproducible, and a reproducible answer is what makes caching safe. So:

* **The scope is a blob, keyed with its path.** `ADR-004 §5.1` gives `taxonomy.path` the
  `blob` scope, and `store/git.py` scopes it as *(path, content)* for the reason spelled out
  there: a classification is a function of the path *and* the bytes, so a key naming only
  the content would let two paths sharing one blob share one wrong answer. Same rule here.
* **The key is ADR-004's, not a new one.** `recommender.store.cache.derive_key` frames it
  (`ck1:sha256(…)`), with a local `Analyzer` manifest entry — `change.classify`, version
  `CACHE_VERSION`, scope `blob`, `depends_on=("taxonomy.path",)`, and the four config keys the
  taxonomy reads — so the invalidation cascade of §5.4/§6 applies to this analyzer like any
  other, and `fathom index --stats` can count its hits.
* **The taxonomy's table digest is inside the input hash**, not only in `dep_digest`.
  `dep_digest` covers a version *bump*; the digest covers a table edit that forgot one.
  Belt and braces, because a stale classification is invisible: the label looks fine and a
  reviewer never sees the file that was mislabelled.
* **Nothing time-dependent is in the payload.** No timestamp, no `id()`, no dict-ordering
  dependence — `encode()` is `json.dumps(…, sort_keys=True)` over plain data, and the
  contract test runs the CLI twice in two processes and diffs the bytes.

The cache object itself is deliberately small: a dict, hit/miss counters, and `encode` /
`decode`. Persisting it is the caller's business (`cli.py` writes the file), so that nothing
in the analysis half of this package touches the filesystem.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field

from recommender.store.cache import (ANALYZERS, Analyzer, derive_key)

from . import markers as M

#: Version of the classifier's cached payload. Bump it with any change to the marker table,
#: the policy weights or the payload shape — the same discipline ADR-004 §5.7 asks of every
#: analyzer, and `tests/test_change_classifier_contract.py` checks the key moves when the
#: table does.
CACHE_VERSION = 1

#: The config keys whose values can change a classification. Exactly what the taxonomy engine
#: reads (§5.5: a key names everything the payload reads, and nothing it does not).
CONFIG_KEYS = ("noise.globs", "noise.builtins_version", "test.globs", "taxonomy.classes.")

CLASSIFIER_ANALYZER = Analyzer(
    "change.classify", CACHE_VERSION, "blob", config_keys=CONFIG_KEYS,
    depends_on=("taxonomy.path",), source_paths=("src/recommender/change/**",),
    est_cost_ms=1)

_ANALYZERS = ANALYZERS + (CLASSIFIER_ANALYZER,)

#: The analyzer set `derive_key` must be given to resolve this analyzer's dependency closure.
ANALYZER_SET = _ANALYZERS


def blob_oid(data: bytes | str) -> str:
    """git's own blob id for these bytes: `sha1("blob N\\0" + bytes)`.

    Not a new hash: the store keys blobs by this value (`ADR-004 §4.3`), so a classifier key
    that quotes a blob id is legible against the store's rows, and two tools that disagree
    about a blob's identity is a bug nobody wants to find.
    """
    if isinstance(data, str):
        data = data.encode("utf-8", "replace")
    digest = hashlib.sha1()
    digest.update(b"blob %d\x00" % len(data))
    digest.update(data)
    return digest.hexdigest()


def input_hash(*, path: str, oid: str, taxonomy_digest: str, taxonomy_version: int,
               markers_digest: str, classifier_version: int) -> str:
    """The scope digest: path + blob + the two tables the answer was read from.

    Length-prefixed framing, the same as `store.cache._frame`, so `"a" + "b.c"` and
    `"a.b" + "c"` cannot collide.
    """
    parts = [path, oid, "tax:%d:%s" % (taxonomy_version, taxonomy_digest),
             "mark:%d:%s" % (M.MARKERS_VERSION, markers_digest),
             "clf:%d" % classifier_version]
    out = bytearray()
    for part in parts:
        raw = part.encode("utf-8", "replace")
        out += len(raw).to_bytes(4, "big")
        out += raw
    return hashlib.sha256(bytes(out)).hexdigest()[:40]


def classification_key(*, path: str, oid: str, taxonomy, config: dict | None = None,
                       markers_digest: str | None = None):
    """The `ck1:…` key for one (path, blob) under one effective table."""
    digest = M.digest() if markers_digest is None else markers_digest
    ih = input_hash(path=path, oid=oid, taxonomy_digest=taxonomy.digest,
                    taxonomy_version=taxonomy.version, markers_digest=digest,
                    classifier_version=CACHE_VERSION)
    return derive_key(analyzer_id=CLASSIFIER_ANALYZER.id, input_hash=ih, input_kind="blob",
                      config=config or {}, analyzers=_ANALYZERS)


@dataclass
class BlobCache:
    """An in-memory classification cache, keyed by `ck1:…`.

    Bounded, because a long-lived daemon's RSS must not track repository size
    (ADR-004 §11.5). The eviction policy is "clear it": a classification is cheap to recompute
    (microseconds), so a cache this small does not need an LRU, and a half-evicted table is
    harder to reason about than an empty one.
    """

    limit: int = 65_536
    entries: dict[str, dict] = field(default_factory=dict)
    hits: int = 0
    misses: int = 0

    def get(self, key: str) -> dict | None:
        entry = self.entries.get(key)
        if entry is None:
            self.misses += 1
        else:
            self.hits += 1
        return entry

    def put(self, key: str, payload: dict) -> None:
        if len(self.entries) >= self.limit:
            self.entries.clear()
        self.entries[key] = payload

    def hit_rate(self) -> float:
        total = self.hits + self.misses
        return (self.hits / total) if total else 0.0

    def stats(self) -> dict:
        return {"entries": len(self.entries), "hits": self.hits, "misses": self.misses,
                "hit_rate": round(self.hit_rate(), 4), "limit": self.limit,
                "analyzer": CLASSIFIER_ANALYZER.id, "version": CACHE_VERSION}

    def clear(self) -> None:
        self.entries.clear()
        self.hits = self.misses = 0

    # -- canonical bytes (persistence is the caller's job) -----------------
    def encode(self) -> bytes:
        """Canonical JSON: sorted keys, compact separators, no clock, no identity.

        Byte-stable across runs and processes, which is the form AC 4 is checked in.
        """
        return json.dumps({"analyzer": CLASSIFIER_ANALYZER.id, "version": CACHE_VERSION,
                           "entries": {key: value for key, value in self.entries.items()}},
                          sort_keys=True, separators=(",", ":")).encode("utf-8")

    @classmethod
    def decode(cls, data: bytes | str) -> "BlobCache":
        text = data.decode("utf-8") if isinstance(data, bytes) else data
        parsed = json.loads(text)
        if parsed.get("analyzer") != CLASSIFIER_ANALYZER.id:
            raise ValueError("not a change-classifier cache")
        cache = cls()
        cache.entries = dict(parsed.get("entries") or {})
        return cache
