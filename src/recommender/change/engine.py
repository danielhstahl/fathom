"""The classifier: one episode's changed paths in, one labelled decision per path out.

`recommender.taxonomy` answers "what does the path say"; this engine adds "what do the bytes
say" and decides between them with the ladder in `policy.py`. The unit of work is a
:class:`Change` — one path in one episode, with whatever content the caller has — and the
unit of output is a :class:`Classification`: a class, a noise level, a confidence, a band,
and the sentence that says why. Everything downstream (`e2.2`'s noise suppression,
`e2.3`'s folder delta, `e2.8`'s non-code surfaces, `e2.9`'s test surface) reads this object
rather than re-deriving a label, which is the whole reason `e0.9` exists and this ticket
extends it.

Two invariants the rest of the file protects:

* **The answer is a function of (path, blob, table, marker table, config).** Nothing else:
  no clock, no filesystem, no iteration-order dependence. That is what makes `cache.py`'s key
  honest and AC 4 ("stable across runs") a test rather than a hope.
* **A decision without receipts is a bug.** `reason`, `votes`, `scores` and `facts` are all
  populated on every path through the code, including the cache-hit path, where they are
  replayed from the cached payload — which is why the payload carries them and the cache key
  covers the tables that produced them.
"""

from __future__ import annotations

import threading
from dataclasses import dataclass, field

from recommender.taxonomy import Taxonomy, classes as C, default_taxonomy
from recommender.taxonomy.classes import default_noise

from . import markers as M
from . import policy as P
from .cache import CACHE_VERSION, BlobCache, classification_key, blob_oid
from .evidence import BuildIndex, FileFacts, inspect_bytes, references_for, votes_from

#: Version of the classifier's own behaviour (policy weights, evidence rules, payload shape).
#: `cache.CACHE_VERSION` is the durable form of the same number; the two must move together
#: and the contract test asserts they are equal.
CLASSIFIER_VERSION = 1

CHANGE_TYPES = ("added", "modified", "deleted", "renamed", "tracked", "untracked")


@dataclass(frozen=True)
class Change:
    """One changed path in an episode, with the content the caller happens to have.

    `content=None` is a legitimate state, not a failure: a deleted file has no new blob, and
    a cold index may classify a path before its blob has been streamed. The answer then comes
    from the path alone and says so in `facts.notes`.
    """

    path: str
    content: bytes | str | None = None
    change_type: str = "modified"
    old_path: str = ""

    @property
    def subject(self) -> str:
        """The path the decision is about — the old one for a deletion, the new otherwise."""
        return self.path or self.old_path

    def oid(self) -> str:
        """The blob id the cache key uses: the caller's, or git's, or `absent`."""
        if self.content is None:
            return "absent"
        return blob_oid(self.content)


@dataclass
class Classification:
    """One decision, with its receipts. This is what the UI, the store and every detector read."""

    path: str
    cls: str
    noise: str
    confidence: float
    band: str
    reason: str
    change_type: str = "modified"
    oid: str = ""
    ambiguous: bool = False
    overrode_path: bool = False
    path_class: str = ""
    path_rule: str = ""
    path_layer: str = ""
    noise_rule: str = ""
    winner: dict | None = None
    scores: list = field(default_factory=list)
    votes: list = field(default_factory=list)
    notes: list = field(default_factory=list)
    facts: dict = field(default_factory=dict)
    taxonomy_version: int = 0
    table_digest: str = ""
    classifier_version: int = CLASSIFIER_VERSION
    markers_version: int = M.MARKERS_VERSION
    cache_key: str = ""
    cached: bool = False

    # -- conveniences every consumer wants ---------------------------------
    @property
    def is_noise(self) -> bool:
        return self.noise != "informative"

    @property
    def diffable(self) -> bool:
        return C.is_diffable(self.cls)

    @property
    def reviewable(self) -> bool:
        return C.is_reviewable(self.cls)

    @property
    def noise_score(self) -> float:
        from recommender.taxonomy import score_of
        return score_of(self.noise)

    def badge(self, width: int = 22) -> str:
        """The tree-pane label: class plus confidence, clipped to a column width.

        ADR-006 §6.2 groups the diff tree by taxonomy class; this is the string that group
        header carries. `width` is the pane's minimum, so the badge is usable at 80x24.
        """
        text = "%s %.2f" % (self.cls, self.confidence)
        return text if len(text) <= width else text[:max(1, width - 1)] + "…"

    def detail(self) -> str:
        """The pane/detail line a reviewer reads before deciding to open the file."""
        return self.reason

    def as_dict(self) -> dict:
        return {"ambiguous": self.ambiguous, "band": self.band, "cache_key": self.cache_key,
                "change_type": self.change_type, "classifier_version": self.classifier_version,
                "cls": self.cls, "confidence": round(self.confidence, 2), "diffable": self.diffable,
                "facts": self.facts, "markers_version": self.markers_version, "noise": self.noise,
                "noise_rule": self.noise_rule, "noise_score": round(self.noise_score, 2),
                "notes": list(self.notes), "oid": self.oid,
                "overrode_path": self.overrode_path, "path": self.path, "path_class": self.path_class,
                "path_layer": self.path_layer, "path_rule": self.path_rule, "reason": self.reason,
                "reviewable": self.reviewable, "scores": [list(s) for s in self.scores],
                "table_digest": self.table_digest, "taxonomy_version": self.taxonomy_version,
                "votes": list(self.votes), "winner": self.winner}

    @classmethod
    def from_dict(cls, payload: dict, *, cached: bool = True) -> "Classification":
        """Rebuild a decision from a cached payload (or from the store's JSON)."""
        known = {f for f in ("path", "cls", "noise", "confidence", "band", "reason",
                             "change_type", "oid", "ambiguous", "overrode_path", "path_class",
                             "path_rule", "path_layer", "noise_rule", "winner", "scores",
                             "votes", "notes", "facts", "taxonomy_version", "table_digest",
                             "classifier_version", "markers_version", "cache_key")}
        return cls(**{k: v for k, v in payload.items() if k in known}, cached=cached)


