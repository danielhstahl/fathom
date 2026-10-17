"""The episode: test lines against production lines, the contract diff, and the findings.

One call answers the ticket:

    from recommender.testsurface import Change, Episode, measure

    view = measure(Episode(id="ep-4f2a", files=[Change(path=…, before=…, after=…)]))
    view.counts()          # {'test_lines_added': 212, 'prod_lines_added': 1043, …}
    view.ratio()           # {'ratio': 0.169, 'test_to_prod': '1 : 4.9', …}
    view.proxy             # Proxy(basis='parsed-cases', confidence=0.9, …)
    view.findings          # the named, located findings, sorted by severity
    view.summary_line()    # the deterministic one-liner the card and the rail render

Four decisions are made here and nowhere else, so that the card (`e2.10`), the timeline
(`e1.6`), the export (`e5.7`) and the risk score cannot each grow a slightly different
answer to "how much of this is tested":

**Which changed lines are test lines.** The taxonomy class decides, with one exception worth
arguing about: inside a Rust file, `#[cfg(test)] mod tests` lines are test lines even though
the *path* is `source`. Attributing the whole file to production would call a refactor plus
one new test "200 production lines, 0 tests" — the exact mistake this ticket exists to fix,
so the parsed region wins over the path label, and the file prints a note saying that
happened.

**Which symbols are new public logic.** `public.py` says what is public and what has logic;
this file says what is *new* (present after, absent before, keyed by qualified name) and then
applies the tunable coverage definition from `policy.py`.

**Whether the episode covered it.** Three independent ways for a covering test to count —
the changed test file's path relates to the source file's path, a new test case's name
overlaps the symbol's name, or the symbol's name appears in lines the episode added to a
test file. Any one is enough; each one prints as evidence when it is the reason a finding
did *not* fire, because "why did this not get flagged" is a question reviewers ask out loud.

**How much to believe any of it.** The coverage proxy. A line count is not a coverage
number, and pretending otherwise is the failure mode the ticket's title refuses, so the
proxy states a basis (`parsed-cases`, `partial`, `lines-only`, `no-test-change`) and a
confidence taken from the policy, and the card prints both. A repository whose tests are
`.feature` files gets `lines-only / 0.30` and a sentence saying why, not a confident
`0 tests added`.

Like the rest of the package this is pure: no clock, no filesystem, no store. The one input
that *could* have needed a repository-wide read — "do the existing tests anywhere mention
this symbol?" — arrives as `Episode.repo_test_mentions`, is used only when
`test.untested.scope = "repo"`, and prints a note when that scope was asked for and the
mentions were not supplied.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from recommender.change.engine import Change as ChangeBlob, ChangeClassifier
from recommender.taxonomy import classes as C

from . import diff as D
from . import parsers as P
from . import public as PUB
from .policy import (ASSERTION_REMOVED, ASSERTION_WEAKENED, FOCUSED_TEST, Policy,
                     SEVERITY_ORDER, TEST_FILE_REMOVED, TEST_REMOVED, TEST_SKIPPED,
                     UNTESTED_PUBLIC, Finding, sort_key)

#: Version of the delta's shape and rules. A stored delta that disagrees is recomputed.
SURFACE_VERSION = 1

BASIS_PARSED = "parsed-cases"
BASIS_PARTIAL = "partial"
BASIS_LINES = "lines-only"
BASIS_NO_TEST = "no-test-change"
BASES: tuple[str, ...] = (BASIS_PARSED, BASIS_PARTIAL, BASIS_LINES, BASIS_NO_TEST)


@dataclass(frozen=True)
class Change:
    """One file in an episode: the path and whichever revisions the caller has."""

    path: str
    before: str | None = None
    after: str | None = None
    change_type: str = "modified"
    old_path: str = ""
    added_lines: int = 0
    deleted_lines: int = 0

    @property
    def subject(self) -> str:
        return self.path or self.old_path

    @property
    def content(self) -> str | None:
        return self.after if self.after is not None else self.before

    @staticmethod
    def from_pair(path: str, before: str | None, after: str | None, **kwargs) -> "Change":
        change_type = "modified"
        if before is None:
            change_type = "added"
        if after is None:
            change_type = "deleted"
        return Change(path=path, before=before, after=after, change_type=change_type, **kwargs)


@dataclass(frozen=True)
class Episode:
    """A change set, in review order, plus the optional repository half of the coverage rule."""

    id: str
    files: tuple[Change, ...] = ()
    title: str = ""
    #: Names the repository's *existing* tests already mention. Only read when
    # `test.untested.scope = "repo"`, because computing it needs the index, which is a
    # caller's job in this pure half.
    repo_test_mentions: tuple[str, ...] = ()

    @classmethod
    def from_pairs(cls, episode_id: str, pairs: dict[str, tuple[str | None, str | None]],
                   **kwargs) -> "Episode":
        return cls(id=episode_id,
                   files=tuple(Change.from_pair(path, before, after)
                               for path, (before, after) in sorted(pairs.items())),
                   **kwargs)

    @property
    def paths(self) -> list[str]:
        return [change.subject for change in self.files]


@dataclass
class FileSurface:
    """One file's contribution: lines, cases, new symbols."""

    path: str
    cls: str = ""
    change_type: str = "modified"
    language: str = ""
    framework: str = ""
    parser_state: str = ""
    confidence_band: str = ""
    class_confidence: float = 0.0
    lines: D.LineStats = field(default_factory=D.LineStats)
    test_added: int = 0
    test_deleted: int = 0
    prod_added: int = 0
    prod_deleted: int = 0
    other_added: int = 0
    other_deleted: int = 0
    regions: tuple[tuple[int, int], ...] = ()
    case_diff: D.CaseDiff | None = None
    new_symbols: list[PUB.Symbol] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    added_test_text: str = ""

    @property
    def is_test_surface(self) -> bool:
        return self.cls == C.PathClass.TEST or bool(self.regions) \
            or bool(self.case_diff and self.case_diff.language)

    @property
    def test_ratio(self) -> float:
        total = self.test_added + self.prod_added
        return self.test_added / total if total else 0.0

    def as_dict(self) -> dict:
        return {"path": self.path, "class": self.cls, "class_confidence":
                round(self.class_confidence, 2), "change_type": self.change_type,
                "language": self.language, "framework": self.framework,
                "parser_state": self.parser_state, "lines": self.lines.as_dict(),
                "test_lines": {"added": self.test_added, "deleted": self.test_deleted},
                "prod_lines": {"added": self.prod_added, "deleted": self.prod_deleted},
                "other_lines": {"added": self.other_added, "deleted": self.other_deleted},
                "test_regions": [list(span) for span in self.regions],
                "new_public_symbols": [symbol.as_dict() for symbol in self.new_symbols],
                "notes": list(self.notes),
                "case_delta": self.case_diff.as_dict() if self.case_diff else None}


