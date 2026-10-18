"""The card itself: sections, figures, the evidence map, and the numbers re-derived to check them.

This is the composition, and it is deliberately boring: seven builders, one per section in
`policy.SECTIONS`, each of which reads one analysis, emits figures, and attaches the evidence
behind each. Three invariants are enforced by construction rather than by prose:

* **no unlinked figures** — `Figure` refuses to exist without a link, which is what "every
  number is a button" means at 2 a.m. when someone adds an eighth section;
* **no evidence that does not say its number** — `_mk` raises if a figure's `key` token is not
  present in the rows behind its own link, so a figure cannot be bolted to unrelated receipts;
* **no section without a state** — `Section` refuses `measured` without a lead and figures, and
  refuses `unavailable` without a reason and the verb that would fill it, so a missing analysis
  produces a printed gap instead of a silent one.

:meth:`Card.verify` then re-reads the assembled card: it resolves every link, re-checks every
figure's key against its evidence, and — the part that is an actual oracle — **re-derives the
headline numbers from the analyses by a second path** (per-file objects rather than the
`counts()` roll-ups the builders read, plus `totals()` beside `counts()`). A card whose roll-up
and per-file arithmetic disagree is a card that prints a number nobody can reproduce, and
`verify` names the figure.

The digest is sha256 over the card's canonical JSON: the byte-determinism claim made checkable,
and ADR-004 §5.5's cache key. The policy digest is inside the payload, so a knob change moves it
— a card cached across a `blast.hops` change would be a card about a different question.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field

from . import blast as BLAST
from .links import (LINKABLE_VIEWS, VIEW_BLAST, VIEW_CHECKLIST, VIEW_CLASSES, VIEW_DEPS,
                    VIEW_DIFF, VIEW_FINDING, VIEW_GRAPH, VIEW_NOISE, VIEW_POLICY, VIEW_STRUCTURE,
                    VIEW_SURFACE, VIEW_TESTS, EvidenceMap, Link, Resolution)
from .model import Absent, Analyses, Episode
from .policy import (SECTION_ATTENTION, SECTION_BLAST, SECTION_DEPENDENCIES, SECTION_INDEX,
                     SECTION_SURFACE, SECTION_TESTS, SECTION_WHAT, SECTION_WHERE,
                     STATE_MEASURED, STATE_PARTIAL, STATE_UNAVAILABLE, UnanchoredSection,
                     UnlinkedFigure, Policy, severity_rank)

CARD_VERSION = "fathom-card/1"

#: How many per-path links a figure may carry before the remainder is counted. A figure with
#: 400 buttons is a file list wearing a figure costume.
MAX_PATH_LINKS = 3

#: What the one-screen block says when a section could not be measured at all. The long reason
#: belongs to the `why:` line under it; the section column stays one scannable clause, which is
#: the only way seven sections fit a screen when five of them are missing.
GAP_LABEL = {"noise": "no noise analysis", "structure": "no structure delta",
             "tests": "no test-surface delta", "test surface": "no test-surface delta",
             "noncode": "no non-code delta", "non-code surface": "no non-code delta",
             "classes": "no path classes", "dependencies": "no dependency delta",
             "module graph": "no module graph"}

#: Non-code owns the words "`this episode` needs deploy coordination"
#: (`recommender.noncode.checklist.DEPLOY_COORDINATION_WORDS`). The card repeats them rather
#: than inventing a second phrasing, so one episode cannot be described two ways.
DEPLOY_COORDINATION_WORDS = "needs deploy coordination"


@dataclass(frozen=True)
class Figure:
    """One number on the card, with its buttons."""

    label: str
    value: str
    links: tuple[Link, ...] = ()
    raw: dict = field(default_factory=dict)
    key: str = ""
    confidence: float | None = None
    basis: str = ""
    note: str = ""

    def __post_init__(self) -> None:
        if not self.label:
            raise UnlinkedFigure("a card figure must have a label to print")
        if not str(self.value).strip():
            raise UnlinkedFigure("%s: a figure with no value is a heading, not a number"
                                 % self.label)
        if not self.links:
            raise UnlinkedFigure("%s: refusing to print a figure with no evidence link — every "
                                 "number on this card is a button" % self.label)
        if not self.key:
            raise UnlinkedFigure("%s: a figure must name the token its evidence has to "
                                 "contain" % self.label)
        for item in self.links:
            if item.view not in LINKABLE_VIEWS:
                raise UnlinkedFigure("%s links to an unregistered view %r"
                                     % (self.label, item.view))
        if self.confidence is not None and not 0.0 < float(self.confidence) <= 1.0:
            raise UnlinkedFigure("%s: confidence must be in (0, 1], not %r"
                                 % (self.label, self.confidence))

    @property
    def available(self) -> bool:
        return any(item.available() for item in self.links)

    def as_dict(self) -> dict:
        out = {"label": self.label, "value": self.value,
               "links": [item.as_dict() for item in self.links]}
        if self.raw:
            out["raw"] = dict(self.raw)
        if self.key:
            out["evidence_key"] = self.key
        if self.confidence is not None:
            out["confidence"] = round(float(self.confidence), 3)
        if self.basis:
            out["basis"] = self.basis
        if self.note:
            out["note"] = self.note
        return out


@dataclass
class Section:
    """One answer: state, headline, figures, and — when it could not be measured — why and how."""

    id: str
    title: str
    state: str = STATE_MEASURED
    lead: str = ""
    short: str = ""
    figures: list[Figure] = field(default_factory=list)
    owner: str = ""
    reason: str = ""
    run: str = ""
    confidence: float | None = None
    basis: str = ""
    notes: list[str] = field(default_factory=list)
    #: the `--all` payload: everything the section's links point at, in one block.
    detail: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        spec = SECTION_INDEX.get(self.id)
        if spec is None:
            raise UnanchoredSection("no card section %r is declared in policy.SECTIONS"
                                    % self.id)
        if self.state not in (STATE_MEASURED, STATE_PARTIAL, STATE_UNAVAILABLE):
            raise UnanchoredSection("%s: state must be measured, partial or unavailable"
                                    % self.id)
        if self.state in (STATE_MEASURED, STATE_PARTIAL):
            if not self.lead:
                raise UnanchoredSection("%s: a %s section has nothing to say"
                                        % (self.id, self.state))
            if not self.figures:
                raise UnanchoredSection("%s: a %s section with no figures is a heading"
                                        % (self.id, self.state))
        elif not self.reason or not self.run:
            raise UnanchoredSection("%s: an unavailable section must say why and name the verb "
                                    "that fills it" % self.id)
        if self.confidence is not None and not 0.0 < float(self.confidence) <= 1.0:
            raise UnanchoredSection("%s: confidence must be in (0, 1]" % self.id)

    @property
    def spec(self):
        return SECTION_INDEX[self.id]

    @property
    def brief(self) -> str:
        """The section's answer in one short clause, for the summary block."""
        if self.short:
            return self.short
        return self.lead.split(" · ")[0] if self.lead else self.reason

    @property
    def measured(self) -> bool:
        return self.state in (STATE_MEASURED, STATE_PARTIAL)

    def as_dict(self) -> dict:
        out = {"id": self.id, "title": self.title, "state": self.state, "owner": self.owner,
               "figures": [figure.as_dict() for figure in self.figures]}
        if self.brief:
            out["brief"] = self.brief
        for name in ("lead", "reason", "run", "basis"):
            value = getattr(self, name)
            if value:
                out[name] = value
        if self.confidence is not None:
            out["confidence"] = round(float(self.confidence), 3)
        if self.notes:
            out["notes"] = list(self.notes)
        if self.detail:
            out["detail"] = list(self.detail)
        return out


