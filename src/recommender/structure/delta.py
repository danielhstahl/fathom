"""The episode answer: a directory table that reconciles, metrics that move, and one sentence.

One call is the whole ticket:

    from recommender.structure import Change, Episode, measure

    view = measure(Episode(id="ep-4f2a", files=[Change(path=…, old_path=…, …)]))
    view.summary()["sentence"]
    # ep-4f2a · structure: `src/pay/` was renamed to `src/billing/` (6 files, 42 line(s)
    # travelled with them, directory shape preserved); 1 directory added, 1 removed
    # · churn 0.418 (structural).
    view.rows            # the per-directory table
    view.counts()        # the episode totals the table must sum to
    view.verify()        # [] when it does, a named problem when it does not
    view.churn           # the score, its band and its five components
    view.findings        # anchored, evidenced findings, severity-ordered
    view.summary()["evidence"]   # the receipts behind every clause of that sentence

Five decisions live here and nowhere else, so `e2.10`'s card, the timeline rail, the export
and the risk score cannot each grow their own answer to "where did this land":

**Which directory a line or file belongs to.** One rule, applied once per file: the directory
it **lands in**. A moved file's added, removed and travelled lines are all credited to the
destination — that is where the reviewer has to look — while the source row still gets
`files_moved_out` and `lines_moved_out`. Two columns that must agree (`files_moved_in` sums
to `files_moved_out` sums to `files_moved`) is a stronger reconciliation than one column that
cannot disagree with itself, so the table is built to be *checkable*, not just consistent.

**What the totals are.** `counts()` is computed from the file facts, not from the rows, and
:func:`StructureDelta.verify` then checks the rows sum to it, column by column, including that
moved lines and moved files balance across their two columns. A property test
(`tests/test_folder_structure_contract.py`, AC 2) runs a few hundred generated episodes
through it, which is the only way this kind of invariant stays true after the next fixture
adds an awkward case.

**Which claims need a full inventory.** "New directory" and "this directory holds 14 files"
are different claims depending on what the caller lent, so the two trees carry a `basis` and
every novelty claim in the card, the JSON and the summary says which one it is.

**What the one-sentence summary is made of.** Ordered clauses, fixed wording, every number
taken from the same objects the table is built from, and one evidence list per clause
(:meth:`StructureDelta.summary`). The order is stories → directory novelty → tree-shape
metrics → the local fallback when there are no stories, so the sentence leads with the
structure story and never with a metric. Because the wording is a fixed table per shape,
the corpus can hand-author the expected sentence instead of blessing whatever came out —
which is the difference between a golden test and a screenshot.

**What is refused.** Findings anchor themselves or raise (`policy.UnanchoredFinding`); a
score component the caller could not supply prints as blind rather than vanishing; and the
summary never claims a move it could not pair.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from . import churn as CH
from . import moves as MV
from .model import (Change, Episode, LineStats, depth_of, directory_of, line_stats,
                    total_lines)
from .policy import (BASIS_EPISODE, BASIS_INVENTORY, CHURN_HIGH, CONCENTRATION_UP,
                     DIRECTORY_ADDED, DIRECTORY_FLATTENED, DIRECTORY_REMOVED, FILES_MOVED,
                     MOVE_UNCONFIRMED, MODULE_SPLIT, NESTING_DEEPENED, PACKAGE_RENAMED,
                     SEVERITY_ORDER, SHAPE_FILES_MOVED, SHAPE_FLATTEN, SHAPE_MODULE_SPLIT,
                     SHAPE_PACKAGE_RENAME, SHAPE_SUBTREE_MOVE, SUBTREE_MOVED, Finding,
                     Policy, anchor, sort_key)
from .tree import DirCensus, FileEntry, Metrics, Tree, build_trees, delta_metrics

#: Version of the delta's shape and rules. A stored delta that disagrees is recomputed.
STRUCTURE_VERSION = 1

#: Files the classifier could not put in a configured code class still count as files.
_LOCAL_FALLBACK_LIMIT = 3


@dataclass
class FileFact:
    """One changed file: its class, its line movement, and the pairing it belongs to (if any)."""

    path: str
    old_path: str = ""
    change_type: str = "modified"
    cls: str = "other"
    code: bool = False
    lines: LineStats = field(default_factory=LineStats)
    lines_before: int | None = None
    lines_after: int | None = None
    pair: MV.Pair | None = None
    notes: list[str] = field(default_factory=list)

    @property
    def is_move(self) -> bool:
        return self.pair is not None

    @property
    def destination(self) -> str:
        return directory_of(self.path)

    @property
    def origin(self) -> str:
        return directory_of(self.old_path or self.path)

    def as_dict(self) -> dict:
        return {"path": self.path, "old_path": self.old_path, "change_type": self.change_type,
                "class": self.cls, "code": self.code, "lines": self.lines.as_dict(),
                "lines_before": self.lines_before, "lines_after": self.lines_after,
                "directory": self.destination, "origin": self.origin,
                "pair": self.pair.as_dict() if self.pair else None,
                "notes": list(self.notes)}


@dataclass
class DirRow:
    """One row of the directory table — the ticket's "per directory" list."""

    path: str
    depth: int = 0
    files_before: int = 0
    files_after: int = 0
    code_lines_before: int = 0
    code_lines_after: int = 0
    subdirectories_before: int = 0
    subdirectories_after: int = 0
    lines_added: int = 0
    lines_removed: int = 0
    lines_moved_in: int = 0
    lines_moved_out: int = 0
    files_created: int = 0
    files_deleted: int = 0
    files_moved_in: int = 0
    files_moved_out: int = 0
    directory_added: bool = False
    directory_removed: bool = False
    basis: str = BASIS_EPISODE
    notes: list[str] = field(default_factory=list)

    @property
    def display(self) -> str:
        return (self.path or "./") + ("/" if self.path else "")

    @property
    def active(self) -> bool:
        """Did this episode do anything here at all?"""
        return bool(self.files_created or self.files_deleted or self.files_moved_in
                    or self.files_moved_out or self.lines_added or self.lines_removed
                    or self.directory_added or self.directory_removed)

    def as_dict(self) -> dict:
        return {"directory": self.path or "./", "depth": self.depth,
                "files_before": self.files_before, "files_after": self.files_after,
                "code_lines_before": self.code_lines_before,
                "code_lines_after": self.code_lines_after,
                "subdirectories_before": self.subdirectories_before,
                "subdirectories_after": self.subdirectories_after,
                "lines_added": self.lines_added, "lines_removed": self.lines_removed,
                "lines_moved_in": self.lines_moved_in,
                "lines_moved_out": self.lines_moved_out,
                "files_created": self.files_created, "files_deleted": self.files_deleted,
                "files_moved_in": self.files_moved_in,
                "files_moved_out": self.files_moved_out,
                "directory_added": self.directory_added,
                "directory_removed": self.directory_removed,
                "basis": self.basis, "notes": list(self.notes)}


