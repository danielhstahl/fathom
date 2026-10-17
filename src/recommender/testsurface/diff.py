"""Comparing two revisions: which named cases moved, and which assertion stopped meaning much.

Two kinds of comparison happen here, and both are per file pair:

**Line counts.** `line_stats()` is a git-style line count with the ranges kept, because
`delta.py` has to attribute *changed lines* to "test" or "production" and, inside a Rust
file, the two are interleaved: lines 1–40 are the API and lines 41–90 are
`#[cfg(test)] mod tests`. A count without ranges cannot do that, and a count of only the
whole file would call a 300-line refactor with one new test "300 production lines, 1 test".

**Case identity.** `case_delta()` compares two :class:`~recommender.testsurface.parsers.ParsedFile`
objects by *case name*, never by line number, and produces five answers:

| answer | rule |
| --- | --- |
| added | a case key present only in the new revision |
| removed | a case key present only in the old revision — including every case in a deleted file |
| newly skipped | present in both, not skipped before, skipped now (or added already skipped, which is said separately) |
| un-skipped | the good direction: a case that was disabled and now runs again. Counted, never reported as a problem |
| weakened / assertions removed | the assertion census, compared subject by subject |

The weakening comparison is the one that needs a rule worth writing down, because "weaker"
is the sort of word a tool gets wrong confidently. The rule, in one sentence: **an exact
assertion about a subject is *weakened* when the same subject survives in the new revision
only inside a presence assertion**, and it is *removed* when the subject is not asserted
about at all and nothing exact was added in its place.

That single sentence covers the four shapes the ticket names — "assert removed, or equality
replaced by existence check" — in five languages, because they all reduce to the same two
strengths:

```
assert_eq!(order.total, 42)     →  assert!(order.total > 0)     exact → presence: weakened
assert store.get("a") == 1      →  assert store.get("a")        exact → presence: weakened
assert.Equal(t, got, want)      →  assert.NotNil(t, got)        exact → presence: weakened
expect(x).toBe(3)               →  expect(x).toBeDefined()      exact → presence: weakened
assertEquals(cart.total(), 3)   →  assertNotNull(cart.total())  exact → presence: weakened
assert_eq!(a, b)  (deleted)     →  nothing                      exact → nothing:   removed
assert_eq!(a, b)  →  assert!(a == b)                           same key: neither, it is the
                                                                same claim in another spelling
```

Subject keys are formatting-insensitive (see `scan.subject`), which is what lets the rule
survive a rename of the *expected* value, a re-wrap, or a change of assertion macro that
kept the comparison. A case that re-orders its assertions, splits into two cases, or changes
language produces no weakening finding — the identity rule is exact on names and the
strength rule is exact on subjects, and a heuristic that fires on a refactor is a heuristic
that gets turned off within a week.
"""

from __future__ import annotations

import difflib
from dataclasses import dataclass, field

from . import parsers as P
from .parsers import Assertion, TestCase

#: Weakening outcomes.
WEAKENED = "weakened"
REMOVED = "removed"


@dataclass(frozen=True)
class LineStats:
    """Changed lines, and *where* they changed, so a span can be attributed per line."""

    added: int = 0
    deleted: int = 0
    added_ranges: tuple[tuple[int, int], ...] = ()      # after-side, 1-based inclusive
    deleted_ranges: tuple[tuple[int, int], ...] = ()    # before-side

    @property
    def changed(self) -> int:
        return self.added + self.deleted

    def added_in(self, start: int, end: int) -> int:
        """Added lines whose after-side line falls inside a 1-based inclusive span."""
        return _overlap(self.added_ranges, start, end)

    def deleted_in(self, start: int, end: int) -> int:
        return _overlap(self.deleted_ranges, start, end)

    def as_dict(self) -> dict:
        return {"added": self.added, "deleted": self.deleted, "changed": self.changed}