@dataclass
class Proxy:
    """The stated confidence in everything this ticket says about test *amount*."""

    basis: str = BASIS_NO_TEST
    confidence: float = 1.0
    statement: str = ""
    evidence: list[str] = field(default_factory=list)
    languages: list[str] = field(default_factory=list)
    unparsed: list[str] = field(default_factory=list)

    def as_dict(self) -> dict:
        return {"basis": self.basis, "confidence": round(self.confidence, 2),
                "statement": self.statement, "evidence": list(self.evidence),
                "languages": list(self.languages), "unparsed": list(self.unparsed)}


@dataclass
class SurfaceDelta:
    """The whole answer for one episode."""

    episode_id: str = ""
    title: str = ""
    files: list[FileSurface] = field(default_factory=list)
    findings: list[Finding] = field(default_factory=list)
    suppressed: dict[str, int] = field(default_factory=dict)
    suppressed_source: dict[str, str] = field(default_factory=dict)
    policy: Policy = field(default_factory=Policy.built_in)
    proxy: Proxy = field(default_factory=Proxy)
    notes: list[str] = field(default_factory=list)
    version: int = SURFACE_VERSION

    # -- the numbers --------------------------------------------------------
    def counts(self) -> dict:
        diffs = [item.case_diff for item in self.files if item.case_diff]
        by_language: dict[str, int] = {}
        for item in self.files:
            if item.language:
                by_language[item.language] = by_language.get(item.language, 0) + 1
        findings: dict[str, int] = {}
        for finding in self.findings:
            findings[finding.kind] = findings.get(finding.kind, 0) + 1
        return {
            "files": len(self.files),
            "files_created": sum(1 for i in self.files if i.change_type == "added"),
            "files_deleted": sum(1 for i in self.files if i.change_type == "deleted"),
            "files_renamed": sum(1 for i in self.files if i.change_type == "renamed"),
            "files_modified": sum(1 for i in self.files if i.change_type == "modified"),
            "test_files_created": sum(1 for i in self.files
                                      if i.is_test_surface and i.change_type == "added"),
            "test_files_deleted": sum(1 for i in self.files
                                      if i.is_test_surface and i.change_type == "deleted"),
            "test_files_renamed": sum(1 for i in self.files
                                      if i.is_test_surface and i.change_type == "renamed"),
            "test_files_modified": sum(1 for i in self.files
                                       if i.is_test_surface and i.change_type == "modified"),
            "test_lines_added": sum(item.test_added for item in self.files),
            "test_lines_deleted": sum(item.test_deleted for item in self.files),
            "prod_lines_added": sum(item.prod_added for item in self.files),
            "prod_lines_deleted": sum(item.prod_deleted for item in self.files),
            "other_lines_added": sum(item.other_added for item in self.files),
            "other_lines_deleted": sum(item.other_deleted for item in self.files),
            "tests_added": sum(len(diff.added) for diff in diffs),
            "tests_removed": sum(len(diff.removed) for diff in diffs),
            "tests_skipped": sum(len(diff.newly_skipped) for diff in diffs),
            "tests_unskipped": sum(len(diff.unskipped) for diff in diffs),
            "tests_xfail": sum(1 for diff in diffs for case, _ in diff.newly_skipped
                               if case.skip_kind == "xfail"),
            "tests_focused": sum(len(diff.focused) for diff in diffs),
            "assertions_weakened": sum(len(diff.weakened) for diff in diffs),
            "assertions_lost": sum(len(diff.assertions_lost) for diff in diffs),
            "assertions_added": sum(diff.exact_after - diff.exact_before for diff in diffs
                                    if diff.exact_after > diff.exact_before),
            "public_symbols_added": sum(len(item.new_symbols) for item in self.files),
            "public_logic_added": sum(1 for item in self.files for symbol in item.new_symbols
                                      if symbol.has_logic),
            "findings": dict(sorted(findings.items())),
            "findings_suppressed": dict(sorted(self.suppressed.items())),
            "languages": dict(sorted(by_language.items())),
        }

    def ratio(self) -> dict:
        """Test lines over test + production lines *added*, plus the same in words."""
        test = self.counts()["test_lines_added"]
        prod = self.counts()["prod_lines_added"]
        total = test + prod
        ratio = (test / total) if total else None
        if ratio is None:
            words = "no test or production lines added"
        elif test == 0:
            words = "0 test lines for %d production line(s)" % prod
        elif prod == 0:
            words = "%d test line(s), nothing added to production" % test
        else:
            words = "%.2f test lines per production line" % (test / prod)
        return {"test_lines_added": test, "prod_lines_added": prod,
                "ratio": round(ratio, 3) if ratio is not None else None,
                "test_to_prod": ("1 : %.1f" % (prod / test)) if test and prod else
                ("none" if test == 0 and prod else ("all-test" if test else "none")),
                "basis": "lines added, attributed by taxonomy class, Rust `#[cfg(test)]` "
                         "regions counted as test",
                "words": words}

    @property
    def confidence(self) -> float:
        return self.proxy.confidence

    # -- the lines a human reads -------------------------------------------
    def summary_line(self) -> str:
        counts = self.counts()
        ratio = self.ratio()
        parts: list[str] = []
        if self.episode_id:
            parts.append(self.episode_id)
        parts.append("+%d test / +%d prod lines"
                     % (counts["test_lines_added"], counts["prod_lines_added"]))
        parts.append("ratio %s" % (("%.3f" % ratio["ratio"]) if ratio["ratio"] is not None
                                   else "—"))
        parts.append("%d tests added, %d removed, %d skipped"
                     % (counts["tests_added"], counts["tests_removed"],
                        counts["tests_skipped"]))
        if counts["assertions_weakened"] or counts["assertions_lost"]:
            parts.append("%d assertions weakened, %d lost"
                         % (counts["assertions_weakened"], counts["assertions_lost"]))
        parts.append("%d finding%s" % (len(self.findings),
                                       "" if len(self.findings) == 1 else "s"))
        parts.append("confidence %.2f (%s)" % (self.proxy.confidence, self.proxy.basis))
        return " · ".join(parts)

    def headline(self) -> str:
        """The card's first line: the ratio question, answered in words."""
        ratio = self.ratio()
        counts = self.counts()
        return "%s — %d test line(s) added against %d production line(s); %s" % (
            self.episode_id or "episode", counts["test_lines_added"],
            counts["prod_lines_added"], ratio["words"])

    def detail(self, max_findings: int | None = None) -> list[str]:
        """The card body: every file, every finding, the stated confidence."""
        cap = max_findings if max_findings is not None else int(self.policy.number("max_findings"))
        out: list[str] = [self.summary_line()]
        for item in self.files:
            out.append("  %s %s (%s%s)" % (_arrow(item.change_type), item.path, item.cls,
                                           ", %s" % item.language if item.language else ""))
            out.append("    +%d/−%d lines · %d test / %d prod / %d other lines added"
                       % (item.lines.added, item.lines.deleted, item.test_added,
                          item.prod_added, item.other_added))
            if item.case_diff and item.case_diff.language:
                counts = item.case_diff.counts()
                out.append("    %s cases: %d added, %d removed, %d skipped, %d weakened"
                           % (item.case_diff.framework or item.case_diff.language,
                              counts["added"], counts["removed"], counts["skipped"],
                              counts["weakened"]))
            for symbol in item.new_symbols:
                out.append("    new public %s: %s at %s:%d — %d body line(s), %d of them logic"
                           % (symbol.kind, symbol.name, item.path, symbol.line,
                              symbol.body_lines, symbol.logic_lines))
            for note in item.notes:
                out.append("    note: %s" % note)
        if self.findings:
            out.append("  findings")
            for finding in self.findings[:cap]:
                out.extend(finding.block(indent=4))
            hidden = len(self.findings) - cap
            if hidden > 0:
                out.append("    … %d more finding(s): fathom testsurface findings %s"
                           % (hidden, self.episode_id or "<episode>"))
        else:
            out.append("  no findings")
        for kind, count in sorted(self.suppressed.items()):
            out.append("  %d %s finding(s) not reported: %s is off (%s)"
                       % (count, kind, kind, self.suppressed_source.get(kind, "policy")))
        out.append("  coverage proxy: %s" % self.proxy.statement)
        for evidence in self.proxy.evidence:
            out.append("    · %s" % evidence)
        for note in self.notes:
            out.append("  note: %s" % note)
        return out

    def as_dict(self) -> dict:
        return {"episode_id": self.episode_id, "title": self.title,
                "version": self.version, "counts": self.counts(), "ratio": self.ratio(),
                "proxy": self.proxy.as_dict(), "headline": self.headline(),
                "summary": self.summary_line(),
                "findings": [finding.as_dict() for finding in self.findings],
                "findings_suppressed": dict(sorted(self.suppressed.items())),
                "policy": self.policy.as_dict(), "notes": list(self.notes),
                "files": [item.as_dict() for item in self.files]}

    # -- internals ----------------------------------------------------------
    def add(self, finding: Finding) -> None:
        """Honour the policy, but never silently: a switch that hid something says so."""
        enabled, source = self.policy.effective(finding.kind)
        if not enabled:
            self.suppressed[finding.kind] = self.suppressed.get(finding.kind, 0) + 1
            self.suppressed_source[finding.kind] = source
            return
        self.findings.append(finding)