@dataclass
class Card:
    """The one-screen answer for one episode."""

    episode_id: str
    sections: list[Section] = field(default_factory=list)
    title: str = ""
    evidence: EvidenceMap = field(default_factory=EvidenceMap)
    policy: Policy = field(default_factory=Policy.built_in)
    version: str = CARD_VERSION
    analysis_source: str = ""
    episode_lines: dict = field(default_factory=dict)
    hidden: list[tuple[str, str]] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    #: kept for `verify()`, which re-derives from these rather than from the figures.
    analyses: Analyses | None = None
    episode: Episode | None = None
    blast: BLAST.Blast | None = None

    # -- accessors ----------------------------------------------------------
    def section(self, section_id: str) -> Section | None:
        for section in self.sections:
            if section.id == section_id:
                return section
        return None

    def figures(self) -> list[tuple[Section, Figure]]:
        return [(section, figure) for section in self.sections for figure in section.figures]

    def links(self) -> list[Link]:
        out: list[Link] = []
        for _, figure in self.figures():
            for item in figure.links:
                if item not in out:
                    out.append(item)
        return out

    def states(self) -> dict[str, str]:
        return {section.id: section.state for section in self.sections}

    def measured_count(self) -> int:
        return sum(1 for section in self.sections if section.measured)

    def headline(self) -> str:
        """The one line a rail or a roll-up shows."""
        parts = [self._lead(section) for section in self.sections if section.measured]
        return "%s · %s" % (self.episode_id, " · ".join(parts)) if parts \
            else "%s · nothing could be measured" % self.episode_id

    def summary_line(self) -> str:
        return self.headline()

    def _lead(self, section: Section) -> str:
        lead = section.lead or ""
        prefix = "%s · " % self.episode_id
        if lead.startswith(prefix):
            lead = lead[len(prefix):]
        return "%s %s" % (section.id, lead.split(" · ")[0] if lead else "")

    # -- determinism --------------------------------------------------------
    def canonical(self) -> str:
        return json.dumps(self.payload(), indent=2, sort_keys=True, default=str)

    def digest(self) -> str:
        return hashlib.sha256(self.canonical().encode("utf-8")).hexdigest()[:16]

    def links_resolve(self) -> list[Resolution]:
        return [self.evidence.resolve(item) for item in self.links()]

    def verify(self) -> list[str]:
        """Every cross-check the card can make about itself. Empty means it holds together."""
        problems: list[str] = []
        for broken in self.links_resolve():
            if not broken.ok:
                problems.append("broken link: %s (%s)"
                                % (broken.link.key(), broken.reason or "no evidence"))
        for section in self.sections:
            for figure in section.figures:
                rows = " ".join(" ".join(self.evidence.resolve(item).lines)
                                for item in figure.links)
                if figure.key and figure.key not in rows:
                    problems.append(
                        "%s/%s: the evidence behind it no longer contains %r, so it does not "
                        "support the figure" % (section.id, figure.label, figure.key))
        problems.extend(self._recheck())
        return problems

    def _recheck(self) -> list[str]:
        """Re-derive the headline numbers by a second path and compare them to the figures."""
        analyses = self.analyses
        problems: list[str] = []
        if analyses is None:
            return problems

        def expect(section_id, label, key, value):
            section = self.section(section_id)
            if section is None:
                return
            for figure in section.figures:
                if figure.label != label:
                    continue
                got = figure.raw.get(key)
                if got is None or abs(float(got) - float(value)) > 1e-9:
                    problems.append("%s/%s: the card prints %r, the second path says %r"
                                    % (section_id, label, got, value))
                return

        if analyses.noise is not None:
            files = list(getattr(analyses.noise, "files", []) or [])
            expect(SECTION_WHAT, "files touched", "files", len(files))
            expect(SECTION_WHAT, "semantic lines", "semantic_lines",
                   sum(item.semantic_lines for item in files))
            expect(SECTION_WHAT, "lines changed", "changed_lines",
                   sum(item.changed_lines for item in files))
        if analyses.structure is not None:
            totals = analyses.structure.totals()
            counts = analyses.structure.counts()
            expect(SECTION_WHERE, "lines that moved", "lines_moved", totals["lines_moved"])
            expect(SECTION_WHERE, "directories", "directories_touched",
                   counts["directories_touched"])
            churn = _figure_raw(self, SECTION_WHERE, "structure churn", "churn")
            if churn is not None and abs(float(churn)
                                         - float(analyses.structure.churn.score)) > 1e-9:
                problems.append("where/structure churn: the printed churn is not the measured "
                                "churn (%.3f)" % analyses.structure.churn.score)
            rows = analyses.structure.row_sums()
            if rows["lines_moved_in"] != totals["lines_moved"]:
                problems.append("where: the directory rows do not sum to the episode totals "
                                "for moved lines (%d vs %d)"
                                % (rows["lines_moved_in"], totals["lines_moved"]))
        if analyses.tests is not None:
            counts = analyses.tests.counts()
            expect(SECTION_TESTS, "cases", "tests_removed", counts["tests_removed"])
            expect(SECTION_TESTS, "cases", "tests_added", counts["tests_added"])
            expect(SECTION_TESTS, "test : production lines", "test_lines_added",
                   counts["test_lines_added"])
            surface = sum(item.test_added for item in analyses.tests.files) \
                if getattr(analyses.tests, "files", None) else counts["test_lines_added"]
            expect(SECTION_TESTS, "test : production lines", "test_lines_added", surface)
        if analyses.noncode is not None:
            counts = analyses.noncode.counts()
            expect(SECTION_SURFACE, "findings", "findings", counts["findings"])
            expect(SECTION_SURFACE, "operator actions", "actions", counts["actions"])
        if analyses.deps is not None and analyses.deps.measured:
            expect(SECTION_DEPENDENCIES, "direct dependencies", "direct_added",
                   analyses.deps.direct_added or 0)
        if self.blast is not None:
            expect(SECTION_BLAST, "direct dependents", "direct", len(self.blast.direct))
            expect(SECTION_BLAST, "one hop", "one_hop", len(self.blast.one_hop))
        return problems

    def payload(self) -> dict:
        """Everything the card asserts, minus the digest — the thing the digest is taken over."""
        return {
            "version": self.version,
            "episode": self.episode_id,
            "title": self.title,
            "policy_digest": self.policy.config_digest,
            "analyses": self.analysis_source,
            "episode_lines": dict(self.episode_lines),
            "sections": [section.as_dict() for section in self.sections],
            "states": self.states(),
            "measured": self.measured_count(),
            "links": len(self.links()),
            "evidence": self.evidence.as_dict(),
            "hidden_sections": [{"id": item[0], "source": item[1]} for item in self.hidden],
            "notes": list(self.notes),
            "blast": self.blast.as_dict() if self.blast is not None else {},
        }

    def as_dict(self) -> dict:
        out = self.payload()
        out["digest"] = self.digest()
        return out