@dataclass
class Weakening:
    """One assertion that stopped meaning as much, with both spellings quoted."""

    case: str
    subject: str
    outcome: str                                  # WEAKENED | REMOVED
    before: Assertion
    after: Assertion | None = None
    path: str = ""
    case_line: int = 0
    case_old_line: int = 0

    @property
    def weakened(self) -> bool:
        return self.outcome == WEAKENED

    def detail(self) -> str:
        if self.outcome == WEAKENED and self.after is not None:
            return "%s → %s" % (self.before.kind, self.after.kind)
        return "%s removed" % self.before.kind

    def evidence(self) -> list[str]:
        out = ["was: %s" % (self.before.text or self.before.describe())]
        if self.after is not None:
            out.append("now: %s" % (self.after.text or self.after.describe()))
        else:
            out.append("now: nothing asserts about `%s`" % self.subject)
        return out

    def as_dict(self) -> dict:
        return {"case": self.case, "subject": self.subject, "outcome": self.outcome,
                "case_line": self.case_line, "case_old_line": self.case_old_line,
                "before": self.before.as_dict(),
                "after": self.after.as_dict() if self.after else None}


@dataclass
class CaseDiff:
    """One file pair's case-level story."""

    path: str
    language: str = ""
    framework: str = ""
    added: list[TestCase] = field(default_factory=list)
    removed: list[TestCase] = field(default_factory=list)
    newly_skipped: list[tuple[TestCase, TestCase | None]] = field(default_factory=list)
    unskipped: list[TestCase] = field(default_factory=list)
    focused: list[TestCase] = field(default_factory=list)
    weakened: list[Weakening] = field(default_factory=list)
    assertions_lost: list[Weakening] = field(default_factory=list)
    assertions_added: int = 0
    exact_before: int = 0
    exact_after: int = 0
    notes: list[str] = field(default_factory=list)

    @property
    def known(self) -> bool:
        """Whether case names were available at all (as opposed to a line count only)."""
        return bool(self.language)

    def counts(self) -> dict:
        return {"added": len(self.added), "removed": len(self.removed),
                "skipped": len(self.newly_skipped), "unskipped": len(self.unskipped),
                "focused": len(self.focused), "weakened": len(self.weakened),
                "assertions_lost": len(self.assertions_lost)}

    def as_dict(self) -> dict:
        return {"path": self.path, "language": self.language, "framework": self.framework,
                "counts": self.counts(),
                "added": [case.as_dict() for case in self.added],
                "removed": [case.as_dict() for case in self.removed],
                "newly_skipped": [{"case": case.as_dict(),
                                   "was": was.as_dict() if was else None}
                                  for case, was in self.newly_skipped],
                "unskipped": [case.as_dict() for case in self.unskipped],
                "focused": [case.as_dict() for case in self.focused],
                "weakened": [item.as_dict() for item in self.weakened],
                "assertions_lost": [item.as_dict() for item in self.assertions_lost],
                "assertions_added": self.assertions_added,
                "notes": list(self.notes)}


# ---------------------------------------------------------------------------
# line counting
# ---------------------------------------------------------------------------
def line_stats(before: str | None, after: str | None) -> LineStats:
    """git-style added/deleted line counts with the ranges that produced them."""
    if before is None and after is None:
        return LineStats()
    old_lines = (before or "").splitlines()
    new_lines = (after or "").splitlines()
    if not old_lines:
        return LineStats(added=len(new_lines), deleted=0,
                         added_ranges=((1, len(new_lines)),) if new_lines else ())
    if not new_lines:
        return LineStats(added=0, deleted=len(old_lines),
                         deleted_ranges=((1, len(old_lines)),) if old_lines else ())
    added: list[tuple[int, int]] = []
    deleted: list[tuple[int, int]] = []
    matcher = difflib.SequenceMatcher(a=old_lines, b=new_lines, autojunk=False)
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            continue
        if i2 > i1:
            deleted.append((i1 + 1, i2))
        if j2 > j1:
            added.append((j1 + 1, j2))
    return LineStats(added=sum(end - start + 1 for start, end in added),
                     deleted=sum(end - start + 1 for start, end in deleted),
                     added_ranges=tuple(added), deleted_ranges=tuple(deleted))


def _overlap(ranges: tuple[tuple[int, int], ...], start: int, end: int) -> int:
    total = 0
    for low, high in ranges:
        lo, hi = max(low, start), min(high, end)
        if hi >= lo:
            total += hi - lo + 1
    return total