# ---------------------------------------------------------------------------
# the run
# ---------------------------------------------------------------------------
def measure(episode: Episode, policy: Policy | None = None,
            classifier: ChangeClassifier | None = None) -> SurfaceDelta:
    """Measure one episode's test surface. The ticket, in one call."""
    active = policy or Policy.built_in()
    clf = classifier or ChangeClassifier.default()
    view = SurfaceDelta(episode_id=episode.id, title=episode.title, policy=active)
    for change in sorted(episode.files, key=lambda item: item.subject):
        view.files.append(_inspect_file(change, clf, active))
    _collect_findings(view)
    _coverage(view, episode, active)
    view.findings.sort(key=sort_key)
    view.proxy = _proxy(view, active)
    return view


def _inspect_file(change: Change, clf: ChangeClassifier, policy: Policy) -> FileSurface:
    path = change.subject
    decision = clf.classify(ChangeBlob(path=change.path or path, content=change.content,
                                       change_type=change.change_type,
                                       old_path=change.old_path))
    surface = FileSurface(path=path, cls=decision.cls, change_type=change.change_type,
                          class_confidence=decision.confidence)
    before, after = change.before, change.after
    parsed_before = P.parse(change.old_path or path, before) if before is not None \
        else P.ParsedFile(path=path, state=P.STATE_UNSUPPORTED, notes=["no old revision"])
    parsed_after = P.parse(path, after) if after is not None \
        else P.ParsedFile(path=path, state=P.STATE_UNSUPPORTED, notes=["no new revision"])
    surface.language = (parsed_after.language or parsed_before.language
                        or P.language_for(path))
    surface.parser_state = (parsed_after.state if after is not None else parsed_before.state)
    if parsed_after.frameworks:
        surface.framework = parsed_after.frameworks[0]
    elif parsed_before.frameworks:
        surface.framework = parsed_before.frameworks[0]

    stats = D.line_stats(before, after)
    if (change.added_lines or change.deleted_lines) and not (stats.added or stats.deleted):
        stats = D.LineStats(added=change.added_lines, deleted=change.deleted_lines)
    surface.lines = stats

    regions = parsed_after.test_regions or parsed_before.test_regions
    surface.regions = tuple(regions)
    attributed = bool(regions) and surface.cls != C.PathClass.TEST
    if surface.cls == C.PathClass.TEST:
        surface.test_added, surface.test_deleted = stats.added, stats.deleted
    elif attributed:
        inside_added = sum(stats.added_in(start, end) for start, end in regions)
        inside_deleted = sum(stats.deleted_in(start, end) for start, end in regions)
        surface.test_added, surface.test_deleted = inside_added, inside_deleted
        surface.prod_added = stats.added - inside_added
        surface.prod_deleted = stats.deleted - inside_deleted
        surface.notes.append("%d added line(s) attributed to test code inside the file "
                             "(`%s`), not to production: the parsed test module outranks "
                             "the path label here"
                             % (inside_added, ", ".join("%d–%d" % span for span in regions)))
    elif surface.cls == C.PathClass.SOURCE:
        surface.prod_added, surface.prod_deleted = stats.added, stats.deleted
    else:
        surface.other_added, surface.other_deleted = stats.added, stats.deleted

    if surface.language and (surface.cls == C.PathClass.TEST or parsed_after.cases
                             or parsed_before.cases):
        surface.case_diff = D.case_delta(path, parsed_before, parsed_after,
                                         language=surface.language)
        for note in list(parsed_before.notes) + list(parsed_after.notes):
            if note not in surface.notes:
                surface.notes.append(note)
    if surface.cls in (C.PathClass.SOURCE,) and change.change_type != "deleted":
        old_symbols = {symbol.key() for symbol in PUB.public_symbols(change.old_path or path,
                                                                    before)}
        new_symbols = PUB.public_symbols(path, after)
        surface.new_symbols = [symbol for symbol in new_symbols
                               if symbol.key() not in old_symbols
                               and policy.counts_as_public(symbol.visibility)]
    if surface.cls == C.PathClass.TEST and after:
        surface.added_test_text = _added_text(after, stats)
    elif after and regions:
        lines = after.splitlines()
        chunks = ["\n".join(lines[start - 1:end]) for start, end in regions
                  if end <= len(lines) + 1]
        surface.added_test_text = _restricted(_added_text(after, stats), "\n".join(chunks))
    return surface


