"""Blast radius: how far this episode reaches, with the basis and the confidence printed beside it.

The plan's glossary (thesis §4.4) is the spec, and it is unusually precise for a one-line
definition:

> **Blast radius** — an episode's reach: the set (and count) of modules, files or symbols that
> directly depend on what changed, plus a one-hop estimate; path heuristics when the graph is
> unavailable, with the reduced confidence stated. — `e2.10`, graph `e3.2`.

Four things in that sentence are load-bearing, and each is a decision in this file:

**The graph is an input, not a dependency.** `e3.2` has not landed, so a blast radius that
*required* a module graph would be this ticket's own blocker. The caller lends a
:class:`~recommender.card.model.ModuleGraph` when it has one — the store manifest already
reserves `blast_radius` as an analyzer that `depends_on` `graph.delta` — and this module answers
from it. When the graph is missing, or has no node for anything that changed, it answers from
path heuristics instead and says which basis it used.

**A heuristic is a named rule with a weight, never a vibe.** Four rules, all knobs:
same-directory (0.60), test-pair (0.50), sibling-directory (0.30), package-prefix (0.15). A
candidate's score is the strongest rule that fired for it, and it is counted only at or above
`blast.heuristic.min_score` (0.30) — which is the knob that stops "it is in the same repository"
from being a blast radius. Every counted row prints the rule that counted it, so a reviewer who
thinks the reach is overstated can lower the weight that overstated it instead of arguing with a
number.

**Reduced confidence is arithmetic, not an adjective.** A graph answer carries
`blast.confidence.graph` (0.90) scaled by how much of the change set the graph actually has
nodes for; a heuristic answer carries `blast.confidence.heuristic` (0.40), multiplied by
`blast.confidence.narrow_census` (0.70) when the caller lent no file inventory — because a reach
computed from a world of twelve files cannot claim the same confidence as one computed from
4 000. The number is on the card, not only in the JSON.

**A reach that stopped is a stated partial.** Past `blast.hops` rings, past
`blast.max_candidates` examined paths, and for changed paths the graph has no node for, the
answer says what it did not look at and how many it left out. An unstopped walk over a monorepo
would blow the 50 ms the card is budgeted for; a stopped walk that says so is an honest answer
inside a budget (`benchmarks/budgets.json`, `query.card.ms`).

Path *classes* come from `e0.9`'s taxonomy, not from a private list: vendored, generated,
lockfile, binary and asset paths are never counted as dependents, because "the vendored copy of
protobuf depends on your module" is not reach. That is ADR-002's rule that nothing downstream
encodes its own glob list, applied once more.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .model import Episode
from .policy import (BASIS_GRAPH, BASIS_HEURISTIC, BASIS_NONE, PARTIAL_CANDIDATES,
                     PARTIAL_CENSUS, PARTIAL_HOPS, PARTIAL_UNMAPPED, Policy)

#: Heuristic rule ids, in the order they are tried (strongest first).
RULE_SAME_DIRECTORY = "same-directory"
RULE_TEST_PAIR = "test-pair"
RULE_SIBLING_DIRECTORY = "sibling-directory"
RULE_PACKAGE_PREFIX = "package-prefix"
RULE_GRAPH_EDGE = "graph-edge"
HEURISTIC_RULES: tuple[str, ...] = (RULE_SAME_DIRECTORY, RULE_TEST_PAIR,
                                    RULE_SIBLING_DIRECTORY, RULE_PACKAGE_PREFIX)

#: Path classes that cannot be dependents. Decided by `e0.9`, and asserted by a test rather
#: than trusted: see `tests/test_summary_card_contract.py`.
NON_DEPENDENT_CLASSES: frozenset[str] = frozenset(
    {"vendored", "lockfile", "generated", "binary", "asset"})

#: Class ids that mean "this is a test" for the test-pair rule.
TEST_CLASS = "test"

#: How many segment levels the package-prefix rule shares. Two, so `src/pay/x.py` and
#: `src/billing/y.py` are *not* package neighbours while `src/pay/a.py` and `src/pay/b.py` are.
PACKAGE_LEVELS = 2


@dataclass(frozen=True)
class Candidate:
    """One thing the change reaches, with the rule or edge that says so."""

    name: str
    hop: int                                 # 1 = direct, 2 = one hop beyond the direct ring
    score: float
    rules: tuple[str, ...] = ()
    via: tuple[str, ...] = ()
    basis: str = BASIS_HEURISTIC
    evidence: tuple[str, ...] = ()

    @property
    def rule(self) -> str:
        return self.rules[0] if self.rules else "?"

    @property
    def ring(self) -> str:
        return "direct" if self.hop == 1 else "hop %d" % self.hop

    def render(self) -> str:
        """One counted row: who, how far, from what, and by which rule.

        The rule prints always — it is what makes a heuristic row checkable rather than asserted.
        The weight prints only when it is not the graph's own certainty: a column of identical
        `1.00`s is width spent saying nothing, and the width is what wraps.
        """
        weight = "" if self.score >= 1.0 else " %.2f" % self.score
        via = ", ".join(self.via[:2]) or "—"
        if len(self.via) > 2:
            via += " +%d more" % (len(self.via) - 2)
        # Name, ring, rule together on one line; the route last, because if the width forces a fold
        # it should fold the route list and not split a row away from the rule that counted it.
        return "%-38s hop %-2d %s%s · via %s" % (self.name, self.hop, self.rule, weight, via)

    def as_dict(self) -> dict:
        return {"name": self.name, "hop": self.hop, "score": round(self.score, 3),
                "rules": list(self.rules), "via": list(self.via), "basis": self.basis,
                "evidence": list(self.evidence)}


@dataclass
class Blast:
    """The reach of one episode, and the honesty that goes with it."""

    episode_id: str
    basis: str = BASIS_NONE
    direct: tuple[Candidate, ...] = ()
    one_hop: tuple[Candidate, ...] = ()
    beyond: int = 0
    confidence: float = 0.0
    partials: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    examined: int = 0
    basis_reason: str = ""
    graph_nodes: int = 0
    graph_edges: int = 0
    changed_paths: int = 0
    mapped_paths: int = 0
    census_size: int = 0
    census_narrow: bool = False
    policy: Policy = field(default_factory=Policy.built_in)
    #: link target -> evidence rows, for the card's evidence map.
    evidence: dict[str, list[str]] = field(default_factory=dict)

    # -- counts -------------------------------------------------------------
    def counts(self) -> dict:
        return {"direct": len(self.direct), "one_hop": len(self.one_hop),
                "total": len(self.direct) + len(self.one_hop), "beyond": self.beyond,
                "basis": self.basis, "examined": self.examined,
                "unmapped_changed": max(0, self.changed_paths - self.mapped_paths),
                "census_size": self.census_size, "census_narrow": self.census_narrow,
                "graph_nodes": self.graph_nodes, "graph_edges": self.graph_edges,
                "partials": len(self.partials)}

    def names(self, hop: int | None = None) -> list[str]:
        out = [item.name for item in self.direct if hop in (None, 1)]
        if hop in (None, 2):
            out += [item.name for item in self.one_hop]
        return sorted(set(out))

    @property
    def measured(self) -> bool:
        return self.basis in (BASIS_GRAPH, BASIS_HEURISTIC)

    @property
    def headline(self) -> str:
        if not self.measured:
            return "reach not measured: %s" % (self.basis_reason or "no basis")
        text = "%d direct, %d one-hop" % (len(self.direct), len(self.one_hop))
        if self.beyond:
            text += ", %d further out not counted" % self.beyond
        return "%s · %s, confidence %s" % (
            text, self.basis, self.policy.state(self.confidence))

    def summary_line(self) -> str:
        return "%s · blast %s" % (self.episode_id, self.headline)

    def lines(self) -> list[str]:
        """The drill-down: every counted row, its rule, and what it is reached from."""
        out: list[str] = []
        cap = self.policy.integer("max_rows")
        for label, rows in (("direct", self.direct), ("one hop", self.one_hop)):
            if not rows:
                out.append("  %s: none counted (%s)" % (label, self.basis))
                continue
            out.append("  %s (%d)" % (label, len(rows)))
            for item in rows[:cap]:
                out.append("    %s" % item.render())
                if len(item.via) > 2:
                    out.append("      · and %d more reached from" % (len(item.via) - 2))
            if len(rows) > cap:
                out.append("    … %d more in the %s ring: fathom card blast %s --all"
                           % (len(rows) - cap, label, self.episode_id))
        for note in self.notes:
            out.append("  note: %s" % note)
        for partial in self.partials:
            out.append("  partial: %s" % partial)
        return out

    def as_dict(self) -> dict:
        return {"episode": self.episode_id, "basis": self.basis,
                "basis_reason": self.basis_reason,
                "confidence": round(self.confidence, 3), "counts": self.counts(),
                "direct": [item.as_dict() for item in self.direct],
                "one_hop": [item.as_dict() for item in self.one_hop],
                "beyond": self.beyond, "partials": list(self.partials),
                "notes": list(self.notes), "policy_digest": self.policy.config_digest}


# ---------------------------------------------------------------------------
# the walk
# ---------------------------------------------------------------------------
def measure(episode: Episode, policy: Policy | None = None, graph=None, taxonomy=None) -> Blast:
    """How far does this episode reach? Graph if it can, named heuristics if it cannot."""
    active = policy or Policy.built_in()
    blast = Blast(episode_id=episode.id, policy=active)
    changed = sorted(set(episode.paths))
    blast.changed_paths = len(changed)
    if not changed:
        blast.basis_reason = "this episode changes no paths"
        blast.evidence["basis"] = ["the episode's change list is empty, so there is nothing to "
                                   "reach from"]
        return blast

    if graph is not None and graph.usable:
        blast.graph_nodes = len(graph.nodes)
        blast.graph_edges = len(graph.edges)
        mapped = [path for path in changed if _covers(graph, path)]
        blast.mapped_paths = len(mapped)
        if mapped:
            _from_graph(blast, graph, changed, mapped, active)
            return blast
        blast.notes.append("the supplied module graph has no node for any of this episode's %d "
                           "changed path(s), so the answer is built from path heuristics"
                           % len(changed))
        blast.basis_reason = "no graph node for any changed path"

    _from_heuristics(blast, episode, changed, active, taxonomy=taxonomy, graph=graph)
    return blast


def _covers(graph, name: str) -> bool:
    """A graph covers a path when it has it as a node, or as the directory of a package node."""
    nodes = set(graph.nodes)
    if name in nodes:
        return True
    stems = _graph_stems(graph)
    return _stem(name) in stems


def _graph_stems(graph) -> set[str]:
    return {_stem(node) for node in graph.nodes}


def _stem(name: str) -> str:
    head, _, leaf = name.rpartition("/")
    leaf = leaf.rpartition(".")[0]
    return ("%s/%s" % (head, leaf) if head else leaf)


def _from_graph(blast: Blast, graph, changed: list[str], mapped: list[str],
                policy: Policy) -> None:
    """Direct dependents, then `blast.hops` further rings, following `dependent -> dependency`."""
    depth_limit = 1 + max(0, policy.integer("hops"))
    max_candidates = max(1, policy.integer("max_candidates"))
    origin = set(mapped)
    seen: dict[str, Candidate] = {}
    frontier = list(mapped)
    hop = 1
    stopped = 0
    beyond = 0
    while frontier and hop <= depth_limit + 1:
        nxt: list[str] = []
        for name in frontier:
            for dependent in graph.dependents_of(name):
                if dependent in origin or dependent in seen:
                    continue
                if hop > depth_limit:
                    # The pass past the limit exists to count what the limit stopped, so the
                    # card can say "and further" instead of ending the list in silence.
                    beyond += 1
                    continue
                if blast.examined >= max_candidates:
                    stopped += 1
                    continue
                blast.examined += 1
                seen[dependent] = Candidate(
                    name=dependent, hop=hop, score=1.0, rules=(RULE_GRAPH_EDGE,),
                    via=(name,), basis=BASIS_GRAPH,
                    evidence=["`%s` → `%s` is an edge in the supplied graph (%s, %d nodes, "
                              "%d edges)" % (dependent, name, graph.basis, len(graph.nodes),
                                             len(graph.edges))])
                nxt.append(dependent)
        if hop <= depth_limit:
            frontier = nxt
        else:
            frontier = []
        hop += 1
    blast.basis = BASIS_GRAPH
    blast.basis_reason = "%d of %d changed paths have graph nodes (%s)" % (
        len(mapped), len(changed), graph.basis)
    blast.direct = tuple(sorted((item for item in seen.values() if item.hop == 1),
                                key=lambda item: item.name))
    blast.one_hop = tuple(sorted((item for item in seen.values() if item.hop > 1),
                                 key=lambda item: (item.hop, item.name)))
    blast.beyond = beyond
    blast.examined += stopped
    if len(mapped) < len(changed):
        missing = sorted(set(changed) - set(mapped))
        blast.partials.append("%s: %d of %d changed paths (%s) — their dependents are unknown, "
                              "not zero" % (PARTIAL_UNMAPPED, len(missing), len(changed),
                                            ", ".join(missing[:3])
                                            + (", …" if len(missing) > 3 else "")))
    if stopped:
        blast.partials.append("%s: %d module(s) the walk stopped at, over the cost guard of "
                              "blast.max_candidates = %d"
                              % (PARTIAL_CANDIDATES, stopped, max_candidates))
    if beyond:
        blast.partials.append("%s: %d module(s) at blast.hops = %s or further out"
                              % (PARTIAL_HOPS, beyond, policy.integer("hops")))
    coverage = len(mapped) / float(len(changed)) if changed else 0.0
    blast.confidence = round(policy.number("confidence_graph") * coverage, 3)
    _attach_evidence(blast, graph_basis=True)


def _from_heuristics(blast: Blast, episode: Episode, changed: list[str], policy: Policy,
                     *, taxonomy=None, graph=None) -> None:
    """Four named path rules over the census the caller was able to lend."""
    universe = list(episode.inventory("after"))
    census_lent = episode.has_inventory("after")
    blast.census_size = len(universe)
    blast.census_narrow = not census_lent
    classify = _classifier(taxonomy)
    changed_set = set(changed)
    weights = {RULE_SAME_DIRECTORY: policy.number("w_same_directory"),
               RULE_TEST_PAIR: policy.number("w_test_pair"),
               RULE_SIBLING_DIRECTORY: policy.number("w_sibling_directory"),
               RULE_PACKAGE_PREFIX: policy.number("w_package_prefix")}
    floor = policy.number("min_score")
    max_candidates = max(1, policy.integer("max_candidates"))

    direct_dirs = {_directory(path) for path in changed}
    direct_parents = {_parent(_directory(path)) for path in changed}
    direct_packages = {_package(path) for path in changed}
    tests_of = {path: _test_names(path) for path in changed}
    changed_tests = [path for path in changed if classify(path) == TEST_CLASS]
    sources_of = {path: _source_names(path) for path in changed_tests}

    scored: dict[str, Candidate] = {}
    skipped_classes: dict[str, int] = {}
    stopped = 0
    for candidate in sorted(set(universe)):
        if candidate in changed_set:
            continue
        if blast.examined >= max_candidates:
            stopped += 1
            continue
        rules: list[str] = []
        if _directory(candidate) in direct_dirs:
            rules.append(RULE_SAME_DIRECTORY)
        if candidate in _test_keys(tests_of) or candidate in _source_keys(sources_of):
            rules.append(RULE_TEST_PAIR)
        if _parent(_directory(candidate)) in direct_parents and _directory(candidate) not in \
                direct_dirs:
            rules.append(RULE_SIBLING_DIRECTORY)
        if _package(candidate) in direct_packages:
            rules.append(RULE_PACKAGE_PREFIX)
        blast.examined += 1
        if not rules:
            continue
        cls = classify(candidate)
        if cls in NON_DEPENDENT_CLASSES:
            skipped_classes[cls] = skipped_classes.get(cls, 0) + 1
            continue
        best = max(rules, key=lambda rule: weights[rule])
        score = weights[best]
        if score < floor:
            continue
        scored[candidate] = Candidate(
            name=candidate, hop=1, score=score, rules=tuple(
                sorted(rules, key=lambda rule: -weights[rule])),
            via=_touched_by(candidate, changed, direct_dirs, direct_parents, direct_packages,
                            tests_of, sources_of),
            basis=BASIS_HEURISTIC,
            evidence=["`%s` is reached from `%s`: rule `%s` (weight %s ≥ min_score %s), class "
                      "`%s`" % (candidate, "`/`".join(
                          _touched_by(candidate, changed, direct_dirs, direct_parents,
                                      direct_packages, tests_of, sources_of)[:3]) or "—",
                          best, _num(weights[best]), _num(floor), cls)])
    blast.basis = BASIS_HEURISTIC
    blast.basis_reason = ("no module graph supplied; reach comes from path rules over a census "
                          "of %d path%s" % (blast.census_size, "" if blast.census_size == 1 else "s")
                          if census_lent else
                          "no module graph and no repository inventory: the census is this "
                          "episode's own %d path%s" % (blast.census_size,
                                                       "" if blast.census_size == 1 else "s"))
    rows = sorted(scored.values(), key=lambda item: (-item.score, item.name))
    blast.direct = tuple(rows)
    blast.one_hop = ()
    blast.beyond = stopped
    if skipped_classes:
        blast.notes.append("%d path%s excluded as dependents on `e0.9`'s classification (%s)"
                           % (sum(skipped_classes.values()),
                              "" if sum(skipped_classes.values()) == 1 else "s",
                              ", ".join("%s %d" % pair for pair in
                                        sorted(skipped_classes.items()))))
    if blast.census_narrow:
        blast.partials.append("%s: the caller lent no file inventory, so nothing outside this "
                              "episode could be counted — the reach is a floor, not an estimate"
                              % PARTIAL_CENSUS)
    if stopped:
        blast.partials.append("%s: %d path(s) after blast.max_candidates = %s"
                              % (PARTIAL_CANDIDATES, stopped, policy.integer("max_candidates")))
    confidence = policy.number("confidence_heuristic")
    if blast.census_narrow:
        confidence *= policy.number("confidence_narrow_census")
    blast.confidence = round(confidence, 3)
    if graph is not None and graph.usable:
        blast.notes.append("the graph that was supplied covers none of this episode's changed "
                           "paths (%d nodes, %d edges)" % (len(graph.nodes), len(graph.edges)))
    _attach_evidence(blast, graph_basis=False)


def _attach_evidence(blast: Blast, *, graph_basis: bool) -> None:
    """Evidence rows for every link the card will print for this answer."""
    basis_rows = ["basis: %s — %s" % (blast.basis, blast.basis_reason),
                  "confidence %s from %s (%s)"
                  % (_num(blast.confidence),
                     "blast.confidence.graph" if graph_basis else "blast.confidence.heuristic",
                     "scaled by %d/%d changed paths mapped"
                     % (blast.mapped_paths, blast.changed_paths) if graph_basis
                     else ("multiplied by blast.confidence.narrow_census because no inventory "
                           "was supplied" if blast.census_narrow else "census is the full "
                           "inventory the caller supplied")),
                  "%d candidate path(s) examined, hop limit %d"
                  % (blast.examined, blast.policy.integer("hops"))]
    blast.evidence["basis"] = basis_rows
    blast.evidence["direct"] = [item.render() for item in blast.direct] or \
        ["no dependents counted in the direct ring (%s)" % blast.basis]
    blast.evidence["one-hop"] = [item.render() for item in blast.one_hop] or \
        ["no second-ring dependents counted (%s; blast.hops = %d)"
         % (blast.basis, blast.policy.integer("hops"))]
    if blast.partials:
        blast.evidence["partials"] = list(blast.partials)
    for item in list(blast.direct) + list(blast.one_hop):
        blast.evidence[item.name] = list(item.evidence)


# ---------------------------------------------------------------------------
# path arithmetic (pure, and shared with the tests)
# ---------------------------------------------------------------------------
def _directory(path: str) -> str:
    head, _, _ = path.rpartition("/")
    return head


def _parent(directory: str) -> str:
    head, _, _ = directory.rpartition("/")
    return head


def _package(path: str) -> str:
    parts = _directory(path).split("/") if _directory(path) else []
    parts = [part for part in parts if part]
    return "/".join(parts[:PACKAGE_LEVELS])


def _classifier(taxonomy=None):
    """A `path -> class` callable, from `e0.9`'s engine, with no glob list of our own."""
    if callable(taxonomy):
        return taxonomy
    engine = taxonomy
    if engine is None:
        from recommender.taxonomy import engine as taxonomy_engine
        engine = taxonomy_engine.default_taxonomy()

    def classify(path: str) -> str:
        try:
            verdict = engine.classify(path)
        except Exception:                            # a taxonomy that cannot answer is no class
            return ""
        return str(getattr(verdict, "cls", "") or "")

    return classify