@dataclass
class StructureDelta:
    """Everything this ticket can say about one episode."""

    episode_id: str = ""
    title: str = ""
    policy: Policy = field(default_factory=Policy.built_in)
    files: list[FileFact] = field(default_factory=list)
    rows: list[DirRow] = field(default_factory=list)
    pairs: list[MV.Pair] = field(default_factory=list)
    stories: list[MV.Story] = field(default_factory=list)
    findings: list[Finding] = field(default_factory=list)
    suppressed: dict[str, int] = field(default_factory=dict)
    suppressed_source: dict[str, str] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)
    partials: list[str] = field(default_factory=list)
    before: Tree = field(default_factory=Tree)
    after: Tree = field(default_factory=Tree)
    metrics_before: Metrics = field(default_factory=Metrics)
    metrics_after: Metrics = field(default_factory=Metrics)
    metrics_delta: dict = field(default_factory=dict)
    churn: CH.Churn = field(default_factory=CH.Churn)
    in_place_renames: list[str] = field(default_factory=list)
    dangling_pairs: list[MV.Pair] = field(default_factory=list)
    version: int = STRUCTURE_VERSION

    # -- the numbers --------------------------------------------------------
    def totals(self) -> dict:
        """The episode totals, computed from the file facts — never from the rows."""
        pairs = self.pairs
        return {
            "files": len(self.files),
            "files_created": sum(1 for item in self.files
                                 if item.change_type == "added" and item.pair is None),
            "files_deleted": sum(1 for item in self.files
                                 if item.change_type == "deleted" and item.pair is None),
            "files_modified": sum(1 for item in self.files
                                  if item.change_type == "modified"),
            "files_moved": len(pairs),
            "files_moved_confirmed": sum(1 for pair in pairs if pair.tier == "fact"),
            "files_moved_inferred": sum(1 for pair in pairs if pair.tier == "guess"),
            "lines_added": sum(item.lines.added for item in self.files),
            "lines_removed": sum(item.lines.removed for item in self.files),
            "lines_moved": sum(pair.lines_moved for pair in pairs),
            "directories_touched": len(self.active_directories()),
            "directories_added": sum(1 for row in self.rows if row.directory_added),
            "directories_removed": sum(1 for row in self.rows if row.directory_removed),
            "stories": len(self.stories),
            "stories_by_shape": self.stories_by_shape(),
            "pairings_in_place_renamed": len(self.in_place_renames),
        }

    def counts(self) -> dict:
        """`totals()` plus the row sums, so a caller can see they agree without calling verify."""
        counts = dict(self.totals())
        counts["rows"] = len(self.rows)
        counts["row_sums"] = self.row_sums()
        counts["findings"] = len(self.findings)
        counts["findings_by_severity"] = {
            name: sum(1 for finding in self.findings if finding.severity == name)
            for name in SEVERITY_ORDER}
        counts["findings_by_kind"] = self._findings_by_kind()
        counts["findings_suppressed"] = dict(sorted(self.suppressed.items()))
        counts["churn"] = self.churn.score
        counts["churn_band"] = self.churn.band
        counts["tree_basis"] = self.basis
        counts["reconciles"] = not self.verify()
        return counts

    def row_sums(self) -> dict:
        return {"lines_added": sum(row.lines_added for row in self.rows),
                "lines_removed": sum(row.lines_removed for row in self.rows),
                "lines_moved_in": sum(row.lines_moved_in for row in self.rows),
                "lines_moved_out": sum(row.lines_moved_out for row in self.rows),
                "files_created": sum(row.files_created for row in self.rows),
                "files_deleted": sum(row.files_deleted for row in self.rows),
                "files_moved_in": sum(row.files_moved_in for row in self.rows),
                "files_moved_out": sum(row.files_moved_out for row in self.rows),
                "directories_added": sum(1 for row in self.rows if row.directory_added),
                "directories_removed": sum(1 for row in self.rows if row.directory_removed)}

    def verify(self) -> list[str]:
        """Every column of the table against the episode totals. `[]` means it reconciles."""
        totals = self.totals()
        sums = self.row_sums()
        problems: list[str] = []
        for key in ("lines_added", "lines_removed", "files_created", "files_deleted"):
            if sums[key] != totals[key]:
                problems.append("%s: the directory table sums to %s, the episode totals say %s"
                                % (key, sums[key], totals[key]))
        for key in ("lines_moved", "files_moved"):
            if sums[key + "_in"] != totals[key] or sums[key + "_out"] != totals[key]:
                problems.append("%s: table in=%s out=%s, episode total %s"
                                % (key, sums[key + "_in"], sums[key + "_out"], totals[key]))
        if sums["directories_added"] != totals["directories_added"]:
            problems.append("directories_added: rows and totals disagree")
        if sums["directories_removed"] != totals["directories_removed"]:
            problems.append("directories_removed: rows and totals disagree")
        if not 0.0 <= self.churn.score <= 1.0:
            problems.append("churn score %.3f is outside [0, 1]" % self.churn.score)
        for row in self.rows:
            if row.files_after < 0 or row.files_before < 0:
                problems.append("%s: negative file census" % row.display)
        return problems

    def active_directories(self) -> list[str]:
        return sorted(row.path for row in self.rows if row.active)

    def stories_by_shape(self) -> dict[str, int]:
        out: dict[str, int] = {}
        for story in self.stories:
            out[story.shape] = out.get(story.shape, 0) + 1
        return dict(sorted(out.items()))

    def _findings_by_kind(self) -> dict[str, int]:
        out: dict[str, int] = {}
        for finding in self.findings:
            out[finding.kind] = out.get(finding.kind, 0) + 1
        return dict(sorted(out.items()))

    @property
    def basis(self) -> str:
        if self.before.basis == self.after.basis:
            return self.before.basis
        return "mixed: before from %s, after from %s" % (self.before.basis, self.after.basis)

    @property
    def confidence(self) -> float:
        """How much of this answer is measured rather than inferred, in the reviewer's units.

        Three things can weaken it, and all three are printed next to the number rather than
        hidden inside it: pairings that are guesses instead of facts, a caller who lent no
        inventory (so the census is about the change set, not the repository), and — mildly —
        an inventory on only one side. A fully inventoried episode whose every pairing is a
        fact is 1.00; one whose moves were all inferred from bytes and whose census came from
        its own path list is a fraction of that, and prints which discount it got.
        """
        total = len(self.pairs)
        guessed = sum(1 for pair in self.pairs if pair.tier == "guess")
        value = 1.0
        if self.before.basis != BASIS_INVENTORY:
            value -= 0.25
        if self.after.basis != BASIS_INVENTORY:
            value -= 0.10
        if total:
            value -= 0.30 * (guessed / total)
        return round(max(0.3, min(1.0, value)), 2)

    def confidence_reasons(self) -> list[str]:
        """Why the confidence number is what it is, in the order it is computed in."""
        total = len(self.pairs)
        guessed = sum(1 for pair in self.pairs if pair.tier == "guess")
        out: list[str] = []
        if self.before.basis != BASIS_INVENTORY:
            out.append("no before inventory: census and novelty are about the change set "
                       "(-0.25)")
        if self.after.basis != BASIS_INVENTORY:
            out.append("no after inventory (-0.10)")
        if total and guessed:
            out.append("%d of %d pairing(s) inferred from bytes rather than asserted (-%.2f)"
                       % (guessed, total, 0.30 * guessed / total))
        if not out:
            out.append("both inventories supplied and every pairing a fact: nothing discounted")
        return out

    # -- the sentence -------------------------------------------------------
    def summary(self) -> dict:
        """The one-sentence answer, the clauses it is made of, and their evidence."""
        clauses: list[dict] = []
        totals = self.totals()
        limit = int(self.policy.number("max_stories"))
        for story in self.stories[:limit]:
            clauses.append({"text": story.summary_clause(), "evidence": list(story.evidence),
                            "source": "story:%s" % story.shape})
        hidden = len(self.stories) - limit
        if hidden > 0:
            clauses.append({"text": "%d further structural move(s) not shown" % hidden,
                            "evidence": [story.key for story in self.stories[limit:]],
                            "source": "story:cap"})
        if totals["directories_added"] or totals["directories_removed"]:
            added = [row for row in self.rows if row.directory_added]
            removed = [row for row in self.rows if row.directory_removed]
            parts: list[str] = []
            evidence: list[str] = []
            if added:
                parts.append(_plural(len(added), "directory added", "directories added"))
                evidence.append("added: %s" % ", ".join(row.display for row in added[:5]))
            if removed:
                parts.append(_plural(len(removed), "directory removed",
                                     "directories removed"))
                evidence.append("removed: %s" % ", ".join(row.display for row in removed[:5]))
            clauses.append({"text": ", ".join(parts), "evidence": evidence,
                            "source": "tree:novelty"})
        shape_clause = self._shape_clause()
        if shape_clause:
            clauses.append(shape_clause)
        if not self.stories:
            clauses.append(self._local_clause(totals))
        body = "; ".join(clause["text"] for clause in clauses)
        sentence = "%s%s · churn %.3f (%s)." % (
            ("%s · " % self.episode_id) if self.episode_id else "", body,
            self.churn.score, self.churn.band)
        evidence: list[str] = []
        for clause in clauses:
            evidence.append("%s ⟵ %s" % (clause["text"], "; ".join(clause["evidence"])
                                          or "no further evidence"))
        evidence.append("tree basis: %s" % self.basis)
        evidence.append("pairings: %d fact, %d guess; %d file(s) renamed within their own "
                        "directory excluded" % (totals["files_moved_confirmed"],
                                                totals["files_moved_inferred"],
                                                totals["pairings_in_place_renamed"]))
        return {"sentence": sentence, "clauses": clauses, "evidence": evidence,
                "churn": self.churn.score, "band": self.churn.band,
                "stories": [story.shape for story in self.stories]}

    def summary_line(self) -> str:
        return self.summary()["sentence"]

    def _shape_clause(self) -> dict | None:
        delta = self.metrics_delta
        parts: list[str] = []
        evidence: list[str] = []
        if delta.get("max_depth"):
            parts.append("max depth %d→%d" % (self.metrics_before.max_depth,
                                              self.metrics_after.max_depth))
            evidence.append("max_depth delta %+d" % delta["max_depth"])
        if abs(delta.get("mean_depth", 0.0)) >= 0.05:
            parts.append("mean depth %.2f→%.2f" % (self.metrics_before.mean_depth,
                                                   self.metrics_after.mean_depth))
            evidence.append("mean_depth delta %+0.2f" % delta["mean_depth"])
        if abs(delta.get("top_share", 0.0)) >= 0.005:
            parts.append("top-%d %s share %.3f→%.3f"
                         % (delta["top_k"], delta["top_unit"], self.metrics_before.top_share,
                            self.metrics_after.top_share))
            evidence.append("top-%d share delta %+0.3f, counted in %s"
                            % (delta["top_k"], delta["top_share"], delta["top_unit"]))
        if not parts:
            return None
        return {"text": ", ".join(parts), "evidence": evidence or ["no shape metric moved"],
                "source": "metrics:shape"}

    def _local_clause(self, totals: dict) -> dict:
        """What to say when nothing moved: where the change *did* land."""
        rows = [row for row in self.rows if row.active]
        top = sorted(rows, key=lambda row: (-(row.lines_added + row.lines_removed
                                              + row.files_created + row.files_deleted),
                                            row.path))[:_LOCAL_FALLBACK_LIMIT]
        where = ", ".join("`%s`" % (row.path or "./") for row in top)
        more = "" if len(rows) <= _LOCAL_FALLBACK_LIMIT else \
            " (+%d other director%s)" % (len(rows) - _LOCAL_FALLBACK_LIMIT,
                                         "y" if len(rows) - _LOCAL_FALLBACK_LIMIT == 1 else "ies")
        text = ("no directory moved: %d file(s) changed in %d director%s (%s%s), %d line(s) "
                "added, %d removed"
                % (totals["files"], len(rows), "y" if len(rows) == 1 else "ies", where, more,
                   totals["lines_added"], totals["lines_removed"]))
        return {"text": text,
                "evidence": ["%s: +%d/−%d lines, %d created, %d deleted"
                             % (row.display, row.lines_added, row.lines_removed,
                                row.files_created, row.files_deleted) for row in top]
                or ["no active directory"],
                "source": "tree:local"}

    def headline(self) -> str:
        if self.stories:
            return "%s, churn %.3f (%s)" % (
                _plural(len(self.stories), "structural story", "structural stories"),
                self.churn.score, self.churn.band)
        return "no structural story: contents changed in place, churn %.3f (%s)" % (
            self.churn.score, self.churn.band)

    def as_dict(self) -> dict:
        return {"episode_id": self.episode_id, "title": self.title,
                "version": self.version, "summary": self.summary(),
                "headline": self.headline(), "counts": self.counts(),
                "totals": self.totals(), "row_sums": self.row_sums(),
                "reconciles": not self.verify(),
                "verification": self.verify(),
                "tree_basis": self.basis, "confidence": self.confidence,
                "metrics": {"before": self.metrics_before.as_dict(),
                            "after": self.metrics_after.as_dict(),
                            "delta": self.metrics_delta},
                "churn": self.churn.as_dict(),
                "confidence_reasons": self.confidence_reasons(),
                "directories": [row.as_dict() for row in self.rows],
                "files": [item.as_dict() for item in self.files],
                "pairs": [pair.as_dict() for pair in self.pairs],
                "stories": [story.as_dict() for story in self.stories],
                "findings": [finding.as_dict() for finding in self.findings],
                "findings_suppressed": dict(sorted(self.suppressed.items())),
                "in_place_renames": list(self.in_place_renames),
                "dangling_pairings": [pair.as_dict() for pair in self.dangling_pairs],
                "notes": list(self.notes), "partials": list(self.partials),
                "policy": self.policy.as_dict(),
                "config_digest": self.policy.config_digest}

    # -- internals ----------------------------------------------------------
    def add(self, finding: Finding) -> None:
        """Honour the policy, never silently: a switch that hid something says so."""
        enabled, source = self.policy.effective(finding.kind)
        if not enabled:
            self.suppressed[finding.kind] = self.suppressed.get(finding.kind, 0) + 1
            self.suppressed_source[finding.kind] = source
            return
        self.findings.append(finding)


