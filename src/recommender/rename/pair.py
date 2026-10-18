"""The pairing engine (`e2.4` / `reco-a0v`): candidates, similarity, shape, tier.

One window goes in, a list of :class:`~recommender.rename.model.Pairing` comes out. The whole design
is an attempt to keep two promises the ticket makes and `s4` measured:

**Never assert what you can only guess.** Facts come from git's own rewriter at `rename.fact_bar`, and
only for a 1:1 shape: git's percentage is the only number in this package a reviewer can reproduce
without our code, and it is the only tier a reviewer is asked to act on. A grouping — a split, a fold,
a copy — is always a guess, because `s4` found those three classes **zero times** at the fact bar with
every method that emits pairs (`G51`): the shape, not the score, is what a 1:1 pairing cannot say.

**Never cost more than the window.** Every candidate set is window-local: deletions, additions, and
optionally modified paths. Nothing here walks the tree, so the work is bounded by changed files —
which is the only way the fourth acceptance criterion can be met at all, and the reason a copy of an
untouched file is a *stated blindness* rather than a feature.

The pipeline, in order:

1. **candidate sets** (:func:`candidate_sets`) — deletions and modified paths as sources, additions and
   modified paths as destinations. Both `rename.copies` and `rename.folds` exist because the naive
   version (deletions × additions) is copy-blind and fold-blind *by construction*, which `s4`
   measured as a recall of zero rather than a small recall.
2. **git's edges** (:func:`git_edges`) — read at `Policy.git_candidate_bar()`, i.e. below the fact bar,
   so a guess can carry git's own number. A git pair whose source or destination is claimed twice at
   fact strength is demoted: two facts about one blob is a contradiction, not a stronger claim.
3. **content edges** (:func:`content_edges`) — exact Jaccard over hashed `n`-line shingles, generated
   through an inverted index rather than a full cross product. Three deliberate parts:
   * a **length filter** (`J ≤ min/max` of the two shingle-set sizes) drops pairs that cannot reach the
     floor before any set work happens;
   * a **boilerplate cap** ignores a shingle that appears in too many destinations. That is the
     scaffold-decoy finding turned into a rule: the corpus's false pair is two handlers sharing a
     template, and a template shared by many files is not evidence of provenance;
   * **exact Jaccard, not the sketch**, for every surviving pair: `s4` measured 29 pairs landing on
     opposite sides of a 0.30 bar depending on which of the two you believe, so an approximation may
     prune candidates and may not decide a threshold.
4. **top-k pruning** (:func:`prune`) — k best per source *or* k best per destination, `s4`'s rule, the
   thing that keeps three split destinations alive instead of the assignment's one.
5. **grouping** (:func:`group`) — connected components become pairings, with a shape, a tier, and the
   reviewer's disposition applied if one exists.

Like :mod:`recommender.rename.model`, this module is pure: no files, no `git`, no clock.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field

from . import model as M
from .detect import name_similarity
from .policy import Policy
from .shingle import exact_jaccard, normalise_lines, shingles

#: A shingle appearing in more than this fraction of candidate destinations is boilerplate: it is a
#: template, an import block, a licence header, and it does not generate candidate pairs.
BOILERPLATE_FRACTION = 0.25
#: … and never fewer than this many destinations must share it before we call it boilerplate, so a
#: four-file window cannot lose its real evidence to a ratio.
BOILERPLATE_MIN = 3


@dataclass
class Blob:
    """One blob's shingle set, memoised per window run."""

    digest: str
    shingles: set[int]
    text_lines: int


class Shingles:
    """Shingle sets keyed by content, computed once per blob per run.

    Same bytes, same shingles: that is also what makes the pairing cacheable per blob pair the way
    `e2.1` caches classifications per blob, and why a digest — not a path — is the memo key.
    """

    def __init__(self, *, width: int, mode: str) -> None:
        self.width = width
        self.mode = mode
        self._cache: dict[str, Blob] = {}
        self.blobs_read = 0

    def of(self, data: bytes) -> Blob:
        digest = M.content_digest(data)
        cached = self._cache.get(digest)
        if cached is not None:
            return cached
        lines = normalise_lines(data, tokens=self.mode == "tokens")
        blob = Blob(digest=digest, shingles=shingles(lines, self.width), text_lines=len(lines))
        self._cache[digest] = blob
        self.blobs_read += 1
        return blob