def _test_keys(tests_of: dict[str, tuple[str, ...]]) -> set[str]:
    out: set[str] = set()
    for names in tests_of.values():
        out.update(names)
    return out


def _source_keys(sources_of: dict[str, tuple[str, ...]]) -> set[str]:
    out: set[str] = set()
    for names in sources_of.values():
        out.update(names)
    return out


_TEST_PREFIXES = ("test_", "test.")
_TEST_SUFFIXES = ("_test.py", "_test.go", ".test.ts", ".test.tsx", ".test.js", ".test.jsx",
                  "_spec.rb", ".spec.ts", "_test.rs", "test.py")


def _is_test_path(path: str) -> bool:
    leaf = path.rpartition("/")[2]
    return leaf.startswith(_TEST_PREFIXES) or leaf.endswith(_TEST_SUFFIXES)


def _test_names(path: str) -> tuple[str, ...]:
    """The paths a changed source file's tests would live at, by naming convention only."""
    stem = _stem(path)
    directory, _, leaf = stem.rpartition("/")
    prefix = "" if not directory else directory + "/"
    out = ["%stest_%s.py" % (prefix, leaf), "tests/%s.py" % leaf,
           "tests/test_%s.py" % leaf, "%s_test.go" % stem, "%s_test.rs" % stem,
           "%s.spec.ts" % stem, "%s.test.ts" % stem, "%sTest.java" % stem]
    return tuple(item for item in out if item != path)


