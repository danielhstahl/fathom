"""The shipped representation (`e2.4` / `reco-a0v`): a window, its candidate sets, and pairings.

The spike compared detectors and reported pairs. The product cannot, because `s4` measured that the
pair *shape* is where the truth fails: a 1-to-3 split, a fold into a file that already existed, and a
copy whose source stays put are found **zero times** at the fact bar by every method that emits pairs
(report §representation, gate `G51`) — not because the scores are wrong but because a 1:1 assignment
cannot name three destinations and git's diff format pairs each deletion with at most one addition.
So the type this package hands the UI is not a pair, it is a :class:`Pairing`: one change of shape,
with its sources, its destinations, one tier, and the evidence for every edge inside it.

Three vocabulary decisions, each of which is a measured finding and not a taste:

* **fact versus guess is a tier, not a threshold.** A fact is a pairing the reviewer can re-check with
  `git log --follow` — git's own rewriter at :attr:`Policy.fact_bar` — because that is the only number
  in this package that does not require our code to reproduce. Everything else is a guess and is
  labelled as one. A similarity score cannot be the fact boundary: the corpus's scaffolded decoy scores
  R064 in git and J≈0.49 while the window's real rewritten move scores R015 and J≈0.06 (gates
  `G29`, `G46`-`G48`), so *any* bar low enough to catch a real rewrite is high enough to ship the lie.
* **a pairing carries its evidence, per edge.** `J0.31` on a row is not a receipt; `content J0.31
  (12/29 3-line shingles)` is, and it is what makes a guess acceptable to a reviewer who disagrees
  with us — which is the whole point of accept/reject (:mod:`recommender.rename.ledger`).
* **a pairing binds to bytes, not to paths.** Two of `s4`'s windows show the same true pair arriving
  at completely different evidence after a history rewrite (a squash rebase takes R100 to R012; a
  conflict-resolved merge takes R100 to R056). A ledger keyed on paths alone would keep asserting a
  fact whose evidence is gone, so every :class:`Pairing` carries a :attr:`~Pairing.binding` digest and
  a review decision that does not match the binding is *re-opened*, not inherited.

Nothing in this module reads a file, runs a program, or looks at a clock: :mod:`recommender.rename.pair`
is pure, and :mod:`recommender.rename.gitio` is the half that touches a repository.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass, field

# -- pairing shapes ---------------------------------------------------------------------------

#: One source became one destination. The only shape git can print, and the only one it can print as
#: a fact.
KIND_RENAME = "rename"
#: One source became several destinations. A *split* is a fact about shape, never a fact about which
#: destination came from which region of the source — the blobs do not support that claim.
KIND_SPLIT = "split"
#: Several sources became one destination, which usually already existed (a fold into a module).
KIND_MERGE = "merge"
#: A destination that came from a source that is still there. Invisible to a detector restricted to
#: deleted paths, which is why the candidate set includes modified paths.
KIND_COPY = "copy"
#: Several sources and several destinations in one connected component: the matcher can say the files
#: are related and cannot say how. Printed as a tangle rather than forced into a shape.
KIND_TANGLE = "tangle"

KINDS: tuple[str, ...] = (KIND_RENAME, KIND_SPLIT, KIND_MERGE, KIND_COPY, KIND_TANGLE)

#: Shapes that are structurally out of reach of a 1:1 assignment, and therefore always guesses.
GROUP_KINDS: tuple[str, ...] = (KIND_SPLIT, KIND_MERGE, KIND_COPY, KIND_TANGLE)

#: The kind a reader sees, with the article included because the card prints it.
KIND_PHRASE: dict[str, str] = {
    KIND_RENAME: "rename",
    KIND_SPLIT: "split (1 file became several)",
    KIND_MERGE: "merge (several files became one)",
    KIND_COPY: "copy (the source is still there)",
    KIND_TANGLE: "tangle (related files, shape unresolved)",
}

# -- tiers --------------------------------------------------------------------------------------

TIER_FACT = "fact"
TIER_GUESS = "guess"

#: The words the card prints. Kept as constants so `render.audit` can demand them.
TIER_PHRASE: dict[str, str] = {TIER_FACT: "FACT", TIER_GUESS: "GUESS"}

# -- dispositions (accept / reject) -------------------------------------------------------------

#: Nobody has looked at this guess.
DISPOSITION_NONE = "unreviewed"
#: A reviewer agreed with the pairing. It is displayed as asserted, with who accepted it.
DISPOSITION_ACCEPTED = "accepted"
#: A reviewer refused it. It is not shown as a pairing, and it is *counted* where it was suppressed.
DISPOSITION_REJECTED = "rejected"
#: A decision existed for these paths but not for these bytes: the history moved under it.
DISPOSITION_REOPENED = "reopened"

DISPOSITIONS: tuple[str, ...] = (DISPOSITION_NONE, DISPOSITION_ACCEPTED, DISPOSITION_REJECTED,
                                 DISPOSITION_REOPENED)

#: Where the tier a pairing ends up displayed at comes from.
SOURCE_FACT = "git"
SOURCE_GUESS = "matcher"


def content_digest(data: bytes) -> str:
    """A short digest of one blob. Addressable, stable, and short enough to print on a row."""
    return hashlib.sha1(data).hexdigest()[:12]


def _digest(*parts: str) -> str:
    return hashlib.sha1("\x1f".join(parts).encode()).hexdigest()[:16]


# -- the window ---------------------------------------------------------------------------------

@dataclass(frozen=True)
class GitPair:
    """One rename/copy line git's own rewriter reported for the window."""

    src: str
    dst: str
    score: float                 # git's similarity index, 0..1
    kind: str = KIND_RENAME

    def label(self) -> str:
        return "git %s%03d" % ("R" if self.kind == KIND_RENAME else "C", round(self.score * 100))