@dataclass
class CandidateSets:
    sources: list[str]
    destinations: list[str]
    notes: list[str] = field(default_factory=list)

    def as_dict(self) -> dict:
        return {"sources": len(self.sources), "destinations": len(self.destinations),
                "cross_product": len(self.sources) * len(self.destinations),
                "notes": list(self.notes)}


def candidate_sets(window: M.Window, policy: Policy, *,
                   extra_sources: list[str] | None = None) -> CandidateSets:
    """Window-local candidate paths. `O(changed files)` — never the tree.

    Deletions are the classic source set. Modified paths join for both sides, and each side is a
    separate knob because each buys a different blind spot: a modified source is the only copy a
    window-local method can ever see (`copies_survive`), and a modified destination is the only way a
    fold into an existing module is reportable at all (`merge_many_to_one`).
    """
    notes: list[str] = []
    sources = set(window.removed)
    if policy.flag("copies"):
        sources |= set(window.modified)
    else:
        notes.append("modified paths are not copy sources (`rename.copies=false`)")
    if policy.flag("copies_harder"):
        # The one place the engine looks at the tree instead of the change set, and the reason the
        # knob is off: every one of these paths costs a blob read and a shingling pass.
        sources |= set(window.untouched_base_paths())
        notes.append("copy sources were widened to every path in the base tree: this window costs "
                     "O(repo files) — %d candidate source(s) instead of %d"
                     % (len(window.untouched_base_paths()), len(window.removed)))
    destinations = set(window.added)
    if policy.flag("folds"):
        destinations |= set(window.modified)
    else:
        notes.append("modified paths are not merge destinations (`rename.folds=false`)")
    for path in extra_sources or []:
        # git may name a source we did not think to look at — an untouched file, when the caller ran
        # it with --find-copies-harder. Take it, but say that the window is no longer purely local.
        if path not in sources and window.base_blob(path) is None and window.survives(path):
            sources.add(path)
            notes.append("git named an untouched source (%s); the caller paid for an O(repo) "
                         "copy search to find it" % path)
    return CandidateSets(sorted(sources), sorted(destinations), notes)


# -- content similarity ---------------------------------------------------------------------------

def _length_bound(left: int, right: int) -> float:
    """`min/max` of two set sizes: an upper bound on their Jaccard, free to compute.

    Two files of 300 and 30 shingles cannot score above 0.10 no matter what they contain, so the
    intersection never needs building. In a real window this is what keeps the content pass near
    linear in changed content instead of quadratic in changed files.
    """
    if not left or not right:
        return 0.0
    return min(left, right) / max(left, right)


def similarity(a: Blob, b: Blob) -> tuple[float, int]:
    """Exact Jaccard and the number of shared shingles — the receipt, not an estimate."""
    shared = len(a.shingles & b.shingles)
    return exact_jaccard(a.shingles, b.shingles), shared


def content_edges(window: M.Window, sets: CandidateSets, policy: Policy, store: Shingles,
                  skip: set[tuple[str, str]] = frozenset()) -> tuple[list[M.Edge], dict]:
    """Every plausible content pairing, by exact Jaccard, generated without a full cross product.

    The inverted index maps each destination's shingles to the destinations holding them, so a pair is
    only ever *seen* if it shares at least one shingle — the pairs that share none (all but a handful
    in any real window) cost nothing. Boilerplate shingles are excluded from generation, and then
    every surviving pair's similarity is computed exactly, so what the index decides is *what to look
    at* and never what to claim.
    """
    budget = policy.integer("budget")
    floor = policy.guess_floor
    min_shared = policy.integer("min_shared_shingles")
    stats = {"cross_product": len(sets.sources) * len(sets.destinations),
             "pruned_by_length": 0, "boilerplate_shingles": 0, "discovered": 0,
             "below_floor": 0, "thin_overlap": 0, "dropped_over_budget": 0,
             "scored": 0, "blobs_read": 0, "content_ms": 0.0}

    index: dict[int, list[str]] = defaultdict(list)
    dst_blobs = {dst: store.of(window.head_blob(dst) or b"") for dst in sets.destinations}
    boilerplate_limit = max(BOILERPLATE_MIN,
                            int(BOILERPLATE_FRACTION * max(1, len(sets.destinations))))
    counts: dict[int, int] = defaultdict(int)
    for dst, blob in dst_blobs.items():
        for shingle in blob.shingles:
            counts[shingle] += 1
    for dst, blob in dst_blobs.items():
        for shingle in blob.shingles:
            if counts[shingle] > boilerplate_limit:
                stats["boilerplate_shingles"] += 1
                continue
            index[shingle].append(dst)

    scored: list[tuple[float, int, str, str]] = []
    for src in sets.sources:
        src_blob = store.of(window.base_blob(src) or b"")
        if not src_blob.shingles:
            continue
        seen: set[str] = set()
        for shingle in src_blob.shingles:
            for dst in index.get(shingle, ()):      # candidate generation only
                if dst != src and dst not in seen and (src, dst) not in skip:
                    seen.add(dst)
        for dst in sorted(seen):
            dst_blob = dst_blobs[dst]
            if _length_bound(len(src_blob.shingles), len(dst_blob.shingles)) < floor:
                stats["pruned_by_length"] += 1
                continue
            jaccard, shared = similarity(src_blob, dst_blob)
            stats["scored"] += 1
            if shared < min_shared:
                stats["thin_overlap"] += 1
                continue
            if jaccard < floor:
                stats["below_floor"] += 1
                continue
            scored.append((jaccard, shared, src, dst))

    stats["discovered"] = len(scored)
    scored.sort(key=lambda row: (-row[0], row[2], row[3]))
    if len(scored) > budget:
        stats["dropped_over_budget"] = len(scored) - budget
        scored = scored[:budget]
    stats["scored_pairs"] = len(scored)
    edges = [M.Edge(src=src, dst=dst, score=jaccard, source=M.SOURCE_GUESS, method="content",
                    detail="%d shared of %d/%d %d-line shingles"
                           % (shared, len(dst_blobs[dst].shingles),
                              len(store.of(window.base_blob(src) or b"").shingles),
                              store.width),
                    name_similarity=name_similarity(src, dst))
             for jaccard, shared, src, dst in scored]
    stats["blobs_read"] = store.blobs_read
    return edges, stats


