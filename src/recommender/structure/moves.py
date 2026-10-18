"""Which file became which file, and the shapes a set of those pairings makes.

Two jobs, in strict order of trust:

**Pairing.** A pairing is `(old_path, new_path)` with a confidence and a tier, and this
package gets them from three places, best first:

1. the change list itself — a `renamed` change says so, and is taken as asserted;
2. pairings the caller hands in as :class:`recommender.structure.model.Move`, which is how
   `e2.4`'s fact tier feeds this ticket. A handed-over pairing counts as a **fact** only at
   or above `structure.move.fact_bar`, and the card prints the tier it got. `e2.4`'s spike
   measured 0.75 as the bar below which git asserts a pairing that is not a move, so this
   ticket refuses to be more trusting than the ticket it consumes;
3. this package's own inference from bytes: same stem, or content that is 0.9 alike, and in
   any case line-set Jaccard at or above `structure.move.min_similarity`, matched 1:1
   greedily in a fixed order (highest score, then old path, then new path). Every one of
   those is a **guess** and is labelled, and `move-unconfirmed` says which parts of the
   answer rest on them. Turning `structure.move.infer_content` off makes the answer
   strictly narrower rather than guessing harder.

Pairing is what the ticket's own spike (`s4`) said it is: a wrong pairing costs more reviewer
trust than a missing one, so an unresolved delete beside an unresolved create stays unresolved
— the directory table then counts a deletion and a creation, which is dull and true, instead
of inventing a move that makes a nicer sentence.

**Shapes.** Once pairings exist, the four shapes the ticket names are *groupings* of them,
each with a rule the reviewer can check (`fathom structure shapes` prints them with the
numbers in force): a whole directory's files arriving at one destination with their relative
paths intact is a `package-rename` or a `subtree-move` depending on which half of the path
changed; the same files arriving in two or more destinations is a `module-split`; a directory
whose files climb into an ancestor and leave it empty is a `directory-flatten`. A story's
confidence is its **weakest** pairing, because a story that is nine confirmed files and one
inferred one is an inferred story.

Files that change name inside their own directory are deliberately *not* structure: no
directory gained or lost anything, so they are counted as ordinary modifications and the
card says they were excluded.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .model import (Move, SOURCE_GIT, TIER_FACT, TIER_GUESS, depth_of, directory_of,
    is_under, leaf, line_stats, normalise, relative_to, similarity)
from .policy import (SHAPE_FILES_MOVED, SHAPE_FLATTEN, SHAPE_MODULE_SPLIT,
                     SHAPE_PACKAGE_RENAME, SHAPE_SUBTREE_MOVE, Policy)
from .tree import Tree

#: Content similarity at which a pairing is accepted even under a different stem.
SAME_CONTENT = 0.9


@dataclass(frozen=True)
class Pair:
    """One resolved file-to-file pairing, with the line movement that travelled with it."""

    old_path: str
    new_path: str
    lines_moved: int = 0
    lines_added: int = 0
    lines_removed: int = 0
    confidence: float = 1.0
    source: str = "caller"
    tier: str = TIER_FACT
    score: float = 0.0
    basis: str = "diff"

    @property
    def key(self) -> str:
        return "%s\u2192%s" % (self.old_path, self.new_path)

    @property
    def changed_directory(self) -> bool:
        return directory_of(self.old_path) != directory_of(self.new_path)

    @property
    def levels(self) -> int:
        return depth_of(self.new_path) - depth_of(self.old_path)

    def tier_phrase(self) -> str:
        return "%s, %s" % (self.tier, self.source)

    def evidence(self) -> str:
        text = "%s → %s · %d line(s) travelled" % (self.old_path, self.new_path,
                                                   self.lines_moved)
        if self.basis != "diff":
            text += " (%s)" % self.basis
        text += " (%s" % self.tier_phrase()
        if self.score:
            text += ", similarity %.2f" % self.score
        return text + ")"

    def as_dict(self) -> dict:
        return {"old_path": self.old_path, "new_path": self.new_path,
                "lines_moved": self.lines_moved, "lines_added": self.lines_added,
                "lines_removed": self.lines_removed, "confidence": round(self.confidence, 3),
                "source": self.source, "tier": self.tier, "score": round(self.score, 3),
                "basis": self.basis, "changed_directory": self.changed_directory}


@dataclass
class Story:
    """One structural story: a shape, the directories it joins, and the numbers behind it."""

    shape: str
    source: str = ""
    destinations: list[str] = field(default_factory=list)
    pairs: list[Pair] = field(default_factory=list)
    levels: int = 0
    relation: str = ""
    detail: str = ""
    evidence: list[str] = field(default_factory=list)
    source_files_before: int = 0
    source_files_after: int = 0

    @property
    def moved_files(self) -> int:
        return len(self.pairs)

    @property
    def moved_lines(self) -> int:
        return sum(pair.lines_moved for pair in self.pairs)

    @property
    def lines_added(self) -> int:
        return sum(pair.lines_added for pair in self.pairs)

    @property
    def lines_removed(self) -> int:
        return sum(pair.lines_removed for pair in self.pairs)

    @property
    def confidence(self) -> float:
        if not self.pairs:
            return 1.0
        return min(pair.confidence for pair in self.pairs)

    @property
    def tier(self) -> str:
        return TIER_FACT if all(pair.tier == TIER_FACT for pair in self.pairs) else TIER_GUESS

    @property
    def key(self) -> str:
        return "%s|%s|%s" % (self.shape, self.source, ",".join(sorted(self.destinations)))

    def name(self) -> str:
        if self.shape == SHAPE_MODULE_SPLIT:
            return "%s split into %s" % (_display(self.source),
                                         _and([_display(item) for item in
                                               self.destinations]))
        if self.shape == SHAPE_FILES_MOVED:
            return "%d file(s) %s → %s" % (len(self.pairs), _display(self.source),
                                           _display(self.destinations[0]))
        return "%s → %s" % (_display(self.source), _display(self.destinations[0]))

    def summary_clause(self) -> str:
        """The clause this story contributes to the one-sentence summary.

        Fixed wording per shape, numbers first-hand from the pairs, no prose that could make
        a claim the numbers do not support — which is what lets the corpus hand-write the
        expected sentence instead of blessing whatever came out.
        """
        src = self.source or "./"
        dest = (self.destinations[0] if self.destinations else "./") or "./"
        if self.shape == SHAPE_MODULE_SPLIT:
            arms = ["`%s`" % ((item or "./")) for item in self.destinations]
            head = "`%s` was split into %s" % (src, _and(arms))
        elif self.shape == SHAPE_FILES_MOVED:
            head = "%d file(s) moved from `%s` to `%s`" % (self.moved_files, src, dest)
        elif self.shape == SHAPE_PACKAGE_RENAME:
            head = "`%s` was renamed to `%s`" % (src, dest)
        elif self.shape == SHAPE_SUBTREE_MOVE:
            head = "`%s` moved to `%s`" % (src, dest)
        else:
            head = "`%s` was flattened into `%s`" % (src, dest)
        tail = "%d file(s), %s" % (self.moved_files, self._line_phrase())
        if self.shape in (SHAPE_PACKAGE_RENAME, SHAPE_SUBTREE_MOVE, SHAPE_FLATTEN):
            tail += ", directory shape preserved"
        return "%s (%s)" % (head, tail)

    def _line_phrase(self) -> str:
        bits = []
        if self.moved_lines:
            bits.append("%d line(s) travelled with them" % self.moved_lines)
        else:
            bits.append("no line content available to count")
        if self.lines_added:
            bits.append("%d line(s) added on the way" % self.lines_added)
        if self.lines_removed:
            bits.append("%d line(s) dropped on the way" % self.lines_removed)
        return ", ".join(bits)

    def as_dict(self) -> dict:
        return {"shape": self.shape, "source": self.source or "./",
                "destinations": [item or "./" for item in self.destinations],
                "moved_files": self.moved_files, "moved_lines": self.moved_lines,
                "lines_added": self.lines_added, "lines_removed": self.lines_removed,
                "levels": self.levels, "relation": self.relation, "tier": self.tier,
                "confidence": round(self.confidence, 3), "detail": self.detail,
                "evidence": list(self.evidence), "name": self.name(),
                "clause": self.summary_clause(),
                "source_files_before": self.source_files_before,
                "source_files_after": self.source_files_after,
                "pairs": [pair.as_dict() for pair in self.pairs]}


def _display(directory: str) -> str:
    return (directory or "./") + ("/" if directory else "")


def _and(items: list[str]) -> str:
    """`a, b and c`, deterministically, because the summary sentence is a tested artefact."""
    if not items:
        return ""
    if len(items) == 1:
        return items[0]
    return ", ".join(items[:-1]) + " and " + items[-1]


# ---------------------------------------------------------------------------
# pairing
# ---------------------------------------------------------------------------
def pair_episode(episode, policy: Policy) -> tuple[list[Pair], list[str], list[str]]:
    """Resolve every pairing for one episode: (pairs, in-place renames, notes)."""
    notes: list[str] = []
    pairs: list[Pair] = []
    claimed_old: set[str] = set()
    claimed_new: set[str] = set()
    by_path = {change.subject: change for change in episode.files}
    old_paths = {change.old_subject: change for change in episode.files}

    for change in sorted(episode.files, key=lambda item: (item.old_subject, item.subject)):
        if not change.is_move:
            continue
        if change.old_subject in claimed_old or change.subject in claimed_new:
            notes.append("duplicate pairing for `%s` ignored" % change.old_subject)
            continue
        claimed_old.add(change.old_subject)
        claimed_new.add(change.subject)
        pairs.append(_pair_from_change(change, policy))

    supplied = sorted(episode.moves, key=lambda move: (move.old_path, move.new_path))
    for move in supplied:
        old = normalise(move.old_path)
        new = normalise(move.new_path)
        if old in claimed_old or new in claimed_new:
            notes.append("caller pairing `%s` → `%s` refused: one of the two paths is "
                         "already paired by the change list" % (old, new))
            continue
        claimed_old.add(old)
        claimed_new.add(new)
        change_new = by_path.get(new)
        change_old = old_paths.get(old)
        pairs.append(_pair_from_move(move, change_old, change_new, policy))

    unpaired_old = sorted(set(_deleted_paths(episode)) - claimed_old)
    deleted = unpaired_old
    added = sorted(set(_added_paths(episode)) - claimed_new)
    if policy.flag("infer_moves"):
        inferred, extra_notes = infer_pairs(episode, deleted, added, policy)
        pairs.extend(inferred)
        notes.extend(extra_notes)
    elif deleted and added:
        notes.append("%d deletion(s) and %d creation(s) were left unpaired because "
                     "`structure.move.infer_content` is off: they are counted as a deletion "
                     "beside a creation, which is narrower and true"
                     % (len(deleted), len(added)))

    # Renames inside one directory are filtered last, so the inferred ones are covered too:
    # no directory gained or lost anything, so this ticket has nothing to report about them
    # beyond the fact that it excluded them.
    in_place = [pair for pair in pairs if not pair.changed_directory]
    pairs = [pair for pair in pairs if pair.changed_directory]
    if in_place:
        notes.append("%d file(s) changed name inside their own directory: counted as "
                     "modifications, not as structure, because no directory gained or lost "
                     "anything (%s)" % (len(in_place),
                                        ", ".join(sorted(pair.key for pair in in_place)[:3])
                                        + ("…" if len(in_place) > 3 else "")))
    return sorted(pairs, key=lambda pair: (pair.old_path, pair.new_path)), in_place, notes


def _pair_from_change(change, policy: Policy) -> Pair:
    stats = line_stats(change.before, change.after, lines_before=change.lines_before,
                       lines_after=change.lines_after)
    return Pair(old_path=change.old_subject, new_path=change.subject,
                lines_moved=stats.common, lines_added=stats.added,
                lines_removed=stats.removed, confidence=1.0, source="caller",
                tier=TIER_FACT, score=round(similarity(change.before, change.after), 3),
                basis=stats.basis)


def _pair_from_move(move: Move, change_old, change_new, policy: Policy) -> Pair:
    before = change_old.content if change_old is not None else None
    after = change_new.content if change_new is not None else None
    if before is None and change_old is not None:
        before = change_old.before
    if after is None and change_new is not None:
        after = change_new.after
    stats = line_stats(before, after)
    # The tier is this package's judgement, not the caller's. A caller may hand over a pairing
    # and call it a fact; if it does not clear `structure.move.fact_bar` it is a guess here, for
    # the same reason spike s4 refused git's own 0.50 tier: a row a reviewer is asked to trust as
    # git's word has to be one git would still sign.
    fact = (move.confidence >= float(policy.number("move_fact_bar"))
            and (move.tier == TIER_FACT or move.source == SOURCE_GIT))
    return Pair(old_path=normalise(move.old_path), new_path=normalise(move.new_path),
                lines_moved=stats.common or move.lines, lines_added=stats.added,
                lines_removed=stats.removed, confidence=move.confidence, source=move.source,
                tier=TIER_FACT if fact else TIER_GUESS,
                score=round(similarity(before, after), 3), basis=stats.basis)


def infer_pairs(episode, deleted: list[str], added: list[str],
                policy: Policy) -> tuple[list[Pair], list[str]]:
    """Greedy 1:1 pairing of this episode's deletions against its creations."""
    notes: list[str] = []
    before = {}
    after = {}
    for change in episode.files:
        if change.subject in deleted:
            before[change.subject] = change.before
        if change.subject in added:
            after[change.subject] = change.after
    floor = float(policy.number("move_min_similarity"))
    candidates: list[tuple[float, str, str]] = []
    for old in deleted:
        for new in added:
            score = similarity(before.get(old), after.get(new))
            same_stem = leaf(old).split(".")[0] == leaf(new).split(".")[0]
            if score < floor and score < SAME_CONTENT:
                continue
            if not same_stem and score < SAME_CONTENT:
                continue
            candidates.append((score, old, new))
    candidates.sort(key=lambda item: (-item[0], item[1], item[2]))
    used_old: set[str] = set()
    used_new: set[str] = set()
    pairs: list[Pair] = []
    for score, old, new in candidates:
        if old in used_old or new in used_new:
            continue
        used_old.add(old)
        used_new.add(new)
        stats = line_stats(before.get(old), after.get(new))
        pairs.append(Pair(old_path=old, new_path=new, lines_moved=stats.common,
                          lines_added=stats.added, lines_removed=stats.removed,
                          confidence=round(max(0.01, min(1.0, score)), 3),
                          source="content", tier=TIER_GUESS, score=round(score, 3),
                          basis=stats.basis))
    leftovers_old = sorted(set(deleted) - used_old)
    leftovers_new = sorted(set(added) - used_new)
    if leftovers_old and leftovers_new:
        notes.append("%d deletion(s) had no creation above `structure.move.min_similarity` "
                     "= %s with the same stem: counted as a deletion, not as a move (%s)"
                     % (len(leftovers_old), _num(floor), ", ".join(leftovers_old[:3])
                        + ("…" if len(leftovers_old) > 3 else "")))
    return pairs, notes