@dataclass
class Window:
    """One review window: two revisions of one tree, with only as much of each as pairing needs.

    `base_paths` and `head_paths` are the *complete* path sets — a list of strings, so holding them is
    O(repo paths) in names and nothing else — because "is this source still there?" (the copy
    question) cannot be answered from the change set alone. Blob bytes are carried only for changed
    paths, which is what keeps the work O(changed files) instead of O(repo files).
    """

    id: str
    base: str
    head: str
    base_paths: set[str] = field(default_factory=set)
    head_paths: set[str] = field(default_factory=set)
    removed: dict[str, bytes] = field(default_factory=dict)
    added: dict[str, bytes] = field(default_factory=dict)
    modified: dict[str, tuple[bytes, bytes]] = field(default_factory=dict)
    git_pairs: list[GitPair] = field(default_factory=list)
    granularity: str = "episode"        # episode | commit_pair
    policy: str = "any"                 # any | first_parent | skip_merge
    note: str = ""
    reader: object = None               # optional callable(side, path) -> bytes | None

    def __post_init__(self) -> None:
        def as_bytes(value) -> bytes:
            # Callers write fixtures and manifests as text; the engine wants bytes. Coercing here
            # means a manifest that carries a source file as a string is a convenience rather than a
            # class of bug that only shows up on binary input.
            if isinstance(value, str):
                return value.encode()
            return bytes(value)

        self.removed = {path: as_bytes(data) for path, data in self.removed.items()}
        self.added = {path: as_bytes(data) for path, data in self.added.items()}
        self.modified = {path: (as_bytes(before), as_bytes(after))
                         for path, (before, after) in self.modified.items()}
        for label, mapping, side in (("removed", self.removed, self.base_paths),
                                     ("added", self.added, self.head_paths),
                                     ("modified", self.modified, self.head_paths)):
            for path in mapping:
                if side and path not in side:
                    raise ValueError("window %s: %s path %r is not in the %s tree"
                                     % (self.id, label, path, "base" if side is self.base_paths
                                        else "head"))

    # -- the candidate vocabulary ---------------------------------------------------------------
    @property
    def deletions(self) -> list[str]:
        return sorted(self.removed)

    @property
    def additions(self) -> list[str]:
        return sorted(self.added)

    @property
    def touches(self) -> int:
        """Changed paths — the cost denominator every bound in this package is stated against."""
        return len(self.removed) + len(self.added) + len(self.modified)

    def survives(self, path: str) -> bool:
        """The source of a *copy*: present in both trees, so nothing was deleted to pair with."""
        return path in self.base_paths and path in self.head_paths

    def untouched_base_paths(self) -> list[str]:
        """Paths in the base tree the window did not change — the widened copy search's sources.

        Named as its own method because it is the only place the engine is allowed to look at the tree
        rather than the change set, and only when the repository has paid for that mode.
        """
        return sorted(p for p in self.base_paths
                      if p not in self.removed and p not in self.modified)

    def base_blob(self, path: str) -> bytes | None:
        if path in self.removed:
            return self.removed[path]
        if path in self.modified:
            return self.modified[path][0]
        return self._read("base", path)

    def head_blob(self, path: str) -> bytes | None:
        if path in self.added:
            return self.added[path]
        if path in self.modified:
            return self.modified[path][1]
        return self._read("head", path)

    def _read(self, side: str, path: str) -> bytes | None:
        """Fall back to the injected reader for a path outside the change set.

        Nothing in the shipped path asks for one: only the opt-in widened copy search does, and that is
        exactly the O(repo) work the injection exists to make visible — every byte it reads goes
        through here, so the count of blobs read is the cost of the mode, not an estimate of it.
        """
        if self.reader is None:
            return None
        data = self.reader(side, path)                      # type: ignore[operator]
        return None if data is None else (data.encode() if isinstance(data, str) else bytes(data))

    def changed_blob(self, side: str, path: str) -> bytes:
        """The bytes the window itself carried, or nothing — never a read outside the change set.

        Used for the disposition binding: a binding must not be able to widen the cost, and computing
        one through `base_blob` on a widened window would read whatever the matcher had skipped.
        The blobs that reach a binding are the blobs the candidate sets already carried.
        """
        if side == "base":
            data = self.removed.get(path)
            return data if data is not None else self.modified.get(path, (b"", b""))[0]
        data = self.added.get(path)
        return data if data is not None else self.modified.get(path, (b"", b""))[1]

    def binding_for(self, kind: str, sources, destinations) -> str:
        """The bytes a reviewer's decision was about — the strong half of a pairing's identity.

        Paths alone would make a decision permanent the moment it was made: accept a rewrite today and
        next week's different bytes inherit the accept. So the binding digests the blob at every path on
        both sides, and a window that changed those bytes is a different claim (ADR-002 §4.4).
        """
        parts = [self.binding_scope(), kind]
        parts += ["base:%s:%s" % (path, content_digest(self.changed_blob("base", path)))
                  for path in sorted(sources)]
        parts += ["head:%s:%s" % (path, content_digest(self.changed_blob("head", path)))
                  for path in sorted(destinations)]
        return _digest(*parts)

    def key(self) -> str:
        return "%s:%s..%s" % (self.id, self.base[:12], self.head[:12])

    def binding_scope(self) -> str:
        """The window half of a disposition binding: which two revisions this pairing was read from."""
        return _digest("window", self.base, self.head)