def _figure_raw(card: Card, section_id: str, label: str, key: str):
    section = card.section(section_id)
    if section is None:
        return None
    for figure in section.figures:
        if figure.label == label:
            return figure.raw.get(key)
    return None


# ---------------------------------------------------------------------------
# the run
# ---------------------------------------------------------------------------
def compose(episode: Episode, analyses: Analyses | None = None,
            policy: Policy | None = None, *, taxonomy=None) -> Card:
    """Build the card for one episode from the analyses the caller has (or can compute)."""
    active = policy or Policy.built_in()
    given = analyses or Analyses()
    contentless = bool(episode.files) and not any(change.has_bytes for change in episode.files)
    if contentless:
        given = _mark_record_only(given, episode)
    elif _needs_fill(given):
        given = _fill(given, episode)
    card = Card(episode_id=episode.id, title=episode.title, policy=active, analyses=given,
                episode=episode,
                analysis_source=given.source or "caller supplied",
                episode_lines=episode.lines.as_dict(), hidden=active.hidden_sections())
    evidence = card.evidence
    paths = _path_index(episode, given)
    builders = {SECTION_WHAT: _what, SECTION_WHERE: _where,
                SECTION_DEPENDENCIES: _dependencies, SECTION_SURFACE: _surface,
                SECTION_TESTS: _tests, SECTION_ATTENTION: _attention}
    for spec in _SECTION_ORDER:
        if not active.enabled_for(spec.id):
            continue
        if spec.id == SECTION_BLAST:
            card.blast = BLAST.measure(episode, active, graph=given.graph, taxonomy=taxonomy)
            section = _blast(card.blast, evidence, active)
        else:
            section = builders[spec.id](episode, given, active, evidence, paths)
        if section is not None:
            card.sections.append(section)
    printed = frozenset(section.reason for section in card.sections
                        if section.state == STATE_UNAVAILABLE)
    card.notes.extend(_composition_notes(given, active, printed))
    return card


_SECTION_ORDER = tuple(SECTION_INDEX[section_id] for section_id in
                       (SECTION_WHAT, SECTION_WHERE, SECTION_DEPENDENCIES, SECTION_SURFACE,
                        SECTION_TESTS, SECTION_BLAST, SECTION_ATTENTION))


def _needs_fill(analyses: Analyses) -> bool:
    return any(getattr(analyses, name) is None for name in
               ("noise", "structure", "tests", "noncode", "classes"))


def _fill(analyses: Analyses, episode: Episode) -> Analyses:
    """Compute the missing pure analyses, leaving everything the caller supplied alone."""
    from .model import analyse
    filled = analyse(episode, noise=analyses.noise, structure=analyses.structure,
                     tests=analyses.tests, noncode=analyses.noncode, classes=analyses.classes,
                     class_confidence=analyses.class_confidence, deps=analyses.deps,
                     graph=analyses.graph, risk=analyses.risk, absent=analyses.absent)
    filled.extra_findings = analyses.extra_findings
    filled.source = "computed at card time"
    return filled


def _mark_record_only(analyses: Analyses, episode: Episode) -> Analyses:
    """A record-only episode: the content analyses are unavailable, and must say so.

    This is the shape a cold spine hands the card — file records and counts, no blobs. Running
    the content analyses over absent bytes would produce zeros, and a zero the tool cannot
    distinguish from "nothing changed" is the exact lie the product is built to avoid.
    """
    reason = ("the index stores no content for this episode (path records and line counts "
              "only), so no content analysis could run")
    verb = "fathom show %s --patch" % episode.id
    absent = dict(analyses.absent)
    for name, label in (("noise", "noise"), ("structure", "structure"),
                        ("tests", "test surface"), ("noncode", "non-code surface"),
                        ("classes", "classes")):
        if getattr(analyses, name) is None and label not in absent:
            absent[label] = Absent(label, reason, run=verb)
    if analyses.deps is None and "dependencies" not in absent:
        absent["dependencies"] = Absent("dependencies", reason, run=verb, ticket="e2.6")
    analyses.absent = absent
    return analyses


# ---------------------------------------------------------------------------
# the figure factory
# ---------------------------------------------------------------------------
def _mk(evidence: EvidenceMap, link: Link, rows, label: str, value: str, raw: dict,
        key: str, *, extra: tuple[Link, ...] = (), **kwargs) -> Figure:
    """Build a figure *and* the evidence behind it, and prove the two agree.

    The `key` must appear in the attached rows: a figure bolted to receipts that do not say its
    number is the failure mode this whole package is here to prevent, so it is a construction
    error rather than a test.
    """
    rows = [str(row) for row in rows if str(row).strip()]
    if not rows:
        raise UnlinkedFigure("%s: no evidence rows to attach" % label)
    if key not in " ".join(rows):
        raise UnlinkedFigure("%s: its evidence does not contain %r, so the evidence does not "
                             "say what the figure says" % (label, key))
    evidence.attach(link, rows)
    for item in extra:
        evidence.attach(item, rows)
    return Figure(label=label, value=value, links=(link,) + tuple(extra), raw=dict(raw),
                  key=key, **kwargs)


def _path_index(episode: Episode, analyses: Analyses) -> dict[str, dict]:
    """Everything a figure needs to know about one changed path, in one pass."""
    noise_by_path = {item.path: item for item in (getattr(analyses.noise, "files", []) or [])}
    out: dict[str, dict] = {}
    for change in episode.files:
        noise_item = noise_by_path.get(change.path)
        info = {"path": change.subject, "change": change.change_type,
                "lines": change.lines.as_dict(), "changed": change.lines.changed,
                "has_bytes": change.has_bytes, "cls": "", "detail": []}
        if noise_item is not None:
            info["cls"] = getattr(noise_item, "cls", "") or ""
            info["semantic_lines"] = noise_item.semantic_lines
            info["folded"] = noise_item.folded
            info["rule"] = noise_item.rule
            info["noise"] = getattr(noise_item, "noise", "")
            info["detail"] = list(noise_item.detail)
        out[change.subject] = info
    return out


def _busiest(paths: dict[str, dict], limit: int = MAX_PATH_LINKS) -> list[str]:
    rows = sorted(paths.values(), key=lambda item: (-item["changed"], item["path"]))
    return [item["path"] for item in rows[:limit]]


def _diff_rows(path: str, info: dict) -> list[str]:
    lines = info["lines"]
    out = ["`%s` · %s · +%d/−%d line(s)" % (path, info["change"], lines["added"],
                                            lines["removed"])]
    if info.get("cls"):
        out.append("classified `%s` by `e0.9`/`e2.1`" % info["cls"])
    if info.get("folded"):
        out.append("folded as %s: %s" % (info.get("rule"),
                                         "; ".join(info.get("detail") or [])
                                         or "see the fold row"))
    elif info.get("semantic_lines") is not None:
        out.append("%d of %d changed line(s) are semantic after `e2.2`"
                   % (info["semantic_lines"], info["changed"]))
    if not info.get("has_bytes", True):
        out.append("no content in the index for this path: counts come from the file record")
    return out


