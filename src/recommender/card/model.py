"""What a card is *given*: the episode, and the analyses the caller lends it.

Nothing in this module reads anything. ADR-002 §4.5 applies with particular force here, because
this is the one analysis in the product whose whole job is to relay other analyses: if the card
package opened files, read the clock, or reached into the store, then every figure on the card
would inherit an unclear provenance and the byte-determinism the ticket asks for would be a
hope rather than a property.

So the card takes its inputs as data:

* an :class:`Episode` — id, title, and the changed files with their two revisions (the same
  shape `e2.2`, `e2.3`, `e2.8` and `e2.9` all take);
* optional **analyses** the caller has already computed — the noise view, the structure delta,
  the test surface, the non-code surface, the path classes — which is how a warm index pays
  nothing at card time for work the index already did;
* two inputs no Python analysis can compute: the :class:`DepsDelta` headline (`e2.6` lives in
  `fathom-analyze::deps`) and the :class:`ModuleGraph` (`e3.2`, not built yet);
* an optional :class:`RiskScore`, which is `e4.4`'s.

Absence is data too. An analysis the caller does not supply is an :class:`Absent` with a reason
and the verb that would fill it, and the card prints that as a section instead of printing an
empty one — which is what makes "complete and correct with all optional analyses unavailable"
a testable claim rather than a mood.

The one derived value this module owns is the line statistics of a change (`added`, `removed`),
computed from the two revisions with the same `difflib` discipline every sibling package uses,
so the card's counts and the diff pane's counts come from one arithmetic.
"""

from __future__ import annotations

import difflib
from dataclasses import dataclass, field

#: Line-change kinds, spelled as the store spells them.
ADDED = "added"
DELETED = "deleted"
MODIFIED = "modified"
RENAMED = "renamed"
CHANGE_TYPES: tuple[str, ...] = (ADDED, DELETED, MODIFIED, RENAMED)

#: Why an analysis is missing, spelled so a card row can name the verb that would fill it.
ABSENT_NOT_RUN = "not computed for this episode"
ABSENT_NOT_COMPILED = "not available in this build"
ABSENT_NO_GRAPH = "no module graph supplied"


@dataclass(frozen=True)
class LineStats:
    """Lines added and removed between two revisions, and the identity claim that goes with them."""

    added: int = 0
    removed: int = 0

    @property
    def changed(self) -> int:
        return self.added + self.removed

    def as_dict(self) -> dict:
        return {"added": self.added, "removed": self.removed, "changed": self.changed}


def line_stats(before: str | None, after: str | None) -> LineStats:
    """`difflib` line counts, matching what `e2.2` and `e2.9` count as a changed line."""
    if before is None:
        return LineStats(added=len(_lines(after)), removed=0)
    if after is None:
        return LineStats(added=0, removed=len(_lines(before)))
    added = removed = 0
    for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(
            None, _lines(before), _lines(after), autojunk=False).get_opcodes():
        if tag == "insert":
            added += j2 - j1
        elif tag == "delete":
            removed += i2 - i1
        elif tag == "replace":
            added += j2 - j1
            removed += i2 - i1
    return LineStats(added=added, removed=removed)


def _lines(text: str | None) -> list[str]:
    if not text:
        return []
    return text.splitlines()


@dataclass(frozen=True)
class Change:
    """One file's before and after, in review order."""

    path: str
    before: str | None = None
    after: str | None = None
    change_type: str = MODIFIED
    old_path: str = ""

    @classmethod
    def from_pair(cls, path: str, before: str | None, after: str | None,
                  **kwargs) -> "Change":
        change_type = str(kwargs.pop("change_type", "") or "")
        if not change_type:
            change_type = ADDED if before is None else (
                DELETED if after is None else MODIFIED)
        return cls(path=path, before=before, after=after, change_type=change_type, **kwargs)

    @property
    def subject(self) -> str:
        """The path the reviewer would type: the new one, unless the file is gone."""
        return self.old_path if self.change_type == DELETED and self.old_path else self.path

    @property
    def lines(self) -> LineStats:
        return line_stats(self.before, self.after)

    @property
    def has_bytes(self) -> bool:
        return self.before is not None or self.after is not None

    def as_dict(self) -> dict:
        return {"path": self.path, "change": self.change_type, "old_path": self.old_path,
                "lines": self.lines.as_dict(), "bytes": self.has_bytes}