# -- pairing ------------------------------------------------------------------------------------

@dataclass
class Edge:
    """One asserted source→destination relation, with the number it came from and its receipt."""

    src: str
    dst: str
    score: float
    source: str                     # SOURCE_FACT | SOURCE_GUESS
    method: str                     # "git" | "content"
    detail: str = ""                # the receipt a reviewer can check by hand
    name_similarity: float = 0.0    # display and ordering only; never part of `score`
    notes: list[str] = field(default_factory=list)

    @property
    def is_fact(self) -> bool:
        return self.source == SOURCE_FACT

    def label(self) -> str:
        if self.method == "git":
            return "git R%03d" % round(self.score * 100)
        return "content J%.2f" % self.score

    def render(self) -> str:
        head = "%s -> %s  %s" % (self.src, self.dst, self.label())
        if self.detail:
            head += " (%s)" % self.detail
        for note in self.notes:
            head += " — %s" % note
        return head


@dataclass
class Pairing:
    """One change of shape, at one tier, with every edge that supports it.

    A pairing is the unit the reviewer accepts or rejects, the unit a disposition binds to, and the
    unit a card row prints. Forcing a split through a rename-shaped type is what `e2.4`'s second
    acceptance criterion forbids, so the shape is a field and the renderer adapts to it.
    """

    window: str
    kind: str
    sources: tuple[str, ...]
    destinations: tuple[str, ...]
    tier: str
    confidence: float
    edges: list[Edge] = field(default_factory=list)
    evidence: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    binding: str = ""
    disposition: str = DISPOSITION_NONE
    disposition_note: str = ""
    reviewer: str = ""

    def __post_init__(self) -> None:
        if self.kind not in KINDS:
            raise ValueError("unknown pairing kind %r" % self.kind)
        if self.tier not in (TIER_FACT, TIER_GUESS):
            raise ValueError("unknown tier %r" % self.tier)
        if not self.sources or not self.destinations:
            raise ValueError("a pairing must name at least one source and one destination")
        if not self.binding:
            self.binding = self.compute_binding()
        if not self.evidence:
            self.evidence = [edge.render() for edge in self.edges]

    # -- identity -------------------------------------------------------------------------------
    def shape(self) -> str:
        return "%d->%d" % (len(self.sources), len(self.destinations))

    def compute_binding(self) -> str:
        """The fallback binding: paths, shape and window only.

        The engine does **not** use this — `pairing_binding` below digests the bytes too, which is the
        whole point of a binding. This exists for a `Pairing` assembled without its window at hand, and
        it is deliberately weaker: a paths-only binding makes a decision permanent the moment it is
        made, so a pairing whose content later moved would keep its old accept forever.
        """
        return _digest(self.window, self.kind, *self.sources, *self.destinations)

    def key(self) -> str:
        """The disposition lookup key: shape and paths, no bytes.

        Lookup is by path so a rewritten history can be *recognised* as the same claim with different
        evidence and re-opened; the binding then decides whether the old decision still applies.
        """
        return _digest(self.window, self.kind, *self.sources, *self.destinations)

    # -- questions ------------------------------------------------------------------------------
    @property
    def is_group(self) -> bool:
        return self.kind in GROUP_KINDS or len(self.edges) > 1

    @property
    def mixed(self) -> bool:
        """Some edge inside it is a fact and some is not — the split-with-a-dominant-part case."""
        return any(edge.is_fact for edge in self.edges) and any(not e.is_fact for e in self.edges)

    @property
    def asserted(self) -> bool:
        """Whether the card may show this as asserted: a fact, or a guess a reviewer accepted."""
        return self.tier == TIER_FACT or self.disposition == DISPOSITION_ACCEPTED

    def headline(self) -> str:
        if self.kind in (KIND_RENAME,):
            return "%s -> %s" % (self.sources[0], self.destinations[0])
        if self.kind == KIND_SPLIT:
            return "%s -> %s (+%d more)" % (self.sources[0], self.destinations[0],
                                            len(self.destinations) - 1)
        if self.kind == KIND_MERGE:
            return "%s (+%d more) -> %s" % (self.sources[0], len(self.sources) - 1,
                                            self.destinations[0])
        if self.kind == KIND_COPY:
            return "%s -> %s [source kept]" % (self.sources[0], self.destinations[0])
        return "%s ... %s (%d x %d)" % (self.sources[0], self.destinations[-1],
                                        len(self.sources), len(self.destinations))

    def why(self) -> list[str]:
        """Every reason this pairing is at its tier, and what a reviewer can run to check it."""
        out = list(self.evidence)
        for note in self.notes:
            out.append("note: %s" % note)
        if self.tier == TIER_FACT:
            out.append("fact: git's own rewriter reports this at or above the fact bar "
                       "(check with `git log --follow -- %s`)" % self.destinations[0])
        else:
            out.append("guess: not reported by git at the fact bar, so the only evidence is ours — "
                       "accept it and the card shows it as asserted")
        return out

    def as_dict(self) -> dict:
        return {"window": self.window, "kind": self.kind, "shape": self.shape(),
                "tier": self.tier, "asserted": self.asserted,
                "confidence": round(self.confidence, 4),
                "sources": list(self.sources), "destinations": list(self.destinations),
                "evidence": list(self.evidence), "notes": list(self.notes),
                "mixed": self.mixed, "binding": self.binding, "key": self.key(),
                "disposition": self.disposition, "disposition_note": self.disposition_note,
                "reviewer": self.reviewer,
                "edges": [{"src": e.src, "dst": e.dst, "score": round(e.score, 4),
                           "source": e.source, "method": e.method, "detail": e.detail,
                           "notes": list(e.notes),
                           "name_similarity": round(e.name_similarity, 4)} for e in self.edges]}

    # -- ordering -------------------------------------------------------------------------------
    def sort_key(self) -> tuple:
        """Facts first, then by confidence. Deterministic: every tie-break is a printed field."""
        return (0 if self.tier == TIER_FACT else 1,
                0 if self.disposition == DISPOSITION_REJECTED else 1,
                -self.confidence, self.kind, self.sources[0], self.destinations[0])