def _deleted_paths(episode) -> list[str]:
    out = []
    for change in episode.files:
        if change.change_type == "deleted":
            out.append(change.subject)
        elif change.is_move:
            out.append(change.old_subject)
    return sorted(set(out))


def _added_paths(episode) -> list[str]:
    out = []
    for change in episode.files:
        if change.change_type == "added":
            out.append(change.subject)
        elif change.is_move:
            out.append(change.subject)
    return sorted(set(out))


# ---------------------------------------------------------------------------
# shapes
# ---------------------------------------------------------------------------
def find_stories(pairs: list[Pair], before: Tree, after: Tree,
                 policy: Policy) -> list[Story]:
    """Group pairings into the named shapes, in a fixed three-pass order.

    The order is the algorithm, so it is worth spelling out. Everything runs
    **shallowest-source-first**, so one whole-package move is told as one story rather than
    one story per folder inside it. Within that, three passes:

    1. **splits** — the only shape whose destinations are several, so it has to be tested
       before the single-destination move or a split reads as "most of the directory moved
       somewhere" and loses the arm that did not fit. A split's arm is built only from files
       that sat **directly** in the source directory: that is what separates a module being
       re-bucketed from four sibling folders each moving whole under a new parent, which is
       four subtree moves and not one split of their common parent.
    2. **whole-directory moves** — single destination, relative paths preserved, source
       emptied (unless `structure.move.require_emptied` is off).
    3. **leftovers** — whatever pairs remain, grouped by origin → destination, reported as
       `files-moved` rather than dropped.

    A pair is consumed by the first story that claims it, and the passes share one `remaining`
    map, so nothing is counted twice and nothing disappears.
    """
    remaining = {pair.key: pair for pair in pairs}
    stories: list[Story] = []
    candidates = sorted(_candidate_sources(pairs), key=lambda item: (item.count("/"), item))
    for attempt in (_split_story, _whole_directory_story):
        for source in candidates:
            mine = [pair for pair in remaining.values() if _under(pair.old_path, source)]
            if len(mine) < int(policy.number("move_min_files")):
                continue
            story = attempt(source, mine, before, after, policy)
            if story is None:
                continue
            for pair in story.pairs:
                remaining.pop(pair.key, None)
            stories.append(story)
    if remaining:
        stories.extend(_loose_stories(sorted(remaining.values(),
                                             key=lambda pair: (pair.old_path, pair.new_path)),
                                      policy))
    order = {SHAPE_PACKAGE_RENAME: 0, SHAPE_SUBTREE_MOVE: 1, SHAPE_MODULE_SPLIT: 2,
             SHAPE_FLATTEN: 3, SHAPE_FILES_MOVED: 4}
    return sorted(stories, key=lambda story: (order.get(story.shape, 9), story.source,
                                              tuple(story.destinations)))