def _plural(count: int, singular: str, plural: str) -> str:
    return "%d %s" % (count, singular if count == 1 else plural)


# ---------------------------------------------------------------------------
# the run
# ---------------------------------------------------------------------------
def measure(episode: Episode, policy: Policy | None = None, classifier=None) -> StructureDelta:
    """Measure one episode's folder structure. The ticket, in one call."""
    active = policy or Policy.built_in()
    view = StructureDelta(episode_id=episode.id, title=episode.title, policy=active)
    code_classes = _code_classes(active)
    clf = classifier
    view.files = [_inspect(change, clf, code_classes) for change in
                  sorted(episode.files, key=lambda item: (item.subject, item.old_subject))]
    view.pairs, in_place_pairs, pair_notes = MV.pair_episode(episode, active)
    # A pairing whose other half is not in this change set cannot be counted: one of its two
    # directories would be credited with movement the file list cannot show. It is kept as a
    # stated partial instead of being quietly believed.
    known_new = {change.subject for change in episode.files}
    known_old = {change.old_subject or change.subject for change in episode.files}
    view.dangling_pairs = [pair for pair in view.pairs
                           if pair.old_path not in known_old or pair.new_path not in known_new]
    if view.dangling_pairs:
        view.pairs = [pair for pair in view.pairs if pair not in view.dangling_pairs]
        for pair in view.dangling_pairs:
            view.partials.append(
                "pairing `%s` → `%s` names a path outside this change set: it is reported here "
                "and not counted, because crediting a directory this file list cannot see would "
                "make the table stop summing" % (pair.old_path, pair.new_path))
    view.in_place_renames = ["%s → %s" % (pair.old_path, pair.new_path)
                             for pair in in_place_pairs]
    view.notes.extend(pair_notes)
    _merge_pairs(view, view.pairs, structure=True)
    _merge_pairs(view, in_place_pairs, structure=False)
    view.before, view.after = _build_trees(episode, view, code_classes)
    _novelty(view)
    view.metrics_before = view.before.metrics(top_k=active.integer("top_dirs"))
    view.metrics_after = view.after.metrics(top_k=active.integer("top_dirs"))
    view.metrics_delta = delta_metrics(view.metrics_before, view.metrics_after)
    view.stories = MV.find_stories(view.pairs, view.before, view.after, active)
    view.churn = CH.score(files_touched=view.totals()["files"],
                          files_moved=view.totals()["files_moved"],
                          lines_added=view.totals()["lines_added"],
                          lines_removed=view.totals()["lines_removed"],
                          lines_moved=view.totals()["lines_moved"],
                          directories_touched=view.totals()["directories_touched"],
                          directories_added=view.totals()["directories_added"],
                          directories_removed=view.totals()["directories_removed"],
                          stories=len(view.stories),
                          mean_depth_delta=view.metrics_delta["mean_depth"],
                          max_depth_delta=view.metrics_delta["max_depth"],
                          top_share_delta=view.metrics_delta["top_share"],
                          policy=active)
    _collect_findings(view)
    view.findings.sort(key=sort_key)
    view.partials = _partials(view, episode)
    return view