def _absent(analyses: Analyses, analysis: str, section: str, default_reason: str,
            run: str) -> Section:
    spec = SECTION_INDEX[section]
    known = analyses.absent.get(analysis)
    reason = known.reason if known else default_reason
    verb = known.run if known and known.run else run
    return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                   short=GAP_LABEL.get(analysis, "not measured"), reason=reason, run=verb)


# ---------------------------------------------------------------------------
# section builders
# ---------------------------------------------------------------------------
def _what(episode: Episode, analyses: Analyses, policy: Policy,
          evidence: EvidenceMap, paths: dict[str, dict]) -> Section | None:
    spec = SECTION_INDEX[SECTION_WHAT]
    noise = analyses.noise
    if noise is None:
        return _absent(analyses, "noise", SECTION_WHAT,
                       "noise suppression was not run for this episode",
                       "python3 -m recommender.noise episode")
    if not episode.files:
        return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                       short="nothing to analyse",
                       reason="this episode contains no files, so there is nothing to count",
                       run="fathom show %s" % episode.id)
    counts = noise.counts()
    headline = noise.headline()
    files_link = Link(VIEW_NOISE, "%s#episode" % episode.id,
                      verb="python3 -m recommender.noise episode")
    files_rows = ["what `e2.2` folded for this episode: %s" % headline,
                  "%s" % noise.summary_line()]
    files_rows += ["file %s: %s" % (item.path, "; ".join(item.detail) or "no fold")
                   for item in noise.files][:8]
    busiest = _busiest(paths)
    path_links = []
    for path in busiest:
        link = Link(VIEW_DIFF, path)
        evidence.attach(link, _diff_rows(path, paths[path]))
        path_links.append(link)
    figures = [
        _mk(evidence, files_link, files_rows + ["files touched %d file(s), %d with semantic "
                                                "change" % (counts["files"],
                                                            counts["semantic_files"])],
            "files touched", "%d file(s), %d with semantic change"
            % (counts["files"], counts["semantic_files"]),
            {"files": counts["files"], "semantic_files": counts["semantic_files"],
             "visible_files": counts["visible_files"]},
            "%d file(s)" % counts["files"], extra=tuple(path_links[:1]),
            basis="`e2.2` suppression, per file"),
        _mk(evidence, Link(VIEW_NOISE, "%s#lines" % episode.id,
                           verb="python3 -m recommender.noise episode"),
            ["changed lines %d line(s) (+%d/−%d across %d path(s))"
             % (counts["changed_lines"], episode.lines.added, episode.lines.removed,
                len(paths)),
             "per-file totals: %s" % (", ".join("`%s` +%d/−%d"
                                                % (path, paths[path]["lines"]["added"],
                                                   paths[path]["lines"]["removed"])
                                                for path in busiest) or "no paths")],
            "lines changed", "%d line(s) (+%d/−%d across %d path(s))"
            % (counts["changed_lines"], episode.lines.added, episode.lines.removed, len(paths)),
            {"changed_lines": counts["changed_lines"], "added": episode.lines.added,
             "removed": episode.lines.removed},
            "%d line(s)" % counts["changed_lines"], extra=tuple(path_links[:2])),
        _mk(evidence, Link(VIEW_NOISE, "%s#semantic" % episode.id,
                           verb="python3 -m recommender.noise episode"),
            ["semantic lines %d of %d changed line(s) survive noise suppression"
             % (counts["semantic_lines"], counts["changed_lines"]),
             "folded out: %d line(s) over %d file(s)" % (counts["folded_lines"],
                                                         counts["folded_files"])],
            "semantic lines", "%d of %d changed line(s) survive noise suppression"
            % (counts["semantic_lines"], counts["changed_lines"]),
            {"semantic_lines": counts["semantic_lines"],
             "folded_lines": counts["folded_lines"]},
            "semantic lines %d of %d" % (counts["semantic_lines"], counts["changed_lines"])),
    ]
    folded = counts["folded_files"]
    share = (float(counts["folded_lines"]) / float(counts["changed_lines"])
             if counts["changed_lines"] else 0.0)
    figures.append(
        _mk(evidence, Link(VIEW_POLICY, "noise.suppress.*"),
            ["noise share %s of changed lines folded as reformat, generated or lockfile churn "
             "(%d file(s))" % (policy.state(share), folded),
             "folded by rule: %s" % (_pairs(counts["by_rule"]) or "nothing folded"),
             "formatters seen: %s" % (_pairs(counts["by_formatter"]) or "none")],
            "noise share", "%s of changed lines folded as reformat, generated or lockfile churn "
                           "(%d file(s))" % (policy.state(share), folded),
            {"noise_share": share, "folded_files": folded},
            "noise share %s" % policy.state(share),
            basis="`benchmarks/budgets.json`-independent: this is a count, not a threshold"))
    notes: list[str] = []
    if analyses.classes:
        pairs = _pairs(analyses.classes)
        figures.append(
            _mk(evidence, Link(VIEW_CLASSES, "%s#episode" % episode.id,
                               verb="python3 -m recommender.change episode"),
                ["path classes: %s" % pairs,
                 "mean classification confidence %s"
                 % (policy.state(analyses.class_confidence)
                    if analyses.class_confidence else "not reported")],
                "path classes", pairs or "nothing classified", dict(analyses.classes), pairs,
                confidence=analyses.class_confidence or None))
    if counts.get("collapsed"):
        notes.append("this episode is noise-only: `e2.2` collapsed it to one line, and every "
                     "figure above is the collapsed view")
    state = STATE_PARTIAL if (counts.get("collapsed") or not paths) else STATE_MEASURED
    short = "%d file(s), %d semantic line(s)%s" % (
        counts["files"], counts["semantic_lines"],
        ", %d folded" % counts["folded_files"] if counts["folded_files"] else "")
    return Section(id=spec.id, title=spec.title, state=state, lead=headline, short=short,
                   figures=figures, owner=spec.owner,
                   confidence=analyses.class_confidence or None,
                   basis="`e2.2` folds, then `e0.9`/`e2.1` classes", notes=notes,
                   detail=[noise.summary_line()] + list(noise.detail()))