def _candidate_sources(pairs: list[Pair]) -> list[str]:
    out: set[str] = set()
    for pair in pairs:
        directory = directory_of(pair.old_path)
        while directory:
            out.add(directory)
            parent = directory_of(directory)
            directory = parent if parent != directory else ""
    return sorted(out)


def _under(path: str, directory: str) -> bool:
    return is_under(path, directory) if directory else True


def _shape_preserving_groups(mine: list[Pair], source: str) -> dict[str, list[Pair]]:
    """Pairings whose new path keeps the source-relative path, grouped by implied destination."""
    groups: dict[str, list[Pair]] = {}
    for pair in mine:
        relative = relative_to(pair.old_path, source)
        if not relative or not pair.new_path.endswith(relative):
            continue
        destination = pair.new_path[: len(pair.new_path) - len(relative)].rstrip("/")
        if destination == source:
            continue
        groups.setdefault(destination, []).append(pair)
    return groups


def _whole_directory_story(source: str, mine: list[Pair], before: Tree, after: Tree,
                           policy: Policy) -> Story | None:
    """A whole directory arriving at one destination with its inner folders intact."""
    groups = _shape_preserving_groups(mine, source)
    if not groups:
        return None
    destination, group = max(sorted(groups.items()), key=lambda item: (len(item[1]), item[0]))
    before_files = len(before.files_under(source)) if source else len(before.entries)
    if not before_files:
        return None
    if len(group) < int(policy.number("move_min_files")):
        return None
    share = len(group) / before_files
    after_files = len(after.files_under(source)) if source else len(after.entries)
    emptied = after_files == 0
    if share < float(policy.number("move_min_share")):
        return None
    if policy.flag("move_require_emptied") and not emptied:
        return None
    shape, relation, levels = _classify_move(source, destination, policy)
    story = Story(shape=shape, source=source, destinations=[destination], pairs=group,
                  levels=levels, relation=relation,
                  source_files_before=before_files, source_files_after=after_files)
    story.detail = _story_detail(story, policy, share)
    story.evidence = _story_evidence(story, policy, share=share, emptied=emptied)
    return story