def _restricted(added: str, region_text: str) -> str:
    """Added lines that are also inside the file's test region."""
    if not added:
        return ""
    allowed = {line for line in region_text.lower().splitlines() if line.strip()}
    return "\n".join(line for line in added.splitlines() if line in allowed)


def _added_text(after: str, stats: D.LineStats) -> str:
    lines = after.splitlines()
    chunks: list[str] = []
    for start, end in stats.added_ranges:
        chunks.extend(lines[start - 1:end])
    return "\n".join(chunks).lower()[:400_000]


# ---------------------------------------------------------------------------
# findings
# ---------------------------------------------------------------------------
def _collect_findings(view: SurfaceDelta) -> None:
    for item in view.files:
        is_test_file = item.cls == C.PathClass.TEST
        if is_test_file and item.change_type == "deleted":
            view.add(Finding(kind=TEST_FILE_REMOVED, name=item.path.rsplit("/", 1)[-1],
                             path=item.path, unit="file",
                             detail="the whole test file was deleted",
                             evidence=["%s deleted; %s" % (item.path,
                                                           "%d line(s) went with it"
                                                           % item.lines.deleted)],
                             language=item.language))
        diff = item.case_diff
        if diff is None:
            continue
        for case in diff.removed:
            detail = "removed from %s" % (diff.framework or diff.language)
            evidence = ["case %s" % case.qualified]
            if case.assertions:
                evidence.append("%d assertion(s) went with it (%d exact)"
                                % (len(case.assertions), case.exact))
            view.add(Finding(kind=TEST_REMOVED, name=case.qualified, path=item.path,
                             line=case.line, language=diff.language, detail=detail,
                             evidence=evidence))
        for case, previous in diff.newly_skipped:
            marker = case.skip_kind or "skip"
            detail = "now `%s%s`" % (marker, " (%s)" % case.skip_reason if case.skip_reason
                                     else "")
            evidence = (["added already disabled — it has never run"] if previous is None
                        else ["it ran in the previous revision"])
            if case.assertions:
                evidence.append("%d assertion(s) that will not execute"
                                % len(case.assertions))
            view.add(Finding(kind=TEST_SKIPPED, name=case.qualified, path=item.path,
                             line=case.line, language=diff.language, detail=detail,
                             evidence=evidence))
        for weakened in diff.weakened:
            view.add(Finding(kind=ASSERTION_WEAKENED,
                             name=weakened.case, path=item.path,
                             line=weakened.case_line or weakened.before.line,
                             old_line=weakened.before.line,
                             language=diff.language,
                             detail="%s: `%s` no longer compared to an expected value"
                                    % (weakened.detail(), weakened.subject),
                             evidence=weakened.evidence()))
        if diff.assertions_lost:
            first = diff.assertions_lost[0]
            view.add(Finding(kind=ASSERTION_REMOVED, name=first.case, path=item.path,
                             line=first.case_line or first.before.line,
                             old_line=first.before.line, language=diff.language,
                             detail="%d exact assertion(s) removed with none added"
                                    % len(diff.assertions_lost),
                             evidence=[line for lost in diff.assertions_lost[:3]
                                       for line in lost.evidence()]))
        for case in diff.focused:
            view.add(Finding(kind=FOCUSED_TEST, name=case.qualified, path=item.path,
                             line=case.line, language=diff.language,
                             detail="focused run marker: every other test in this file is "
                                    "skipped by the runner",
                             evidence=list(case.evidence) or ["`.only`"]))