def _where(episode: Episode, analyses: Analyses, policy: Policy,
           evidence: EvidenceMap, paths: dict[str, dict]) -> Section | None:
    spec = SECTION_INDEX[SECTION_WHERE]
    view = analyses.structure
    if view is None:
        return _absent(analyses, "structure", SECTION_WHERE,
                       "the folder-structure delta was not computed for this episode",
                       "python3 -m recommender.structure card")
    if not episode.files:
        return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                       short="nothing to analyse",
                       reason="this episode contains no files, so nothing landed anywhere",
                       run="fathom show %s" % episode.id)
    counts = view.counts()
    summary = view.summary()
    dirs_rows = _dir_rows(view)
    story_rows = _story_rows(view, summary)
    figures = [
        _mk(evidence, Link(VIEW_STRUCTURE, "%s#dirs" % episode.id,
                           verb="python3 -m recommender.structure dirs"),
            dirs_rows + ["directories %d touched (%d added, %d removed)"
                         % (counts["directories_touched"], counts["directories_added"],
                            counts["directories_removed"])],
            "directories", "%d touched (%d added, %d removed)"
            % (counts["directories_touched"], counts["directories_added"],
               counts["directories_removed"]),
            {"directories_touched": counts["directories_touched"],
             "directories_added": counts["directories_added"],
             "directories_removed": counts["directories_removed"]},
            "%d touched" % counts["directories_touched"]),
        _mk(evidence, Link(VIEW_STRUCTURE, "%s#stories" % episode.id,
                           verb="python3 -m recommender.structure stories"),
            story_rows + ["lines that moved %d line(s) travelled with %d moved file(s)"
                          % (counts["lines_moved"], counts["files_moved"])],
            "lines that moved", "%d line(s) travelled with %d moved file(s)"
            % (counts["lines_moved"], counts["files_moved"]),
            {"lines_moved": counts["lines_moved"], "files_moved": counts["files_moved"]},
            "%d line(s)" % counts["lines_moved"]),
        _mk(evidence, Link(VIEW_STRUCTURE, "%s#churn" % episode.id,
                           verb="python3 -m recommender.structure card"),
            list(view.churn.lines()),
            "structure churn", "%s (%s)" % (policy.state(view.churn.score), view.churn.band),
            {"churn": view.churn.score}, "%s" % policy.state(view.churn.score),
            basis="five printed components"),
        _mk(evidence, Link(VIEW_STRUCTURE, "%s#basis" % episode.id,
                           verb="python3 -m recommender.structure card"),
            ["tree basis: %s" % view.basis, "confidence %s" % policy.state(view.confidence)]
            + list(view.confidence_reasons()),
            "census basis", view.basis, {"confidence": view.confidence}, view.basis,
            confidence=view.confidence,
            note="; ".join(view.confidence_reasons()[:2])),
    ]
    notes = list(view.notes) + list(view.partials)
    problems = view.verify()
    if problems:
        notes.append("the directory table does not reconcile to the episode totals: %s"
                     % problems[0])
    state = STATE_PARTIAL if problems or view.partials else STATE_MEASURED
    short = "%s, %d dir(s), churn %s (%s)" % (
        "%d story" % counts["stories"], counts["directories_touched"],
        policy.state(view.churn.score), view.churn.band)
    return Section(id=spec.id, title=spec.title, state=state, lead=view.headline(), short=short,
                   figures=figures, owner=spec.owner, confidence=view.confidence,
                   basis=view.basis, notes=notes,
                   detail=[summary["sentence"]] + dirs_rows)


def _dependencies(episode: Episode, analyses: Analyses, policy: Policy,
                  evidence: EvidenceMap, paths: dict[str, dict]) -> Section | None:
    spec = SECTION_INDEX[SECTION_DEPENDENCIES]
    deps = analyses.deps
    if deps is None:
        return _absent(analyses, "dependencies", SECTION_DEPENDENCIES,
                       "no dependency delta was supplied for this episode (`e2.6` computes it "
                       "in `fathom-analyze::deps`)",
                       "fathom analyze %s" % episode.id)
    if not deps.measured:
        return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                       reason="a dependency delta was supplied but reports no counts at all, "
                              "which is not the same as zero changes",
                       run="fathom analyze %s" % episode.id)
    counts = deps.counts()
    headline = deps.headline or "dependency delta over %s" % (", ".join(deps.ecosystems)
                                                              or "unspecified ecosystems")
    figures: list[Figure] = []
    if deps.direct_added is not None or deps.direct_removed is not None:
        figures.append(
            _mk(evidence, Link(VIEW_DEPS, "%s#direct" % episode.id),
                [headline] + list(deps.evidence)
                + ["direct dependencies %s added, %s removed (basis: %s, source: %s)"
                   % (_maybe(deps.direct_added, "+"), _maybe(deps.direct_removed, "−"),
                      deps.basis, deps.source)],
                "direct dependencies", "%s added, %s removed"
                % (_maybe(deps.direct_added, "+"), _maybe(deps.direct_removed, "−")),
                {"direct_added": deps.direct_added or 0,
                 "direct_removed": deps.direct_removed or 0},
                "%s added, %s removed" % (_maybe(deps.direct_added, "+"),
                                          _maybe(deps.direct_removed, "−"))))
    if deps.upgraded is not None or deps.downgraded is not None:
        figures.append(
            _mk(evidence, Link(VIEW_DEPS, "%s#versions" % episode.id),
                ["version moves %s upgraded, %s downgraded (integrity changes %s)"
                 % (_maybe(deps.upgraded), _maybe(deps.downgraded),
                    _maybe(deps.integrity_changed))] + list(deps.evidence),
                "version moves", "%s upgraded, %s downgraded"
                % (_maybe(deps.upgraded), _maybe(deps.downgraded)),
                {"upgraded": deps.upgraded or 0, "downgraded": deps.downgraded or 0},
                "%s upgraded, %s downgraded" % (_maybe(deps.upgraded),
                                                _maybe(deps.downgraded))))
    if deps.transitive_only is not None:
        figures.append(
            _mk(evidence, Link(VIEW_DEPS, "%s#transitive" % episode.id),
                ["transitive-only moves %d package(s) moved without a manifest change "
                 "(conflicts %s)" % (deps.transitive_only, _maybe(deps.conflicts))],
                "transitive-only moves",
                "%d package(s) moved without a manifest change" % deps.transitive_only,
                {"transitive_only": deps.transitive_only},
                "transitive-only moves %d" % deps.transitive_only))
    if deps.source_switched is not None or deps.licence_changed is not None:
        figures.append(
            _mk(evidence, Link(VIEW_DEPS, "%s#supply" % episode.id),
                ["supply-chain surface %d source switch(es), %s licence-set change(s)"
                 % (deps.source_switched or 0, _maybe(deps.licence_changed))],
                "supply-chain surface", "%d source switch(es), %s licence-set change(s)"
                % (deps.source_switched or 0, _maybe(deps.licence_changed)),
                {"source_switched": deps.source_switched or 0,
                 "licence_changed": deps.licence_changed or 0},
                "supply-chain surface %d source switch(es)" % (deps.source_switched or 0)))
    if deps.ecosystems:
        figures.append(
            _mk(evidence, Link(VIEW_DEPS, "%s#ecosystems" % episode.id),
                ["ecosystems parsed: %s (unparseable files %s)"
                 % (", ".join(deps.ecosystems), _maybe(deps.unparseable))],
                "ecosystems", "%s parsed" % ", ".join(deps.ecosystems),
                {"ecosystems": len(deps.ecosystems)},
                "ecosystems parsed: %s" % ", ".join(deps.ecosystems)))
    notes = []
    if deps.unknown():
        notes.append("unknown, not zero: %s" % ", ".join(deps.unknown()))
    if deps.unparseable:
        notes.append("%d dependency file(s) could not be parsed in this episode — `e2.5`'s "
                     "`unparsed` rows carry the raw preview" % deps.unparseable)
    state = STATE_PARTIAL if notes else STATE_MEASURED
    short = "%s added, %s removed, %s upgraded" % (
        _maybe(deps.direct_added, "+"), _maybe(deps.direct_removed, "−"),
        _maybe(deps.upgraded))
    return Section(id=spec.id, title=spec.title, state=state, lead=headline, short=short,
                   figures=figures, owner=spec.owner, basis=deps.basis, notes=notes,
                   detail=[headline] + list(deps.evidence))