def _classify_move(source: str, destination: str, policy: Policy) -> tuple[str, str, int]:
    levels = depth_of(destination + "/x") - depth_of(source + "/x")
    if is_under(source, destination) and depth_of(source + "/x") - depth_of(destination + "/x") \
            >= int(policy.number("flatten_min_levels")):
        return SHAPE_FLATTEN, "moved up %d level(s) into an ancestor" % abs(levels), levels
    if directory_of(source) == directory_of(destination) and leaf(source) != leaf(destination):
        return SHAPE_PACKAGE_RENAME, "renamed in place", levels
    if leaf(source) == leaf(destination) and directory_of(source) != directory_of(destination):
        return SHAPE_SUBTREE_MOVE, "re-parented under `%s`" % (directory_of(destination) or "./"), levels
    return (SHAPE_SUBTREE_MOVE, "re-parented and renamed to `%s`" % (leaf(destination)),
            levels)


def _split_story(source: str, mine: list[Pair], before: Tree, after: Tree,
                 policy: Policy) -> Story | None:
    """One directory's files re-bucketed into two or more new homes.

    Only files that sat **directly** in the source are arms of the split. That restriction is
    what keeps "`src/a`, `src/b`, `src/c` and `src/d` each moved whole under a new parent"
    from being read as one split of `src` into two parents: those files did not live in `src`,
    so `src` was not the module that was split. A destination needs
    `structure.split.min_files_per_destination` files to count as an arm at all, so one stray
    file in a new folder does not turn a move into a split.
    """
    arms: dict[str, list[Pair]] = {}
    for pair in mine:
        if directory_of(pair.old_path) != source:
            continue
        if directory_of(pair.new_path) == source:
            continue
        arms.setdefault(directory_of(pair.new_path), []).append(pair)
    floor = max(1, int(policy.number("split_min_files_per_destination")))
    arms = {key: value for key, value in arms.items() if len(value) >= floor}
    if len(arms) < int(policy.number("split_min_destinations")):
        return None
    order = sorted(arms, key=lambda key: (-len(arms[key]), key))
    before_files = len(before.files_under(source)) if source else len(before.entries)
    if not before_files:
        return None
    total = sum(len(value) for value in arms.values())
    share = total / before_files
    after_files = len(after.files_under(source)) if source else len(after.entries)
    if total < int(policy.number("move_min_files")):
        return None
    if share < float(policy.number("move_min_share")):
        return None
    if policy.flag("move_require_emptied") and after_files:
        return None
    story = Story(shape=SHAPE_MODULE_SPLIT, source=source, destinations=list(order),
                  pairs=[pair for key in order for pair in sorted(arms[key],
                                                                  key=lambda item: item.old_path)],
                  levels=0, relation="%d destination directories" % len(arms),
                  source_files_before=before_files, source_files_after=after_files)
    story.detail = _story_detail(story, policy, share)
    story.evidence = _story_evidence(story, policy, share=share, emptied=not after_files)
    return story