def git_edges(window: M.Window, policy: Policy) -> tuple[list[M.Edge], list[str]]:
    """git's own lines, split into fact-strength and guess-strength, with the contested ones demoted.

    A fact must survive one check: can the reviewer reproduce it? `git log --follow` says the same
    thing our fact tier does only if git was asked at a bar at least as strict as ours, which is what
    `rename.fact_bar` is for; below it git is a *candidate generator with a number attached*, which is
    worth showing and is not worth acting on.
    """
    notes: list[str] = []
    bar, floor = policy.fact_bar, policy.guess_floor
    strong: dict[str, int] = defaultdict(int)
    for pair in window.git_pairs:
        if pair.score >= bar:
            strong["s:" + pair.src] += 1
            strong["d:" + pair.dst] += 1
    edges: list[M.Edge] = []
    for pair in window.git_pairs:
        if pair.score < floor:
            notes.append("git reported %s -> %s at R%03d, below `rename.guess_floor`=%.2f: not "
                         "shown" % (pair.src, pair.dst, round(pair.score * 100), floor))
            continue
        fact = pair.score >= bar and not (strong["s:" + pair.src] > 1 or strong["d:" + pair.dst] > 1)
        if pair.score >= bar and not fact:
            notes.append("git reported %s -> %s at R%03d but claimed %s twice at or above the fact "
                         "bar: shown as a guess, because two facts about one blob is a "
                         "contradiction" % (pair.src, pair.dst, round(pair.score * 100),
                                            "its source" if strong["s:" + pair.src] > 1
                                            else "its destination"))
        edges.append(M.Edge(src=pair.src, dst=pair.dst, score=pair.score,
                            source=M.SOURCE_FACT if fact else M.SOURCE_GUESS, method="git",
                            detail="git -M%d%% output" % policy.git_candidate_bar(),
                            name_similarity=name_similarity(pair.src, pair.dst)))
    return edges, notes


def prune(edges: list[M.Edge], policy: Policy) -> tuple[list[M.Edge], list[str]]:
    """Keep each side's k best: a pair survives if either side wants it.

    This is `s4`'s pairing rule, and it is the only reason a 1-to-3 split can be reported at all: a
    best-match assignment drops two of the three destinations, and the two it drops are not the weak
    ones — they are equally true.
    """
    top = policy.integer("top_k")
    by_src: dict[str, list[M.Edge]] = defaultdict(list)
    by_dst: dict[str, list[M.Edge]] = defaultdict(list)
    for edge in edges:
        by_src[edge.src].append(edge)
        by_dst[edge.dst].append(edge)

    def best(rows: list[M.Edge]) -> set[tuple[str, str]]:
        ordered = sorted(rows, key=lambda e: (-e.score, e.method != M.SOURCE_FACT, e.src, e.dst))
        return {(e.src, e.dst) for e in ordered[:top]}

    keep: set[tuple[str, str]] = set()
    for rows in by_src.values():
        keep |= best(rows)
    for rows in by_dst.values():
        keep |= best(rows)
    dropped = [e for e in edges if (e.src, e.dst) not in keep]
    notes = []
    if dropped:
        notes.append("top-%d pruning dropped %d candidate pair(s): %s"
                     % (top, len(dropped),
                        "; ".join("%s->%s J%.2f" % (e.src, e.dst, e.score) for e in dropped[:3])
                        + (" …" if len(dropped) > 3 else "")))
    return [e for e in edges if (e.src, e.dst) in keep], notes