def _code_classes(policy: Policy) -> tuple[str, ...]:
    return tuple(item for item in policy.words("code_classes")) or ("source", "test")


def _merge_pairs(view: StructureDelta, pairs: list[MV.Pair], *, structure: bool) -> None:
    """Fold a delete-plus-create pairing into one entry in the file list.

    Two reasons, one shape. A structural pairing has to stop reading as 1 file when git
    asserted the rename and 2 files (a deletion beside a creation) when this package inferred
    it — otherwise the table, the totals and the churn score would depend on whether the
    caller's rename detection happened to run. An in-place rename has to stop reading as a
    deletion beside a creation too, but it must not become a move, because no directory gained
    or lost anything. So structural pairs come back as `renamed` entries carrying their
    pairing; in-place pairs come back as `modified` entries with `old_path` kept and no
    pairing attached. Either way the lines that travelled are not also counted as added and
    removed.
    """
    by_new = {item.path: item for item in view.files}
    by_old = {(item.old_path or item.path): item for item in view.files}
    dropped: set[str] = set()
    for pair in pairs:
        new_fact = by_new.get(pair.new_path)
        old_fact = by_old.get(pair.old_path)
        if new_fact is None or old_fact is None:
            # `measure` already pulled these out into `dangling_pairs` and said so; nothing to
            # fold here.
            continue
        if new_fact is not old_fact:
            new_fact.old_path = pair.old_path
            new_fact.change_type = "renamed" if structure else "modified"
            new_fact.lines_before = old_fact.lines_before
            dropped.add(old_fact.path)
        # A `renamed` entry in the change list is its own before and after, so `new_fact is
        # old_fact` here: that is the ordinary shape of an asserted rename, not a partial.
        new_fact.lines = LineStats(added=pair.lines_added, removed=pair.lines_removed,
                                   common=pair.lines_moved,
                                   total_before=old_fact.lines_before or 0,
                                   total_after=new_fact.lines_after or 0, basis=pair.basis)
        if structure:
            new_fact.pair = pair
    if dropped:
        view.files = [item for item in view.files if item.path not in dropped]
        view.files.sort(key=lambda item: (item.path, item.old_path))