@dataclass
class Result:
    """Everything one window produced: the pairings, the cost, and what could not be seen."""

    window: Window
    pairings: list[Pairing] = field(default_factory=list)
    stats: dict = field(default_factory=dict)
    limits: list[str] = field(default_factory=list)

    @property
    def facts(self) -> list[Pairing]:
        return [p for p in self.pairings if p.tier == TIER_FACT and p.disposition !=
                DISPOSITION_REJECTED]

    @property
    def guesses(self) -> list[Pairing]:
        return [p for p in self.pairings if p.tier == TIER_GUESS and p.disposition !=
                DISPOSITION_REJECTED]

    @property
    def suppressed(self) -> list[Pairing]:
        return [p for p in self.pairings if p.disposition == DISPOSITION_REJECTED]

    @property
    def reopened(self) -> list[Pairing]:
        return [p for p in self.pairings if p.disposition == DISPOSITION_REOPENED]

    def counts(self) -> dict:
        out = {"pairings": len(self.pairings), "facts": len(self.facts),
               "guesses": len(self.guesses), "suppressed": len(self.suppressed),
               "reopened": len(self.reopened)}
        for kind in KINDS:
            out[kind] = sum(1 for p in self.pairings if p.kind == kind)
        out.update(self.stats)
        return out

    def as_dict(self) -> dict:
        return {"window": {"id": self.window.id, "base": self.window.base,
                           "head": self.window.head, "granularity": self.window.granularity,
                           "policy": self.window.policy},
                "counts": self.counts(), "limits": list(self.limits),
                "pairings": [p.as_dict() for p in self.pairings]}