@dataclass(frozen=True)
class Episode:
    """The change set, plus the two inventories the caller can lend.

    `before_paths` / `after_paths` are the whole-tree file lists. They are optional, and they
    are the difference between a blast-radius heuristic that can see the repository and one
    that can only see the episode — which the card prints as a confidence multiplier
    (`blast.confidence.narrow_census`) instead of silently narrowing the world.
    """

    id: str
    files: tuple[Change, ...] = ()
    title: str = ""
    before_paths: tuple[str, ...] = ()
    after_paths: tuple[str, ...] = ()
    author: str = ""
    commit_count: int = 0

    @classmethod
    def from_pairs(cls, episode_id: str, pairs: dict[str, tuple[str | None, str | None]],
                   **kwargs) -> "Episode":
        return cls(id=episode_id,
                   files=tuple(Change.from_pair(path, before, after)
                               for path, (before, after) in sorted(pairs.items())),
                   **kwargs)

    @classmethod
    def from_tree_pair(cls, episode_id: str, before: dict[str, str | None],
                       after: dict[str, str | None], **kwargs) -> "Episode":
        """An episode out of two `{path: content}` inventories, with both carried onto it."""
        removed = sorted(path for path in before if path not in after)
        added = sorted(path for path in after if path not in before)
        modified = sorted(path for path in after if path in before and after[path] != before[path])
        files = [Change(path=path, before=before[path], after=None, change_type=DELETED,
                        old_path=path) for path in removed]
        files += [Change(path=path, before=None, after=after[path], change_type=ADDED)
                  for path in added]
        files += [Change(path=path, before=before[path], after=after[path],
                         change_type=MODIFIED) for path in modified]
        return cls(id=episode_id, files=tuple(files),
                   before_paths=tuple(sorted(before)), after_paths=tuple(sorted(after)),
                   **kwargs)

    @property
    def paths(self) -> list[str]:
        return [change.subject for change in self.files]

    @property
    def added_paths(self) -> list[str]:
        return [change.path for change in self.files if change.change_type == ADDED]

    @property
    def deleted_paths(self) -> list[str]:
        return [change.subject for change in self.files if change.change_type == DELETED]

    @property
    def lines(self) -> LineStats:
        return LineStats(sum(change.lines.added for change in self.files),
                         sum(change.lines.removed for change in self.files))

    def inventory(self, side: str = "after") -> tuple[str, ...]:
        """The repository census: the lent inventory when there is one, else the episode's paths."""
        lent = self.after_paths if side == "after" else self.before_paths
        if lent:
            return tuple(lent)
        return tuple(sorted(set(self.paths) | set(self.after_paths) | set(self.before_paths)))

    def has_inventory(self, side: str = "after") -> bool:
        return bool(self.after_paths if side == "after" else self.before_paths)

    def as_dict(self) -> dict:
        return {"id": self.id, "title": self.title, "files": len(self.files),
                "lines": self.lines.as_dict(), "author": self.author,
                "commits": self.commit_count,
                "inventory": {"after": len(self.after_paths), "before": len(self.before_paths)}}


# ---------------------------------------------------------------------------
# the analyses the caller lends
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Absent:
    """An analysis the card was not given: a reason, and the verb that would fill it."""

    analysis: str
    reason: str
    run: str = ""
    ticket: str = ""

    def sentence(self) -> str:
        text = "%s: %s" % (self.analysis, self.reason)
        if self.run:
            text += " — run `%s`" % self.run
        return text