def _inspect(change, classifier, code_classes: tuple[str, ...]) -> FileFact:
    fact = FileFact(path=change.subject, old_path=change.old_subject,
                    change_type=change.change_type)
    cls = _classify(classifier, change)
    fact.cls = cls
    fact.code = cls in code_classes
    stats = line_stats(change.before, change.after, lines_before=change.lines_before,
                       lines_after=change.lines_after)
    fact.lines = stats
    fact.lines_before = total_lines(change.before, change.lines_before)
    fact.lines_after = total_lines(change.after, change.lines_after)
    if stats.basis == "none":
        fact.notes.append("no content and no stored line counts: this file contributes "
                          "0 added/removed/travelled lines rather than an estimate")
    if change.change_type == "added" and fact.lines_before:
        fact.notes.append("classified as added but carries a before revision")
    return fact


def _classify(classifier, change) -> str:
    if classifier is None:
        from recommender.taxonomy import default_taxonomy
        return default_taxonomy().classify(change.subject).cls
    from recommender.change.engine import Change as BlobChange
    decision = classifier.classify(BlobChange(path=change.path or change.old_path,
                                              content=change.content,
                                              change_type=change.change_type,
                                              old_path=change.old_path))
    return decision.cls


def _build_trees(episode: Episode, view: StructureDelta,
                 code_classes: tuple[str, ...]) -> tuple[Tree, Tree]:
    """The two inventories, from the caller's paths when it has them and from the episode when not."""
    facts_by_new = {item.path: item for item in view.files}
    facts_by_old: dict[str, FileFact] = {}
    for item in view.files:
        facts_by_old[item.old_path or item.path] = item
    notes: list[str] = []

    if episode.before_paths:
        before_basis = BASIS_INVENTORY
        before_entries = {}
        for path in sorted(set(episode.before_paths)):
            fact = facts_by_old.get(path)
            before_entries[path] = FileEntry(
                path=path, lines=_before_lines(fact), code=_is_code(fact, code_classes),
                cls=fact.cls if fact else "other", changed=fact is not None)
    else:
        before_basis = BASIS_EPISODE
        before_entries = {}
        for path, fact in facts_by_old.items():
            if fact.change_type == "added":
                continue
            before_entries[path] = FileEntry(path=path, lines=_before_lines(fact),
                                             code=fact.code, cls=fact.cls, changed=True)
        notes.append("no before inventory was supplied: directory censuses and \"new "
                     "directory\" claims are about this episode's file list, not the "
                     "repository")

    if episode.after_paths:
        after_basis = BASIS_INVENTORY
        after_entries = {}
        for path in sorted(set(episode.after_paths)):
            fact = facts_by_new.get(path)
            after_entries[path] = FileEntry(
                path=path, lines=_after_lines(fact), code=_is_code(fact, code_classes),
                cls=fact.cls if fact else "other", changed=fact is not None)
    else:
        after_basis = BASIS_EPISODE
        after_entries = {}
        for path, fact in facts_by_new.items():
            if fact.change_type == "deleted":
                continue
            after_entries[path] = FileEntry(path=path, lines=_after_lines(fact),
                                            code=fact.code, cls=fact.cls, changed=True)
        notes.append("no after inventory was supplied: \"this directory holds N files\" means "
                     "N files in this change set")
    view.notes.extend(notes)
    return build_trees(before_entries, after_entries, before_basis=before_basis,
                       after_basis=after_basis)