# ---------------------------------------------------------------------------
# the classifier
# ---------------------------------------------------------------------------
class ChangeClassifier:
    """Path table + content evidence, for one repository's resolved configuration.

    Built once per index run, called per changed path, safe to share: the taxonomy is
    immutable, the cache is lock-guarded, and no method mutates the classifier.
    """

    def __init__(self, taxonomy: Taxonomy | None = None, *, config=None,
                 cache: BlobCache | None = None, build_index: BuildIndex | None = None,
                 use_content: bool = True, enable_cache: bool = True):
        self.taxonomy = taxonomy or default_taxonomy()
        self.config = config
        self.config_values = getattr(config, "values", config) or {}
        self.cache = cache if cache is not None else BlobCache()
        self.build_index = build_index
        self.use_content = use_content
        self.enable_cache = enable_cache
        self.markers_digest = M.digest()
        self._lock = threading.Lock()
        self.counters = {"classify": 0, "cache_hit": 0, "cache_miss": 0, "content_used": 0,
                         "overridden": 0, "ambiguous": 0, "other": 0}

    # -- construction ------------------------------------------------------
    @classmethod
    def default(cls) -> "ChangeClassifier":
        """Built-in taxonomy, built-in markers, no configuration, caching on."""
        return cls(default_taxonomy())

    @classmethod
    def for_taxonomy(cls, taxonomy: Taxonomy, *, config=None, **kwargs) -> "ChangeClassifier":
        return cls(taxonomy, config=config, **kwargs)

    # -- the question ------------------------------------------------------
    def classify(self, change: Change) -> Classification:
        path = change.subject
        if not path:
            return Classification(path="", cls=C.PathClass.OTHER, noise="informative",
                                  confidence=0.0, band=P.BAND_AMBIGUOUS,
                                  reason="nothing to classify: the change names no path")
        self.counters["classify"] += 1

        key = None
        if self.enable_cache:
            key = classification_key(path=path, oid=change.oid(), taxonomy=self.taxonomy,
                                     config=self.config_values,
                                     markers_digest=self.markers_digest)
            hit = self.cache.get(key.raw)
            if hit is not None:
                self.counters["cache_hit"] += 1
                return Classification.from_dict(hit, cached=True)
            self.counters["cache_miss"] += 1

        decision = self._compute(change, path)
        decision.cache_key = key.raw if key is not None else ""
        if key is not None:
            self.cache.put(key.raw, decision.as_dict())
        return decision

    def classify_path(self, path: str, content=None, **kwargs) -> Classification:
        return self.classify(Change(path=path, content=content, **kwargs))

    def classify_files(self, files: dict[str, str | bytes], **kwargs) -> list[Classification]:
        """Classify a `{path: content}` map — the shape an episode's file set already is."""
        return [self.classify(Change(path=path, content=content, **kwargs))
                for path, content in sorted(files.items())]

    # -- internals ---------------------------------------------------------
    def _compute(self, change: Change, path: str) -> Classification:
        verdict = self.taxonomy.classify(path)
        facts: FileFacts | None = None
        content_votes: list[P.Vote] = []
        if self.use_content and change.content is not None:
            facts = inspect_bytes(path, change.content)
            content_votes = P.refine_not_text(
                votes_from(facts, path=path, build_refs=references_for(path, self.build_index)),
                verdict.cls)
            self.counters["content_used"] += 1
        elif self.build_index is not None:
            refs = references_for(path, self.build_index)
            if refs:
                content_votes = votes_from(FileFacts(path=path, available=False),
                                           path=path, build_refs=refs)

        path_vote = P.path_weight(verdict)
        votes = ([path_vote] if path_vote is not None else []) + content_votes
        result = P.combine(votes, path_class=verdict.cls)

        noise, noise_rule = self._noise(verdict, result)
        decision = Classification(
            path=path, cls=result.cls, noise=noise, confidence=result.confidence,
            band=result.band, reason="", change_type=change.change_type, oid=change.oid(),
            ambiguous=result.ambiguous, overrode_path=result.overrode_path,
            path_class=verdict.cls, path_rule=verdict.rule_id, path_layer=verdict.layer,
            noise_rule=noise_rule, winner=result.winner.as_dict() if result.winner else None,
            scores=[[cls, round(score, 3)] for cls, score in result.scores],
            votes=[vote.as_dict() for vote in result.votes],
            notes=list(result.notes) + (facts.notes if facts else []),
            facts=facts.as_dict() if facts is not None else {"available": False},
            taxonomy_version=verdict.taxonomy_version, table_digest=verdict.table_digest)
        decision.reason = self.reason(decision, result, verdict)
        if result.overrode_path:
            self.counters["overridden"] += 1
        if result.ambiguous:
            self.counters["ambiguous"] += 1
        if result.cls == C.PathClass.OTHER:
            self.counters["other"] += 1
        return decision

    def _noise(self, verdict, result: P.Result) -> tuple[str, str]:
        """Which noise level travels with this class claim, and who said so.

        When the class the content won is the class the path won, `e0.9`'s noise answer
        travels unchanged — including a `noise.globs` demotion or a `!` re-promotion, which
        are the user's own words about this path. When content *moved* the class, the class's
        default noise follows it: a protoc stub is `generated` noise whether or not the path
        happened to say `source`. A `reformat` (which no path rule may claim,
        `taxonomy/noise.py`) is never invented here either — that is `e2.2`'s content claim
        about the *diff*, not this one about the file.
        """
        if result.cls == verdict.cls:
            return verdict.noise, (verdict.noise_match.rule_id if verdict.noise_match else "")
        return default_noise(result.cls), "class:%s" % result.cls

    def reason(self, decision: Classification, result: P.Result, verdict) -> str:
        """The one-sentence answer AC 2 asks for: what, how strongly, and on what evidence."""
        parts: list[str] = []
        winner = result.winner
        explained = False
        if winner is None:
            # `combine` already wrote the sentence for this case — which of the three ways a
            # class can fail to win it is, and what the competing claims scored.
            parts.extend(result.notes)
            explained = True
            if verdict.matched and result.cls != verdict.cls:
                parts.append("the path rule `%s` said `%s`" % (verdict.rule_id, verdict.cls))
        elif winner.kind in (P.KIND_PATH, P.KIND_ASSERTION):
            parts.append(winner.note)
        else:
            parts.append("content evidence — %s" % winner.note)
            if verdict.matched:
                parts.append("the path rule `%s` says `%s`%s"
                             % (verdict.rule_id, verdict.cls,
                                " and the content overrules it" if result.overrode_path
                                else ", which the content agrees with"))
            else:
                parts.append("no path rule matched this path, so the alternative was `other`")
        if winner is not None and winner.kind in (P.KIND_PATH, P.KIND_ASSERTION) \
                and any(v.kind == "content" for v in result.votes):
            extra = sum(1 for v in result.votes if v.kind == "content")
            parts.append("%d content %s corrobor%s" % (extra, "claim" if extra == 1 else "claims",
                                                       "ates" if extra == 1 else "e"))
        if not explained:
            parts.extend(result.notes)
        return "; ".join(part for part in parts if part)

    # -- reporting ---------------------------------------------------------
    def stats(self) -> dict:
        return {"classifier_version": CLASSIFIER_VERSION, "cache_version": CACHE_VERSION,
                "taxonomy_version": self.taxonomy.version, "table_digest": self.taxonomy.digest,
                "markers_version": M.MARKERS_VERSION, "markers_digest": self.markers_digest,
                "counters": dict(self.counters), "cache": self.cache.stats(),
                "use_content": self.use_content}


_DEFAULT: ChangeClassifier | None = None
_DEFAULT_LOCK = threading.Lock()


def default_classifier() -> ChangeClassifier:
    """Process-wide built-ins-only classifier (tests and fixtures; a repo passes its own)."""
    global _DEFAULT
    if _DEFAULT is None:
        with _DEFAULT_LOCK:
            if _DEFAULT is None:
                _DEFAULT = ChangeClassifier.default()
    return _DEFAULT


def reset_default_classifier() -> None:
    """Drop the shared instance. Tests only."""
    global _DEFAULT
    _DEFAULT = None