def _source_names(path: str) -> tuple[str, ...]:
    """The source a changed test file looks like it covers, by naming convention only."""
    directory, _, leaf = path.rpartition("/")
    stem = leaf.rpartition(".")[0]
    for cut in ("test_", "test.", "_test", ".test", ".spec", "test"):
        if stem.startswith(cut) and len(stem) > len(cut):
            stem = stem[len(cut):]
            break
    else:
        if stem.endswith("_test"):
            stem = stem[:-len("_test")]
    candidates = ("src/%s.py" % stem, "%s.py" % stem, "%s.rs" % stem, "%s.go" % stem,
                  "%s.ts" % stem, "src/%s.rs" % stem, "src/%s/%s.py" % (directory, stem))
    return tuple(item for item in candidates if item != path)


def _touched_by(candidate: str, changed: list[str], direct_dirs: set[str], direct_parents: set[str],
                direct_packages: set[str], tests_of: dict[str, tuple[str, ...]],
                sources_of: dict[str, tuple[str, ...]]) -> tuple[str, ...]:
    """Which changed files make this candidate count as reached — the `via` on the row."""
    out: list[str] = []
    for path in changed:
        if _directory(candidate) == _directory(path):
            out.append(path)
        elif candidate in tests_of.get(path, ()) or candidate in sources_of.get(path, ()):
            out.append(path)
        elif _parent(_directory(candidate)) == _parent(_directory(path)):
            out.append(path)
        elif _package(candidate) and _package(candidate) == _package(path):
            out.append(path)
    return tuple(sorted(set(out)))


def _num(value: float) -> str:
    return ("%.3f" % float(value)).rstrip("0").rstrip(".")