def _is_code(fact: FileFact | None, code_classes: tuple[str, ...]) -> bool:
    return bool(fact) and fact.cls in code_classes


def _before_lines(fact: FileFact | None) -> int | None:
    return fact.lines_before if fact else None


def _after_lines(fact: FileFact | None) -> int | None:
    return fact.lines_after if fact else None


def _novelty(view: StructureDelta) -> None:
    """Build the directory table: one row per directory on either side, plus novelty flags."""
    before_census = view.before.census()
    after_census = view.after.census()
    directories = sorted(set(before_census) | set(after_census))
    rows: dict[str, DirRow] = {}
    for directory in directories:
        before_row: DirCensus | None = before_census.get(directory)
        after_row: DirCensus | None = after_census.get(directory)
        rows[directory] = DirRow(
            path=directory,
            depth=(after_row or before_row).depth,
            files_before=before_row.files if before_row else 0,
            files_after=after_row.files if after_row else 0,
            code_lines_before=before_row.code_lines if before_row else 0,
            code_lines_after=after_row.code_lines if after_row else 0,
            subdirectories_before=before_row.subdirs if before_row else 0,
            subdirectories_after=after_row.subdirs if after_row else 0,
            directory_added=before_row is None and after_row is not None,
            directory_removed=before_row is not None and after_row is None,
            basis=view.basis)
    for fact in view.files:
        destination = rows.setdefault(fact.destination, DirRow(path=fact.destination,
                                                              depth=depth_of(
                                                                  fact.destination + "/x")))
        if fact.change_type == "added":
            destination.files_created += 1
            destination.lines_added += fact.lines.added
        elif fact.change_type == "deleted":
            origin = rows.setdefault(fact.origin, DirRow(path=fact.origin,
                                                         depth=depth_of(fact.origin + "/x")))
            origin.files_deleted += 1
            origin.lines_removed += fact.lines.removed
        elif fact.is_move:
            destination.lines_added += fact.lines.added
            destination.lines_removed += fact.lines.removed
            destination.lines_moved_in += fact.pair.lines_moved if fact.pair else 0
            destination.files_moved_in += 1
            origin = rows.setdefault(fact.origin, DirRow(path=fact.origin,
                                                         depth=depth_of(fact.origin + "/x")))
            origin.files_moved_out += 1
            origin.lines_moved_out += fact.pair.lines_moved if fact.pair else 0
        else:
            destination.lines_added += fact.lines.added
            destination.lines_removed += fact.lines.removed
    view.rows = [rows[key] for key in sorted(rows)]