# -- grouping ------------------------------------------------------------------------------------

class _Union:
    def __init__(self) -> None:
        self.parent: dict[str, str] = {}

    def find(self, item: str) -> str:
        self.parent.setdefault(item, item)
        root = item
        while self.parent[root] != root:
            root = self.parent[root]
        while self.parent[item] != root:
            self.parent[item], item = root, self.parent[item]
        return root

    def union(self, left: str, right: str) -> None:
        a, b = self.find(left), self.find(right)
        if a != b:
            self.parent[b] = a


def shape_of(window: M.Window, sources: list[str], destinations: list[str]) -> str:
    """The shape, decided by what was *deleted*, not by how many paths are involved.

    A source that is still in the head tree was not moved: it was copied, and no amount of pairing
    confidence changes that. Order matters — a 1-to-1 with nothing deleted is a copy, not a rename.
    """
    if not any(path in window.removed for path in sources):
        return M.KIND_COPY
    if len(sources) == 1 and len(destinations) == 1:
        return M.KIND_RENAME
    if len(sources) == 1:
        return M.KIND_SPLIT
    if len(destinations) == 1:
        return M.KIND_MERGE
    return M.KIND_TANGLE


#: Why a shape can never be asserted, printed on the row. Every sentence here is a measured zero.
SHAPE_CEILINGS: dict[str, str] = {
    M.KIND_SPLIT: "a split cannot be a fact: git pairs each deletion with one addition and drops "
                  "the rest, so it reports 0 split pairs at any threshold (s4: G7)",
    M.KIND_MERGE: "a fold cannot be a fact: its destination is a modified file and never appears "
                  "among the additions, so git reports 0 folds (s4: G9)",
    M.KIND_COPY: "a copy cannot be a fact: its source was not deleted, so no pair exists for git "
                 "to report (s4: G11)",
    M.KIND_TANGLE: "a tangle cannot be a fact: several sources and several destinations are related, "
                   "and the blobs do not say which went where",
}


def group(window: M.Window, edges: list[M.Edge], policy: Policy,
          ledger: dict[str, M.Pairing] | None = None) -> list[M.Pairing]:
    """Connected components to pairings: shape, tier, evidence, disposition."""
    if not edges:
        return []
    union = _Union()
    for edge in edges:
        union.find("s:" + edge.src)
        union.find("d:" + edge.dst)
        union.union("s:" + edge.src, "d:" + edge.dst)

    components: dict[str, list[M.Edge]] = defaultdict(list)
    for edge in edges:
        components[union.find("s:" + edge.src)].append(edge)

    out: list[M.Pairing] = []
    for _root, rows in components.items():
        sources = sorted({e.src for e in rows})
        destinations = sorted({e.dst for e in rows})
        kind = shape_of(window, sources, destinations)
        ordered = sorted(rows, key=lambda e: (-e.score, e.src, e.dst))
        if policy.flag("allow_content"):
            # Only if a repository insists: our own score is now the thing a fact rests on, and the
            # row says so, because a reviewer cannot reproduce it without this package.
            for edge in ordered:
                if edge.score >= policy.fact_bar:
                    edge.source = M.SOURCE_FACT
                    edge.notes.append("asserted from `rename.fact.allow_content`: the number on "
                                      "this row is ours, not git's")
        all_fact = all(e.is_fact for e in ordered)
        tier = M.TIER_FACT if (kind == M.KIND_RENAME and all_fact) else M.TIER_GUESS
        confidence = min(e.score for e in ordered)   # a group is as sure as its weakest edge
        notes: list[str] = []
        if tier == M.TIER_GUESS and kind in SHAPE_CEILINGS:
            notes.append(SHAPE_CEILINGS[kind])
        if kind == M.KIND_RENAME and tier == M.TIER_GUESS and not all_fact:
            notes.append("git did not report this at the fact bar (%s), so the similarity on this "
                         "row is ours and a reviewer cannot re-check it with git"
                         % ", ".join(sorted({e.label() for e in ordered})))
        if any(e.is_fact for e in ordered) and tier == M.TIER_GUESS:
            notes.append("this group contains %d pairing(s) git does assert as a fact; the rest are "
                         "guesses, and the row is labelled by its weakest part"
                         % sum(1 for e in ordered if e.is_fact))
        surviving = [s for s in sources if window.survives(s)]
        if surviving and kind != M.KIND_COPY:
            notes.append("%s still in the tree after this window: %s"
                         % (", ".join(surviving), "it was copied as well as moved"))
        pairing = M.Pairing(window=window.key(), kind=kind, sources=tuple(sources),
                            destinations=tuple(destinations), tier=tier,
                            confidence=confidence,
                            edges=list(ordered),
                            binding=window.binding_for(kind, sources, destinations),
                            evidence=[e.render() for e in ordered], notes=notes)
        _apply_disposition(pairing, ledger)
        out.append(pairing)
    out.sort(key=lambda p: p.sort_key())
    return out