def _surface(episode: Episode, analyses: Analyses, policy: Policy,
             evidence: EvidenceMap, paths: dict[str, dict]) -> Section | None:
    spec = SECTION_INDEX[SECTION_SURFACE]
    view = analyses.noncode
    if view is None:
        return _absent(analyses, "non-code surface", SECTION_SURFACE,
                       "the non-code surface delta was not computed for this episode",
                       "python3 -m recommender.noncode card")
    if not episode.files:
        return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                       short="nothing to analyse",
                       reason="this episode contains no files, so no surface could move",
                       run="fathom show %s" % episode.id)
    counts = view.counts()
    surfaces = view.surfaces_touched()
    pairs = _surface_pairs(counts["findings_by_surface"])
    severity = _pairs(counts["findings_by_severity"]) or "none at any severity"
    findings_rows = [view.summary_line(), view.headline(),
                     "surfaces touched %s" % (pairs or "no non-code surface moved"),
                     "findings %d (%s)" % (counts["findings"], severity)]
    findings_rows += ["%s: %s · %s" % (finding.kind, finding.name, finding.location)
                      for finding in view.findings][:8]
    checklist_rows = ["%d action(s), %d blocking" % (counts["actions"], counts["blocking"])] \
        + list(view.checklist.lines())[:8]
    figures = [
        _mk(evidence, Link(VIEW_SURFACE, "%s#findings" % episode.id,
                           verb="python3 -m recommender.noncode findings"),
            findings_rows, "surfaces touched",
            pairs or "no non-code surface moved",
            {"surfaces": len([name for name, value in counts["findings_by_surface"].items()
                              if value]),
             "files_with_surface": counts["files_with_surface"]},
            pairs or "no non-code surface moved"),
        _mk(evidence, Link(VIEW_SURFACE, "%s#severity" % episode.id,
                           verb="python3 -m recommender.noncode findings"),
            ["findings %d (%s)" % (counts["findings"], severity)]
            + ["%s: %s · %s" % (finding.kind, finding.name, finding.location)
               for finding in view.findings][:6],
            "findings", "%d (%s)" % (counts["findings"], severity),
            {"findings": counts["findings"]}, "%d (%s)" % (counts["findings"], severity)),
    ]
    if counts["actions"]:
        figures.append(
            _mk(evidence, Link(VIEW_CHECKLIST, "%s#checklist" % episode.id,
                               verb="python3 -m recommender.noncode checklist"),
            checklist_rows, "operator actions",
                "%d action(s), %d blocking" % (counts["actions"], counts["blocking"]),
                {"actions": counts["actions"], "blocking": counts["blocking"]},
                "%d action(s)" % counts["actions"]))
    notes = []
    if counts["suppressed"]:
        notes.append("%d finding(s) suppressed by `non_code.findings.*` switches"
                     % counts["suppressed"])
    if counts["blocking"]:
        notes.append("%d operator action(s) block the merge until they are done"
                     % counts["blocking"])
    if view.requires_operator_action:
        notes.append("this episode cannot be reviewed from the diff alone: someone has to do "
                     "something outside it")
    if getattr(view.checklist, "coordination", False):
        notes.append("this episode %s: the order things land in is part of the change"
                     % DEPLOY_COORDINATION_WORDS)
    short = "%s · %d finding(s)%s" % (pairs or "no non-code surface", counts["findings"],
                                      ", %d operator action(s)" % counts["actions"]
                                      if counts["actions"] else "")
    return Section(id=spec.id, title=spec.title, state=STATE_MEASURED, short=short,
                   lead=view.summary_line(), figures=figures, owner=spec.owner,
                   confidence=view.confidence, basis="six surfaces, 27 finding kinds",
                   notes=notes, detail=view.checklist.lines())


def _tests(episode: Episode, analyses: Analyses, policy: Policy,
           evidence: EvidenceMap, paths: dict[str, dict]) -> Section | None:
    spec = SECTION_INDEX[SECTION_TESTS]
    view = analyses.tests
    if view is None:
        return _absent(analyses, "test surface", SECTION_TESTS,
                       "the test-surface delta was not computed for this episode",
                       "python3 -m recommender.testsurface card")
    if not episode.files:
        return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                       short="nothing to analyse",
                       reason="this episode contains no files, so no testing contract moved",
                       run="fathom show %s" % episode.id)
    counts = view.counts()
    ratio = view.ratio()
    ratio_rows = ["test : production lines %s (%d test / %d production line(s) added)"
                  % (ratio.get("words", "no ratio"), counts["test_lines_added"],
                     counts["prod_lines_added"]),
                  "ratio %s · basis: %s" % (_ratio_text(ratio), ratio.get("basis", ""))]
    figures = [
        _mk(evidence, Link(VIEW_TESTS, "%s#ratio" % episode.id,
                           verb="python3 -m recommender.testsurface card"),
            ratio_rows, "test : production lines",
            "%s (%d test / %d production line(s) added)"
            % (ratio.get("words", "no ratio"), counts["test_lines_added"],
               counts["prod_lines_added"]),
            {"test_lines_added": counts["test_lines_added"],
             "prod_lines_added": counts["prod_lines_added"], "ratio": ratio.get("ratio")},
            "%d test / %d production line(s) added" % (counts["test_lines_added"],
                                                      counts["prod_lines_added"]),
            basis=ratio.get("basis", "")),
        _mk(evidence, Link(VIEW_TESTS, "%s#cases" % episode.id,
                           verb="python3 -m recommender.testsurface card"),
            ["cases %d added, %d removed, %d skipped, %d weakened"
             % (counts["tests_added"], counts["tests_removed"], counts["tests_skipped"],
                counts["assertions_weakened"]),
             "case detail: +%d added, −%d removed, %d skipped, %d un-skipped, %d focused"
             % (counts["tests_added"], counts["tests_removed"], counts["tests_skipped"],
                counts["tests_unskipped"], counts["tests_focused"]),
             "assertions: %d weakened, %d lost, %d added"
             % (counts["assertions_weakened"], counts["assertions_lost"],
                counts["assertions_added"])],
            "cases", "%d added, %d removed, %d skipped, %d weakened"
            % (counts["tests_added"], counts["tests_removed"], counts["tests_skipped"],
               counts["assertions_weakened"]),
            {"tests_added": counts["tests_added"], "tests_removed": counts["tests_removed"],
             "tests_skipped": counts["tests_skipped"],
             "assertions_weakened": counts["assertions_weakened"]},
            "%d added, %d removed" % (counts["tests_added"], counts["tests_removed"])),
    ]
    if counts["test_files_deleted"] or counts["tests_removed"] or counts["assertions_weakened"]:
        figures.append(
            _mk(evidence, Link(VIEW_TESTS, "%s#findings" % episode.id,
                               verb="python3 -m recommender.testsurface findings"),
                ["test findings %d finding(s) about the tests themselves" % len(view.findings)]
                + ["%s: %s · %s" % (finding.kind, finding.name, finding.location)
                   for finding in view.findings],
                "test findings", "%d finding(s) about the tests themselves" % len(view.findings),
                {"findings": len(view.findings)},
                "%d finding(s)" % len(view.findings)))
    notes = []
    if view.proxy is not None:
        notes.append("coverage basis: %s (confidence %s)"
                     % (view.proxy.statement, policy.state(view.proxy.confidence)))
    if not counts["test_lines_added"] and not counts["prod_lines_added"]:
        notes.append("no test or production lines added: the ratio is undefined here and is "
                     "printed as absent, not as zero")
    proxy_partial = bool(view.proxy) and view.proxy.basis != "parsed-cases"
    short = "ratio %s (%d test / %d prod lines added)" % (
        ratio.get("words", "none"), counts["test_lines_added"], counts["prod_lines_added"])
    return Section(id=spec.id, title=spec.title,
                   state=STATE_PARTIAL if proxy_partial else STATE_MEASURED,
                   lead=view.headline(), short=short, figures=figures, owner=spec.owner,
                   confidence=view.confidence,
                   basis=view.proxy.statement if view.proxy else "line attribution",
                   notes=notes, detail=view.detail())