# ---------------------------------------------------------------------------
# the tunable coverage definition
# ---------------------------------------------------------------------------
def _coverage(view: SurfaceDelta, episode: Episode, policy: Policy) -> None:
    """`test.findings.untested_public`: new public logic with nothing covering it."""
    min_lines = int(policy.number("min_body_lines"))
    require_logic = policy.flag("require_logic")
    candidates: list[tuple[FileSurface, PUB.Symbol]] = []
    for item in view.files:
        excluded = policy.excluded(item.path)
        if excluded:
            view.notes.append("%s is exempt from the untested-public finding "
                              "(`test.untested.exclude` = %s)" % (item.path, excluded))
            continue
        for symbol in item.new_symbols:
            if require_logic and not symbol.has_logic:
                continue
            if symbol.body_lines < min_lines:
                continue
            candidates.append((item, symbol))

    if not candidates:
        return
    test_files = [item for item in view.files if item.is_test_surface]
    new_case_tokens = [_tokens(case.qualified)
                       for item in view.files if item.case_diff
                       for case in item.case_diff.added]
    mentions = {name.lower() for name in episode.repo_test_mentions}
    if policy.text("scope") == "repo" and not mentions:
        view.notes.append("test.untested.scope = repo asked for a repository-wide mention "
                          "check, but no test mentions were supplied: measured against this "
                          "episode only")
    for item, symbol in candidates:
        ways = _coverage_ways(item, symbol, test_files, new_case_tokens, policy, mentions)
        if ways:
            view.notes.append("%s is covered: %s" % (symbol.name, "; ".join(ways)))
            continue
        view.add(Finding(
            kind=UNTESTED_PUBLIC, name=symbol.name, path=item.path, line=symbol.line,
            language=item.language, unit="symbol",
            detail="new public %s with %d logic line(s), no covering test in this episode"
                   % (symbol.kind, symbol.logic_lines),
            evidence=_coverage_evidence(symbol, policy)))
    view.findings.sort(key=sort_key)