@dataclass(frozen=True)
class DepsDelta:
    """`e2.6`'s per-episode headline, as the card consumes it.

    The dependency delta is computed in Rust (`fathom-analyze::deps::dependency_episode`); the
    card is the consumer, and this is the shape it needs. Every count is optional: a caller that
    can say "3 direct additions" and nothing else gets a section that says exactly that, and the
    fields it did not fill are printed as unknown rather than as zero.
    """

    headline: str = ""
    ecosystems: tuple[str, ...] = ()
    direct_added: int | None = None
    direct_removed: int | None = None
    upgraded: int | None = None
    downgraded: int | None = None
    transitive_only: int | None = None
    source_switched: int | None = None
    integrity_changed: int | None = None
    licence_changed: int | None = None
    conflicts: int | None = None
    unparseable: int | None = None
    basis: str = "resolved graph diff"
    evidence: tuple[str, ...] = ()
    source: str = "caller"

    @property
    def measured(self) -> bool:
        """True when at least one count was actually filled in."""
        return any(value is not None for value in (
            self.direct_added, self.direct_removed, self.upgraded, self.downgraded,
            self.transitive_only, self.source_switched, self.integrity_changed,
            self.licence_changed, self.conflicts))

    def counts(self) -> dict[str, int]:
        keys = ("direct_added", "direct_removed", "upgraded", "downgraded", "transitive_only",
                "source_switched", "integrity_changed", "licence_changed", "conflicts")
        return {key: int(getattr(self, key) or 0) for key in keys}

    def unknown(self) -> list[str]:
        return sorted(key for key in
                      ("direct_added", "direct_removed", "upgraded", "downgraded",
                       "transitive_only", "source_switched", "integrity_changed",
                       "licence_changed", "conflicts", "unparseable")
                      if getattr(self, key) is None)

    def as_dict(self) -> dict:
        out = {key: getattr(self, key) for key in
               ("headline", "ecosystems", "basis", "source", "direct_added", "direct_removed",
                "upgraded", "downgraded", "transitive_only", "source_switched",
                "integrity_changed", "licence_changed", "conflicts", "unparseable")}
        out["ecosystems"] = list(self.ecosystems)
        out["evidence"] = list(self.evidence)
        out["unknown"] = self.unknown()
        return {key: value for key, value in out.items() if value not in ((), [], None, "")}


@dataclass(frozen=True)
class ModuleGraph:
    """The module graph the caller lends — `e3.2`'s, when it exists.

    `edges` are ``(dependent, dependency)`` pairs: an edge `a → b` says `a` uses `b`, so a
    change to `b` reaches `a`. Names are paths (a module file, or a directory for a package
    node); the card never re-resolves them.
    """

    nodes: tuple[str, ...] = ()
    edges: tuple[tuple[str, str], ...] = ()
    basis: str = "resolved imports"
    revision: str = "after"
    source: str = "caller"
    partial: tuple[str, ...] = ()

    @property
    def usable(self) -> bool:
        return bool(self.edges) or bool(self.nodes)

    def dependents_of(self, name: str) -> tuple[str, ...]:
        return tuple(sorted({src for src, dst in self.edges if dst == name}))

    def covers(self, name: str) -> bool:
        return name in set(self.nodes)

    def as_dict(self) -> dict:
        return {"nodes": len(self.nodes), "edges": len(self.edges), "basis": self.basis,
                "revision": self.revision, "source": self.source, "partial": list(self.partial)}


@dataclass(frozen=True)
class RiskScore:
    """`e4.4`'s score, if the caller has one. The card relays it and adds nothing."""

    score: float = 0.0
    rank: int = 0
    of: int = 0
    reasons: tuple[str, ...] = ()
    blind_spots: tuple[str, ...] = ()
    source: str = "caller"

    def as_dict(self) -> dict:
        return {"score": self.score, "rank": self.rank, "of": self.of,
                "reasons": list(self.reasons), "blind_spots": list(self.blind_spots)}