def _blast(blast: BLAST.Blast, evidence: EvidenceMap, policy: Policy) -> Section | None:
    spec = SECTION_INDEX[SECTION_BLAST]
    if not blast.measured:
        return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                       reason=blast.basis_reason or "no basis for a reach estimate",
                       run="python3 -m recommender.card blast %s" % blast.episode_id)
    direct_rows = blast.evidence.get("direct", []) + \
        ["direct dependents %d module(s) use what changed" % len(blast.direct)]
    one_rows = blast.evidence.get("one-hop", []) + \
        ["one hop %d more reached through the direct ring" % len(blast.one_hop)]
    figures = [
        _mk(evidence, Link(VIEW_BLAST, "%s#direct" % blast.episode_id), direct_rows,
            "direct dependents", "%d module(s) use what changed" % len(blast.direct),
            {"direct": len(blast.direct)}, "%d module(s)" % len(blast.direct),
            basis=blast.basis, confidence=blast.confidence),
        _mk(evidence, Link(VIEW_BLAST, "%s#one-hop" % blast.episode_id), one_rows,
            "one hop", "%d more reached through the direct ring" % len(blast.one_hop),
            {"one_hop": len(blast.one_hop)}, "%d more reached" % len(blast.one_hop),
            basis=blast.basis, confidence=blast.confidence,
            note="blast.hops = %d" % policy.integer("hops") if blast.beyond else ""),
        _mk(evidence, Link(VIEW_BLAST, "%s#basis" % blast.episode_id),
            blast.evidence.get("basis", []),
            "basis", "%s, confidence %s" % (blast.basis, policy.state(blast.confidence)),
            {"confidence": blast.confidence}, blast.basis, confidence=blast.confidence,
            basis=blast.basis_reason),
    ]
    notes = list(blast.notes)
    for partial in blast.partials:
        notes.append("partial: %s" % partial)
    cap_rows = policy.integer("max_rows")
    for label, rows in (("direct", blast.direct), ("one hop", blast.one_hop)):
        if len(rows) > cap_rows:
            notes.append("%d %s dependents over blast.max_rows = %d are counted in the figures "
                         "and not listed (`fathom card blast %s --all`)"
                         % (len(rows) - cap_rows, label, cap_rows, blast.episode_id))
    if blast.graph_edges:
        figures.append(
            _mk(evidence, Link(VIEW_GRAPH, "%s#dependents" % blast.episode_id),
                ["graph coverage %d of %d changed path(s) have graph nodes (%d node(s), %d "
                 "edge(s))" % (blast.mapped_paths, blast.changed_paths, blast.graph_nodes,
                               blast.graph_edges)] + blast.evidence.get("basis", []),
                "graph coverage", "%d of %d changed path(s) have graph nodes"
                % (blast.mapped_paths, blast.changed_paths),
                {"mapped": blast.mapped_paths, "changed": blast.changed_paths},
                "%d of %d changed path(s) have graph nodes"
                % (blast.mapped_paths, blast.changed_paths)))
    short = "%d direct, %d one-hop (%s)" % (len(blast.direct), len(blast.one_hop), blast.basis)
    return Section(id=spec.id, title=spec.title,
                   state=STATE_PARTIAL if blast.partials else STATE_MEASURED,
                   lead=blast.headline, short=short, figures=figures, owner=spec.owner,
                   confidence=blast.confidence, basis=blast.basis, notes=notes,
                   detail=blast.lines())


@dataclass(frozen=True)
class Attention:
    """One finding as the card sees it: where it came from, how heavy it is, and its proof."""

    source: str
    kind: str
    name: str
    severity: str
    location: str
    detail: str
    evidence: tuple[str, ...] = ()

    def sort_key(self) -> tuple:
        return (severity_rank(self.severity), self.source, self.kind, self.name, self.location)

    def render(self) -> str:
        return "[%s] %-28s %-30s %s" % (self.severity, self.kind, self.name[:30],
                                        self.location or "—")

    def as_dict(self) -> dict:
        return {"source": self.source, "kind": self.kind, "name": self.name,
                "severity": self.severity, "location": self.location, "detail": self.detail,
                "evidence": list(self.evidence)}


def collect_findings(analyses: Analyses) -> list[Attention]:
    """Every finding the supplied analyses raised, in one list, with its home kept.

    The three packages name findings the same way (`kind`, `name`, a location, evidence) but
    they are three different classes, so this adapter reads attributes rather than importing any
    of them — which also means `e4.3`'s findings need nothing here when they land.
    """
    out: list[Attention] = []
    for source, view in (("structure", analyses.structure), ("non_code", analyses.noncode),
                         ("test_surface", analyses.tests)):
        for finding in list(getattr(view, "findings", []) or []):
            out.append(Attention(
                source=source, kind=str(getattr(finding, "kind", "?")),
                name=str(getattr(finding, "name", "")), severity=str(_severity_of(finding)),
                location=str(getattr(finding, "location", "") or ""),
                detail=str(getattr(finding, "detail", "") or ""),
                evidence=tuple(str(row) for row in (getattr(finding, "evidence", ()) or ()))))
    for finding in analyses.extra_findings:
        out.append(Attention(source=str(getattr(finding, "source", "caller")),
                             kind=str(getattr(finding, "kind", "?")),
                             name=str(getattr(finding, "name", "")),
                             severity=str(_severity_of(finding)),
                             location=str(getattr(finding, "location", "") or ""),
                             detail=str(getattr(finding, "detail", "") or ""),
                             evidence=tuple(str(row) for row in
                                            (getattr(finding, "evidence", ()) or ()))))
    out.sort(key=lambda item: item.sort_key())
    return out


def _severity_of(finding) -> str:
    severity = getattr(finding, "severity", None)
    if severity:
        return str(severity)
    return str(getattr(getattr(finding, "rule", None), "severity", "note"))