# ---------------------------------------------------------------------------
# case comparison
# ---------------------------------------------------------------------------
def case_delta(path: str, before: P.ParsedFile | None, after: P.ParsedFile | None,
               *, language: str = "") -> CaseDiff:
    """Compare two revisions of one file's test surface."""
    language = language or (after and after.language) or (before and before.language) or ""
    framework = ""
    for source in (after, before):
        if source and source.frameworks:
            framework = source.frameworks[0]
            break
    diff = CaseDiff(path=path, language=language, framework=framework)
    if before is not None and before.unsupported:
        diff.notes.append("old revision could not be parsed (%s): case names before this "
                          "episode are unknown, so nothing is reported as removed"
                          % "; ".join(before.notes) if before.notes
                          else "old revision could not be parsed")
    if after is not None and after.unsupported:
        diff.notes.append("new revision could not be parsed (%s): case counts are a lower "
                          "bound" % "; ".join(after.notes) if after.notes
                          else "new revision could not be parsed")
    if before is None or after is None or not language:
        # One side missing or no parser: names are only known on the side that exists.
        known = after if after is not None else before
        if known is None or not language:
            diff.notes.append("no test-case parser for this file: line counts only")
            return diff
    old = before.case_keys() if before is not None and before.parsed else {}
    new = after.case_keys() if after is not None and after.parsed else {}
    if before is not None and not before.parsed:
        old = {}
    if after is not None and not after.parsed:
        new = {}
    diff.exact_before = sum(case.exact for case in old.values())
    diff.exact_after = sum(case.exact for case in new.values())

    for key, case in sorted(new.items()):
        if key not in old:
            diff.added.append(case)
            if case.skipped:
                diff.newly_skipped.append((case, None))
            if case.focused:
                diff.focused.append(case)
        else:
            previous = old[key]
            if case.skipped and not previous.skipped:
                diff.newly_skipped.append((case, previous))
            if not case.skipped and previous.skipped:
                diff.unskipped.append(case)
            if case.focused and not previous.focused:
                diff.focused.append(case)
    for key, case in sorted(old.items()):
        if key not in new:
            diff.removed.append(case)

    for key in sorted(set(old) & set(new)):
        _compare_assertions(path, old[key], new[key], diff)

    diff.assertions_added = max(0, diff.exact_after - diff.exact_before)
    diff.removed.sort(key=lambda case: case.qualified)
    diff.added.sort(key=lambda case: case.qualified)
    return diff


def _compare_assertions(path: str, before: TestCase, after: TestCase, diff: CaseDiff) -> None:
    """The weakening rule: same subject, less strength → weakened; no subject → removed."""
    after_keys = {assertion.key() for assertion in after.assertions if assertion.comparable}
    after_subjects = {assertion.subject for assertion in after.assertions if assertion.comparable}
    presence_subjects = {assertion.subject for assertion in after.assertions
                         if assertion.strength == P.PRESENCE and assertion.comparable}
    gained_exact = 0
    for assertion in after.assertions:
        if assertion.strength == P.EXACT and assertion.key() not in \
                {item.key() for item in before.assertions}:
            gained_exact += 1
    vanished: list[Assertion] = []
    for assertion in before.assertions:
        if assertion.strength != P.EXACT or not assertion.comparable:
            continue
        if assertion.key() in after_keys:
            continue
        vanished.append(assertion)
    explained: list[Weakening] = []
    unexplained: list[Weakening] = []
    for assertion in vanished:
        if assertion.subject in presence_subjects:
            replacement = next((item for item in after.assertions
                                if item.strength == P.PRESENCE
                                and item.subject == assertion.subject), None)
            explained.append(Weakening(case=after.qualified, subject=assertion.subject,
                                       outcome=WEAKENED, before=assertion, after=replacement,
                                       path=path, case_line=after.line,
                                       case_old_line=before.line))
        elif assertion.subject not in after_subjects:
            unexplained.append(Weakening(case=after.qualified, subject=assertion.subject,
                                         outcome=REMOVED, before=assertion, after=None,
                                         path=path, case_line=after.line,
                                         case_old_line=before.line))
    diff.weakened.extend(explained)
    if unexplained and gained_exact == 0:
        diff.assertions_lost.extend(unexplained)
    elif unexplained:
        diff.notes.append("%s lost %d exact assertion(s) and gained %d: reported as added "
                          "coverage rather than as a loss"
                          % (after.qualified, len(unexplained), gained_exact))