def _coverage_ways(item: FileSurface, symbol: PUB.Symbol, test_files: list[FileSurface],
                   new_case_tokens: list[set[str]], policy: Policy,
                   mentions: set[str]) -> list[str]:
    """Every way the episode could have counted as covering this symbol, with receipts."""
    ways: list[str] = []
    need_path = int(policy.number("path_overlap"))
    source_tokens = _path_tokens(item.path)
    for test in test_files:
        if test.path == item.path or test.cls != C.PathClass.TEST:
            continue
        shared = source_tokens & _path_tokens(test.path)
        if len(shared) >= need_path:
            ways.append("changed test file %s shares %s with the source path"
                        % (test.path, ", ".join(sorted(shared))))
            break
    need_name = int(policy.number("name_overlap"))
    symbol_tokens = PUB.name_tokens(symbol.name)
    for tokens in new_case_tokens:
        shared = symbol_tokens & tokens
        if len(shared) >= need_name:
            ways.append("new test case name shares %s" % ", ".join(sorted(shared)))
            break
    if policy.flag("mention_in_test") and item.language:
        leaf = symbol.leaf().lower()
        if len(leaf) >= 4 and any(leaf in (test.added_test_text or "") for test in test_files):
            ways.append("`%s` is named in lines this episode added to a test file" % leaf)
        elif len(leaf) < 4:
            for tokens in [_path_tokens(path) for path in
                           [test.path for test in test_files]]:
                if len(symbol_tokens & tokens) >= 2:
                    ways.append("short name `%s` matched by two path tokens" % leaf)
                    break
    if mentions and symbol.leaf().lower() in mentions:
        ways.append("the repository's existing tests already mention `%s`" % symbol.leaf())
    return ways