def _loose_stories(pairs: list[Pair], policy: Policy) -> list[Story]:
    groups: dict[tuple[str, str], list[Pair]] = {}
    for pair in pairs:
        key = (directory_of(pair.old_path), directory_of(pair.new_path))
        groups.setdefault(key, []).append(pair)
    out: list[Story] = []
    for (source, destination), group in sorted(groups.items()):
        story = Story(shape=SHAPE_FILES_MOVED, source=source, destinations=[destination],
                      pairs=group,
                      levels=depth_of(destination + "/x") - depth_of(source + "/x"),
                      relation="shape not preserved",
                      detail="%d file(s) changed directory without forming a named shape — "
                             "%s" % (len(group), _why_not_a_shape(source, group, policy)))
        story.evidence = [_clip(item, 120) for item in [pair.evidence() for pair in group]][:
            int(policy.number("max_evidence"))]
        out.append(story)
    return out


def _why_not_a_shape(source: str, group: list[Pair], policy: Policy) -> str:
    """Why this group is a `files-moved` row rather than one of the four shapes, with the knob named.

    A reviewer who sees a 16-file move called `files moved` needs to know which rule refused the
    bigger word, so every refusal here names the knob that did it.
    """
    floor = int(policy.number("move_min_files"))
    if len(group) < floor:
        return ("only %d pairing(s) come from %s and `structure.move.min_files` is %s, so no "
                "whole-directory shape is claimed" % (len(group), _display(source), floor))
    destinations = {directory_of(pair.new_path) for pair in group}
    levels = {pair.levels for pair in group}
    if len(destinations) > 1:
        return ("files went to %d destinations (%s) while %s keeps files of its own: "
                "`structure.split.min_destinations` arms that are too small or "
                "`structure.move.require_emptied` refused the split"
                % (len(destinations), ", ".join(sorted(_display(item) for item in destinations)),
                   _display(source)))
    if levels and max(levels) <= 0:
        return ("the source directory still holds files after the move, which "
                "`structure.move.require_emptied` refuses to call a moved directory")
    return ("the destination renamed files on the way, so the directory shape is not "
            "preserved (`structure.flatten.min_levels` asks for a move that keeps the "
            "relative paths)")