def _apply_disposition(pairing: M.Pairing, ledger: dict | None) -> None:
    """Look up the reviewer's earlier decision, and re-open it if the bytes moved.

    The ledger is keyed by path and shape, not by content, on purpose: keying it by content would make
    a stale decision simply disappear, which is how a review ledger quietly forgets that it disagreed.
    Finding a record whose binding does not match is an event — the pairing is shown again, as a
    guess, with the old decision named.
    """
    if not ledger:
        return
    record = ledger.get(pairing.key())
    if record is None:
        return
    decision = getattr(record, "decision", None) or record.get("decision")
    binding = getattr(record, "binding", None) or record.get("binding")
    reviewer = getattr(record, "reviewer", None) or record.get("reviewer", "")
    if binding == pairing.binding:
        pairing.disposition = decision
        pairing.reviewer = reviewer
        return
    pairing.disposition = M.DISPOSITION_REOPENED
    pairing.notes.append("a previous review %s this pairing (%s); the blobs behind it have since "
                         "changed, so the decision does not carry over and the pairing is a guess "
                         "again" % (decision, reviewer or "reviewer unknown"))


# -- the run -------------------------------------------------------------------------------------

def pair(window: M.Window, policy: Policy | None = None, *,
         ledger: dict | None = None, store: Shingles | None = None) -> M.Result:
    """Pair one window. The only entry point the CLI and the analyzers need."""
    policy = policy or Policy.built_in()
    store = store or Shingles(width=policy.integer("shingle_lines"), mode=policy.text("normalise"))
    git, git_notes = git_edges(window, policy)
    sets = candidate_sets(window, policy,
                          extra_sources=[p.src for p in window.git_pairs if p.src not in window.removed
                                         and window.survives(p.src)])
    content, stats = content_edges(window, sets, policy, store)
    # A git line already outranks our number for the same pair, so the matcher's version of that pair
    # becomes the git row's receipt instead of a second row about the same two files.
    by_pair = {(e.src, e.dst): e for e in content}
    for edge in git:
        twin = by_pair.get((edge.src, edge.dst))
        if twin is not None:
            edge.detail += "; %s agrees (%s)" % (twin.label(), twin.detail)
    content = [e for e in content if (e.src, e.dst) not in {(g.src, g.dst) for g in git}]
    edges = git + content
    edges, prune_notes = prune(edges, policy)
    pairings = group(window, edges, policy, ledger)

    limits = list(policy.limits())
    limits.extend(sets.notes)
    limits.extend(git_notes)
    limits.extend(prune_notes)
    if stats["dropped_over_budget"]:
        limits.append("candidate budget `%s` dropped %d plausible pair(s) after the top scores; "
                      "this window is larger than the budget and its recall is short by that "
                      "many" % ("rename.candidates.budget", stats["dropped_over_budget"]))
    if stats["boilerplate_shingles"]:
        limits.append("%d shingle(s) appeared in more than %d candidate destination(s) and were "
                      "treated as boilerplate: a shared template is not provenance (s4: G29)"
                      % (stats["boilerplate_shingles"],
                         max(BOILERPLATE_MIN,
                             int(BOILERPLATE_FRACTION * max(1, len(sets.destinations))))))
    return M.Result(window=window, pairings=pairings,
                    stats={"candidates": sets.as_dict(), "content": stats,
                           "git_pairs": len(window.git_pairs), "touches": window.touches,
                           "shingle_width": store.width, "normalise": store.mode},
                    limits=limits)