def _collect_findings(view: StructureDelta) -> None:
    active = view.policy
    for story in view.stories:
        kind = _story_kind(story)
        if kind is None:
            continue
        view.add(Finding(
            kind=kind, name=story.name(), path=anchor(story.source),
            detail=story.detail, shape=story.shape, tier=story.tier,
            confidence=story.confidence,
            evidence=list(story.evidence),
            numbers={"moved_files": story.moved_files, "moved_lines": story.moved_lines,
                     "levels": story.levels, "destinations": list(story.destinations)}))
    for row in view.rows:
        if row.directory_removed:
            view.add(Finding(
                kind=DIRECTORY_REMOVED, name=row.display, path=anchor(row.path),
                detail="%d file(s) were here and none remain" % row.files_before,
                evidence=["%d file(s) before, %d after (tree basis: %s)"
                          % (row.files_before, row.files_after, row.basis)]
                + ["%d file(s) deleted here, %d moved out"
                   % (row.files_deleted, row.files_moved_out)]))
        if row.directory_added and row.files_after:
            view.add(Finding(
                kind=DIRECTORY_ADDED, name=row.display, path=anchor(row.path),
                detail="a new directory now holds %d file(s)" % row.files_after,
                evidence=["%d file(s) created here, %d moved in (tree basis: %s)"
                          % (row.files_created, row.files_moved_in, row.basis)]))
    guesses = sorted((pair for pair in view.pairs if pair.tier == "guess"),
                     key=lambda pair: (pair.old_path, pair.new_path))
    if guesses:
        view.add(Finding(
            kind=MOVE_UNCONFIRMED,
            name="%d inferred pairing(s)" % len(guesses),
            path=guesses[0].new_path,
            detail="no caller pairing covered these; they were inferred from bytes and are "
                   "not git's fact tier",
            evidence=[pair.evidence() for pair in guesses[:int(active.number("max_evidence"))]],
            numbers={"inferred": len(guesses)}))
    delta = view.metrics_delta
    if delta.get("max_depth", 0) >= int(active.number("depth_max_delta")) or \
            delta.get("mean_depth", 0.0) >= float(active.number("depth_mean_delta")):
        view.add(Finding(
            kind=NESTING_DEEPENED, name="tree depth", path="./",
            detail="mean directory depth grew %.2f levels (max %d → %d)"
                   % (delta["mean_depth"], view.metrics_before.max_depth,
                      view.metrics_after.max_depth),
            evidence=["mean depth %.2f → %.2f, max %d → %d (tree basis: %s)"
                      % (view.metrics_before.mean_depth, view.metrics_after.mean_depth,
                         view.metrics_before.max_depth, view.metrics_after.max_depth,
                         view.basis)],
            numbers=delta))
    if delta.get("top_share", 0.0) >= float(active.number("concentration_delta")):
        view.add(Finding(
            kind=CONCENTRATION_UP,
            name="top-%d code share" % delta["top_k"],
            path="./",
            detail="top-%d directories now hold %s of code, up %s"
                   % (delta["top_k"], _pct(view.metrics_after.top_share),
                      _pct(delta["top_share"])),
            evidence=["top-%d %s: %s → %s (counted in %s)"
                      % (delta["top_k"], _pct(view.metrics_before.top_share),
                         _pct(view.metrics_after.top_share), delta["top_unit"], view.basis)],
            numbers=delta))
    if view.churn.score >= float(active.number("band_structural")):
        view.add(Finding(
            kind=CHURN_HIGH, name="structure churn %.3f" % view.churn.score, path="./",
            detail="the episode moved the tree itself, not just what lives in it",
            evidence=[("component %s: %s = %.3f × %.2f"
                       % (component.name, component.reading, component.value,
                          component.weight)) for component in view.churn.components],
            numbers={"score": view.churn.score, "band": view.churn.band}))
    view.findings.sort(key=sort_key)