def _story_detail(story: Story, policy: Policy, share: float) -> str:
    if story.shape == SHAPE_MODULE_SPLIT:
        return ("%s held %d file(s) and none remain; they now live in %d destination "
                "directories (%s of the directory moved)"
                % (_display(story.source), story.source_files_before, len(story.destinations),
                   _pct(share)))
    if story.shape == SHAPE_FLATTEN:
        return ("%s moved %d of its %d file(s) up %d level(s) into %s and disappeared (%s "
                "of the directory moved)"
                % (_display(story.source), story.moved_files, story.source_files_before,
                   abs(story.levels), _display(story.destinations[0]), _pct(share)))
    relation = story.relation or "moved"
    return ("%s held %d file(s), %d moved to %s and none remain (%s of the directory); %s"
            % (_display(story.source), story.source_files_before, story.moved_files,
               _display(story.destinations[0]), _pct(share), relation))


def _story_evidence(story: Story, policy: Policy, *, share: float, emptied: bool) -> list[str]:
    cap = int(policy.number("max_evidence"))
    lines = [pair.evidence() for pair in story.pairs]
    lines.append("source %s held %d file(s) before, %d after%s"
                 % (_display(story.source), story.source_files_before,
                    story.source_files_after, "" if emptied else " (not emptied)"))
    lines.append("destinations: %s" % ", ".join(
        "%s (%d file(s))" % (_display(item), sum(1 for pair in story.pairs
                                                 if directory_of(pair.new_path) == item
                                                 or is_under(pair.new_path, item)))
        for item in story.destinations))
    lines.append("share of the source directory that moved: %s" % _pct(share))
    head = lines[:cap]
    if len(lines) > cap:
        head.append("… %d more line(s) of evidence: fathom structure findings --all"
                    % (len(lines) - cap))
    return [_clip(line, 160) for line in head]


def _clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[: max(1, width - 3)] + "…"


def _num(value: object) -> str:
    if isinstance(value, float):
        return ("%.2f" % value).rstrip("0").rstrip(".") if value % 1 else "%d" % value
    return str(value)


def _pct(value: float) -> str:
    return "%.0f%%" % (max(0.0, min(1.0, value)) * 100.0)