def _attention(episode: Episode, analyses: Analyses, policy: Policy,
               evidence: EvidenceMap, paths: dict[str, dict]) -> Section | None:
    spec = SECTION_INDEX[SECTION_ATTENTION]
    if not episode.files:
        return Section(id=spec.id, title=spec.title, state=STATE_UNAVAILABLE, owner=spec.owner,
                       short="nothing to analyse",
                       reason="this episode contains no files, so nothing was raised about it",
                       run="fathom show %s" % episode.id)
    items = collect_findings(analyses)
    if not items and not any((analyses.noise, analyses.structure, analyses.tests,
                              analyses.noncode)):
        # Every content analysis is missing, so "no findings" describes the inputs, not the
        # episode. Printing that as a measured zero is the lie this section exists to prevent.
        # A missing `deps` or `graph` does not count: they raise no findings of their own here.
        return _absent(analyses, "noise", spec.id,
                       "no content analysis was supplied, so nothing could be raised — an empty "
                       "attention section here is not the same as a clean episode",
                       "fathom show %s --patch" % episode.id)
    floor = policy.attn_floor()
    shown = [item for item in items if severity_rank(item.severity) <= floor]
    hidden = len(items) - len(shown)
    risk = analyses.risk
    counts: dict[str, int] = {}
    for item in items:
        counts[item.severity] = counts.get(item.severity, 0) + 1
    notes = []
    if risk is None:
        notes.append("no risk score supplied: these rows are in severity order, which is not "
                     "the queue order (`e4.4` scores that, `e4.5` ranks it)")
    else:
        tau = policy.integer("risk_tau")
        notes.append("risk %s is %s `risk.tau` = %d, so the queue %s it"
                     % (risk.score, "at or above" if risk.score >= tau else "below", tau,
                        "surfaces" if risk.score >= tau else "does not surface"))
    figures: list[Figure] = []
    if risk is not None:
        figures.insert(0, _mk(
            evidence, Link(VIEW_POLICY, "risk.tau"),
            ["risk score %s%s, and `risk.tau` is the threshold an episode is surfaced at"
             % (risk.score, " (rank %d of %d)" % (risk.rank, risk.of) if risk.of else ""),
             "`risk.tau` is %d: at or above it the queue surfaces the episode"
             % policy.integer("risk_tau"),
             "scored by %s" % risk.source] + list(risk.reasons),
            "risk score", "%s%s" % (risk.score,
                                    " (rank %d of %d)" % (risk.rank, risk.of) if risk.of else ""),
            {"risk": risk.score}, "risk score %s" % risk.score, basis=risk.source))
    cap = policy.integer("max_findings")
    for item in shown[:cap]:
        figures.append(_mk(
            evidence, Link(VIEW_FINDING, "%s/%s" % (item.source, item.kind)),
            ["%s · %s · %s (%s)" % (item.kind, item.name, item.location or "no anchor",
                                    item.source), item.detail] + list(item.evidence),
            item.kind, "%s · %s" % (item.name, item.location or "no anchor"),
            {"severity_rank": severity_rank(item.severity)}, item.name,
            basis=item.source, note=item.severity))
    if not figures:
        figures.append(_mk(
            evidence, Link(VIEW_POLICY, "card.attention.severity"),
            ["findings raised %d raised, none at or above `card.attention.severity = %s`"
             % (len(items), policy.text("attention_severity"))]
            + [item.render() for item in items][:6],
            "findings raised", "%d raised, none at or above `%s`"
            % (len(items), policy.text("attention_severity")),
            {"findings": len(items)}, "%d raised" % len(items)))
    lead = "%s · %d shown%s" % (
        _pairs(counts) or "no findings raised by any supplied analysis",
        min(len(shown), cap), " (%d below the attention floor)" % hidden if hidden else "")
    if hidden:
        notes.append("%d finding(s) at a lighter severity than card.attention.severity = %s are "
                     "counted and not listed (`fathom card attention %s --all`)"
                     % (hidden, policy.text("attention_severity"), episode.id))
    if len(shown) > cap:
        notes.append("%d attention row(s) over card.max_findings = %d: counted in the lead line "
                     "and in `--json`, not printed" % (len(shown) - cap, cap))
    short = "%d shown of %d raised%s" % (min(len(shown), cap), len(items),
                                         " (%s)" % _pairs(counts) if counts else "")
    # A missing risk score is a note, not a partial answer: severity order is a complete answer
    # to "what is heavy here". What makes the section partial is findings the floor or the cap
    # kept off the card, because then the card really is showing less than the analyses said.
    return Section(id=spec.id, title=spec.title,
                   state=STATE_PARTIAL if (hidden or len(shown) > cap) else STATE_MEASURED,
                   lead=lead, short=short, figures=figures, owner=spec.owner, notes=notes,
                   detail=[item.render() for item in shown])


def _composition_notes(analyses: Analyses, policy: Policy,
                       printed: frozenset[str] = frozenset()) -> list[str]:
    """Card-level notes: the gaps no section carried, and the switches that were thrown.

    A gap the reader already met in its own section is not repeated here — five sections each
    saying "the index stores no content" is five lines, and the card's whole subject is saying a
    thing once. What is left is what the sections could not say: `classes` has no section of its
    own, and a section switched off by configuration has no section at all.
    """
    notes: list[str] = []
    for key, absent in sorted(analyses.absent.items()):
        if absent.reason in printed:
            continue
        notes.append("absent: %s" % absent.sentence())
    for section_id, source in policy.hidden_sections():
        notes.append("section `%s` is switched off by %s (%s): the card would have answered it"
                     % (section_id, SECTION_INDEX[section_id].config_key, source))
    return notes


# ---------------------------------------------------------------------------
# small text helpers (deterministic by construction)
# ---------------------------------------------------------------------------
def _pairs(counts: dict | None) -> str:
    if not counts:
        return ""
    return ", ".join("%s %s" % (key, value) for key, value in sorted(counts.items()))


def _surface_pairs(counts: dict | None) -> str:
    if not counts:
        return ""
    out = " · ".join("%s %d" % (key, value) for key, value in sorted(counts.items()) if value)
    return out or ""


def _maybe(value, sign: str = "") -> str:
    if value is None:
        return "unknown"
    return "%s%s" % (sign, value) if value else "0"


def _ratio_text(ratio: dict) -> str:
    if ratio.get("ratio") is None:
        return "none: no test or production lines were added"
    return "%s (%s)" % (ratio.get("words", ""), ratio.get("test_to_prod", ""))


def _dir_rows(view) -> list[str]:
    out: list[str] = []
    for row in view.rows:
        if not row.active:
            continue
        out.append("%-34s %3d→%-3d files · +%d/−%d lines · moved in %d, out %d"
                   % (row.display, row.files_before, row.files_after, row.lines_added,
                      row.lines_removed, row.lines_moved_in, row.lines_moved_out))
    if not out:
        out.append("no directory has anything to report for this episode")
    return out


def _story_rows(view, summary: dict) -> list[str]:
    out = [summary["sentence"]]
    for clause in summary["clauses"]:
        out.append("· %s" % clause["text"])
        for row in clause["evidence"]:
            out.append("  – %s" % row)
    if not view.stories:
        out.append("no structural story: the episode changed contents where they already were")
    return out