def _story_kind(story):
    """Which finding a story raises. A story that raises nothing is a bug, not a design choice."""
    return {SHAPE_PACKAGE_RENAME: PACKAGE_RENAMED, SHAPE_SUBTREE_MOVE: SUBTREE_MOVED,
            SHAPE_MODULE_SPLIT: MODULE_SPLIT, SHAPE_FLATTEN: DIRECTORY_FLATTENED,
            SHAPE_FILES_MOVED: FILES_MOVED}.get(story.shape)


def _partials(view: StructureDelta, episode: Episode) -> list[str]:
    out = list(view.partials)
    if not episode.before_paths or not episode.after_paths:
        out.append("directory census is scoped to the change set: pass `before_paths` / "
                   "`after_paths` (a tree's file inventory is cheap to supply) for "
                   "repository-wide claims")
    blind = [component.name for component in view.churn.components if component.blind]
    if blind:
        out.append("churn component(s) not measurable from what the caller supplied: %s"
                   % ", ".join(blind))
    if view.metrics_after.code_basis != "code lines":
        out.append("top-%d share is counted in files, not lines, because some code file's line "
                   "count was unknown to the caller" % view.metrics_after.top_k)
    return out


def _pct(value: float) -> str:
    return "%.1f%%" % (value * 100.0)


def aggregate(episodes, *, policy: Policy | None = None, classifier=None) -> dict:
    """Roll several episodes up — the shape `e2.10`'s card row and `e4.4`'s risk input want.

    Churn is summed as a mean weighted by files touched, never by episode: a two-file episode
    that moved one file would otherwise count as half as structural as a two-hundred-file
    reshuffle.
    """
    active = policy or Policy.built_in()
    totals = {"episodes": 0, "files": 0, "files_moved": 0, "lines_added": 0,
              "lines_removed": 0, "lines_moved": 0, "directories_added": 0,
              "directories_removed": 0, "stories": 0, "findings": 0,
              "churn_structural": 0, "churn_transitional": 0, "churn_local": 0}
    weighted = 0.0
    weight = 0.0
    shapes: dict[str, int] = {}
    directories: dict[str, dict] = {}
    bases: set[str] = set()
    for episode in episodes:
        view = measure(episode, active, classifier)
        counts = view.counts()
        bases.add(view.basis)
        totals["episodes"] += 1
        if view.churn.band == "structural":
            totals["churn_structural"] += 1
        elif view.churn.band == "transitional":
            totals["churn_transitional"] += 1
        else:
            totals["churn_local"] += 1
        for key, source in (("files", "files"), ("files_moved", "files_moved"),
                            ("lines_added", "lines_added"),
                            ("lines_removed", "lines_removed"),
                            ("lines_moved", "lines_moved"),
                            ("directories_added", "directories_added"),
                            ("directories_removed", "directories_removed"),
                            ("stories", "stories"), ("findings", "findings")):
            totals[key] += counts[source]
        for shape, count in view.totals()["stories_by_shape"].items():
            shapes[shape] = shapes.get(shape, 0) + count
        weighted += view.churn.score * max(1, counts["files"])
        weight += max(1, counts["files"])
        for row in view.rows:
            merged = directories.setdefault(row.path, {
                "directory": row.display, "path": row.path,
                "lines_added": 0, "lines_removed": 0, "lines_moved_in": 0,
                "lines_moved_out": 0, "files_created": 0, "files_deleted": 0,
                "files_moved_in": 0, "files_moved_out": 0, "episodes": 0})
            for key in ("lines_added", "lines_removed", "lines_moved_in", "lines_moved_out",
                        "files_created", "files_deleted", "files_moved_in", "files_moved_out"):
                merged[key] += getattr(row, key)
            merged["episodes"] += 1
    totals["stories_by_shape"] = dict(sorted(shapes.items()))
    totals["churn_mean"] = round(weighted / weight, 3) if weight else 0.0
    totals["basis"] = bases.pop() if len(bases) == 1 else "mixed across episodes"
    totals["directories"] = sorted(directories.values(),
                                   key=lambda row: (-row["lines_moved_in"] - row["lines_added"]
                                                    - row["lines_removed"], row["path"]))
    totals["reconciles"] = (sum(row["lines_moved_in"] for row in directories.values())
                            == totals["lines_moved"]
                            and sum(row["lines_added"] for row in directories.values())
                            == totals["lines_added"])
    return totals