def _coverage_evidence(symbol: PUB.Symbol, policy: Policy) -> list[str]:
    return ["%s at line %d: %d body line(s), %d of them logic"
            % (symbol.kind, symbol.line, symbol.body_lines, symbol.logic_lines),
            "thresholds in force: min_body_lines=%s, require_logic=%s, scope=%s"
            % (policy.number("min_body_lines"),
               "true" if policy.flag("require_logic") else "false",
               policy.text("scope")),
            "no changed test file matched its path or name, and its name is not in any line "
            "this episode added to a test file"]


def _tokens(text: str) -> set[str]:
    return PUB.name_tokens(text)


def _path_tokens(path: str) -> set[str]:
    stem = path.rsplit("/", 1)[-1]
    directory = path.rsplit("/", 1)[0] if "/" in path else ""
    stem = stem.split(".")[0]
    tokens = PUB.name_tokens(stem) | PUB.name_tokens(directory.rsplit("/", 1)[-1])
    return {token for token in tokens if token not in ("test", "tests", "spec", "src", "lib",
                                                       "index")}


# ---------------------------------------------------------------------------
# the proxy
# ---------------------------------------------------------------------------
def _proxy(view: SurfaceDelta, policy: Policy) -> Proxy:
    """Which half of the story is measured, and how much to believe it."""
    surfaces = [item for item in view.files if item.is_test_surface]
    languages = sorted({item.language for item in surfaces if item.language})
    unparsed_surfaces = [item for item in surfaces
                         if item.language == "" or item.parser_state
                         in (P.STATE_UNSUPPORTED, P.STATE_FAILED)
                         or not (item.case_diff and item.case_diff.language)]
    parsed_surfaces = [item for item in surfaces if item not in unparsed_surfaces]
    unparsed = [item.path for item in unparsed_surfaces]
    if not surfaces:
        return Proxy(basis=BASIS_NO_TEST, confidence=1.0,
                     statement="no test surface changed in this episode: the line counts "
                               "below are exact and no case-level claim is made",
                     evidence=["no changed path was classified `test` and no parsed test "
                               "module was found inside a source file"],
                     languages=[], unparsed=[])
    parsed = parsed_surfaces
    if not unparsed:
        return Proxy(basis=BASIS_PARSED, confidence=float(policy.number(
            "proxy_confidence_parsed")),
            statement="test surface measured from named cases in %s — %d file(s) parsed"
                      % (", ".join(languages) or "unknown language", len(surfaces)),
            evidence=["every changed test file had a parser and was parsed into named cases",
                      "frameworks seen: %s" % ", ".join(sorted(
                          {item.framework for item in surfaces if item.framework})
                          or ["none recorded"])],
            languages=languages, unparsed=[])
    if parsed:
        return Proxy(basis=BASIS_PARTIAL, confidence=float(policy.number(
            "proxy_confidence_partial")),
            statement="%d of %d test file(s) parsed into named cases; %d had no parser or "
                      "refused, so case counts are a lower bound"
                      % (len(parsed), len(surfaces), len(unparsed)),
            evidence=["named cases are known for %s" % ", ".join(
                item.path for item in parsed[:5]) or "no file",
                      "line counts only for %s" % ", ".join(unparsed[:5])],
            languages=languages, unparsed=unparsed)
    return Proxy(basis=BASIS_LINES, confidence=float(policy.number(
        "proxy_confidence_lines_only")),
        statement="no test framework this tool can parse is present in this episode: the "
                  "test surface is a line count only, and a line count is not a coverage "
                  "number",
        evidence=["no parser for: %s" % ", ".join(sorted(
            {item.path.rsplit(".", 1)[-1] if "." in item.path else item.path
             for item in surfaces}))],
        languages=[], unparsed=unparsed)


def severity_order() -> tuple[str, ...]:
    return SEVERITY_ORDER


def _arrow(change_type: str) -> str:
    return {"added": "+", "deleted": "\u2212", "renamed": "\u2192",
            "modified": "~"}.get(change_type, "\u00b7")