@dataclass
class Analyses:
    """What the caller hands the card, with every gap kept as a named gap."""

    noise: object | None = None
    structure: object | None = None
    tests: object | None = None
    noncode: object | None = None
    classes: dict[str, int] | None = None
    class_confidence: float | None = None
    deps: DepsDelta | None = None
    graph: ModuleGraph | None = None
    risk: RiskScore | None = None
    #: `analysis id -> Absent`, for the gaps the caller knows it is leaving.
    absent: dict[str, Absent] = field(default_factory=dict)
    #: Findings the caller adds that no supplied analysis raised (`e4.3`'s, when it lands).
    extra_findings: tuple[object, ...] = ()
    source: str = "computed at card time"

    def missing(self) -> list[str]:
        out = [name for name, value in (("noise", self.noise), ("structure", self.structure),
                                        ("tests", self.tests), ("noncode", self.noncode),
                                        ("classes", self.classes), ("dependencies", self.deps),
                                        ("module graph", self.graph)) if value is None]
        return sorted(out)

    def as_dict(self) -> dict:
        return {"supplied": sorted(name for name in
                                   ("noise", "structure", "tests", "noncode", "classes",
                                    "deps", "graph", "risk")
                                   if getattr(self, name) is not None),
                "missing": self.missing(),
                "absent": {key: value.sentence() for key, value in sorted(self.absent.items())},
                "source": self.source}


def analyse(episode: Episode, *, noise=None, structure=None, tests=None, noncode=None,
            classes=None, class_confidence: float | None = None, deps=None, graph=None,
            risk=None, absent: dict[str, Absent] | None = None) -> Analyses:
    """Build an :class:`Analyses`, computing whatever the caller did not supply.

    The four analyses computed here are pure functions of the episode's bytes — so calling this
    at card time is correct, just not free. A warm index passes the stored objects instead, and
    the card cannot tell the difference, which is the point of taking analyses at all.
    """
    from recommender.change.episode import Episode as ClassEpisode
    from recommender.change.episode import classify_episode
    from recommender.noise import episode as noise_mod
    from recommender.noncode import episode as noncode_mod
    from recommender.structure import delta as structure_mod
    from recommender.testsurface import delta as tests_mod

    pairs = {change.path: (change.before, change.after) for change in episode.files}
    title = episode.title
    if noise is None:
        noise = noise_mod.suppress(noise_mod.Episode(
            id=episode.id,
            files=tuple(noise_mod.Change(path=change.path, before=change.before,
                                         after=change.after)
                        for change in episode.files), title=title))
    if structure is None:
        structure = structure_mod.measure(structure_mod.Episode(
            id=episode.id,
            files=tuple(structure_mod.Change(path=change.path, before=change.before,
                                             after=change.after,
                                             change_type=change.change_type,
                                             old_path=change.old_path)
                        for change in episode.files),
            title=title,
            before_paths=tuple(episode.before_paths), after_paths=tuple(episode.after_paths)))
    if tests is None:
        tests = tests_mod.measure(tests_mod.Episode.from_pairs(episode.id, pairs, title=title))
    if noncode is None:
        noncode = noncode_mod.measure(noncode_mod.Episode.from_pairs(episode.id, pairs,
                                                                     title=title))
    if classes is None:
        census = classify_episode(ClassEpisode.from_files(
            episode.id, {change.path: (change.after if change.after is not None
                                       else (change.before or ""))
                         for change in episode.files}))
        classes = census.class_counts()
        if class_confidence is None:
            class_confidence = census.mean_confidence()
    return Analyses(noise=noise, structure=structure, tests=tests, noncode=noncode,
                    classes=classes, class_confidence=class_confidence,
                    deps=deps, graph=graph, risk=risk, absent=dict(absent or {}),
                    source="computed at card time")
