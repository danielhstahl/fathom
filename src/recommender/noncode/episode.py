"""One episode, all six surfaces, one answer: what has to happen before this ships.

`measure(episode)` is the whole ticket in one call:

    from recommender.noncode import Change, Episode, measure

    view = measure(Episode(id="ep-7c1", files=[Change(path=…, before=…, after=…)]))
    view.summary_line()
    # ep-7c1 · env 3 · migration 2 · ci 2 · 9 findings · 7 need an operator · 2 blocking
    view.findings          # every finding, severity-then-surface ordered
    view.checklist         # the roll-up AC 4 asks for: an operator-action checklist
    view.counts(); view.confidence; view.requires_operator_action

Four decisions live here rather than in the detectors, so the card, the checklist, the export
(`e5.7`) and the risk score (`e2.8`'s consumer `e2.12`) cannot each grow their own:

**Where the policy applies.** The detectors observe; they do not consult the switchboard. If
a disabled finding simply never got computed, "3 findings suppressed because
`non_code.findings.env_removed` is off" would be a guess about work nobody did. So the survey
runs with everything on, and this module partitions the result — which is also why a
repository that switches a finding off still sees that it *would* have fired.

**What a finding's confidence means here.** Each finding carries how sure its detector was,
and the episode's number is the severity-weighted mean of those, not a plain average: one
confident `migration-added` should not be diluted to mush by six `note`-severity rows, and
one uncertain high-severity row should not hide behind confident trivia. Episodes with nothing
to report still state a confidence, derived from how much of the episode the readers could
actually parse — "I found nothing" and "I could not read it" are different claims.

**What counts as needing an operator.** Every finding carries an action, by construction (see
`policy.Finding`), so any finding means the checklist is non-empty. `blocking` is the
high-severity count, which is the number a release gate reads.

**Order.** Severity, then surface, then path, then line. The same order in the card, the
checklist, the JSON and the TUI, because a reviewer who has seen one card should be able to
skim the next.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from . import ci as CI
from . import container as CONTAINER
from . import env as ENV
from . import flags as FLAGS
from . import iac as IAC
from . import migrations as MIGRATIONS
from .policy import (FINDING_IDS, SEVERITY_HIGH, SEVERITY_NOTABLE, SEVERITY_ORDER, SURFACES,
                     Finding, Policy, sort_key)
from .surfaces import (SURVEY_VERSION, Change, Episode, FileFacts, Run, line_diff)
from .checklist import Checklist, build as build_checklist

#: Version of the delta's shape and rules. A stored delta that disagrees is recomputed.
DELTA_VERSION = 1

_SEVERITY_WEIGHT = {SEVERITY_HIGH: 3, SEVERITY_NOTABLE: 2, "note": 1}


@dataclass
class NonCodeDelta:
    """Everything this ticket can say about one episode."""

    episode_id: str
    title: str
    policy: Policy
    run: Run
    findings: list[Finding] = field(default_factory=list)
    suppressed: dict[str, int] = field(default_factory=dict)
    suppressed_source: dict[str, str] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)
    surfaces: dict[str, int] = field(default_factory=dict)
    partials: list[str] = field(default_factory=list)
    checklist: Checklist = None                        # type: ignore[assignment]

    # -- the numbers --------------------------------------------------------
    def counts(self) -> dict:
        by_surface = {name: 0 for name in SURFACES}
        by_severity = {name: 0 for name in SEVERITY_ORDER}
        by_kind = {}
        for finding in self.findings:
            by_surface[finding.surface] += 1
            by_severity[finding.severity] += 1
            by_kind[finding.kind] = by_kind.get(finding.kind, 0) + 1
        return {"files": len(self.run.files),
                "files_with_surface": sum(1 for facts in self.run.files if facts.surfaces),
                "findings": len(self.findings),
                "findings_by_surface": by_surface,
                "findings_by_severity": by_severity,
                "findings_by_kind": by_kind,
                "suppressed": sum(self.suppressed.values()),
                "actions": len(self.checklist.actions) if self.checklist else 0,
                "blocking": by_severity[SEVERITY_HIGH],
                "notes": len(self.notes),
                "partials": len(self.partials)}

    def surfaces_touched(self) -> list[str]:
        """Surfaces with at least one *finding*, in checklist order."""
        hit = {finding.surface for finding in self.findings}
        return [name for name in SURFACES if name in hit]

    def files_on(self, surface: str) -> list[str]:
        return [facts.path for facts in self.run.files_on(surface)]

    @property
    def confidence(self) -> float:
        if self.findings:
            total = sum(_SEVERITY_WEIGHT[finding.severity] for finding in self.findings)
            weighted = sum(_SEVERITY_WEIGHT[finding.severity] * finding.confidence
                           for finding in self.findings)
            return round(weighted / total, 2) if total else 1.0
        # Nothing found: the honest claim is about how much could be read, not about the
        # absence of findings. Every partial reader costs a tenth, floored at 0.5.
        return round(max(0.5, 1.0 - 0.1 * len(self.partials)), 2)

    @property
    def requires_operator_action(self) -> bool:
        return bool(self.findings)

    def summary_line(self) -> str:
        parts = ["%s · non-code surfaces" % (self.episode_id or "episode")]
        touched = self.surfaces_touched()
        counts = self.counts()
        parts.append(" · ".join("%s %d" % (name, counts["findings_by_surface"][name])
                                for name in touched) or "nothing on any surface")
        parts.append("%d finding%s" % (counts["findings"], "" if counts["findings"] == 1
                                       else "s"))
        parts.append("%d need an operator" % counts["actions"])
        if counts["blocking"]:
            parts.append("%d blocking" % counts["blocking"])
        return " · ".join(parts)

    def headline(self) -> str:
        if not self.findings:
            if self.partials:
                return "no non-code finding, and %d file(s) could not be fully read"                        % len(self.partials)
            return "no non-code surface changed in this episode"
        ordered = sorted({finding.surface for finding in self.findings},
                         key=SURFACES.index)
        return "this episode changes %s and needs %d operator action(s)" \
               % (", ".join(ordered), self.counts()["actions"])

    def findings_for(self, surface: str) -> list[Finding]:
        return [finding for finding in self.findings if finding.surface == surface]

    def as_dict(self) -> dict:
        return {"episode_id": self.episode_id, "title": self.title,
                "delta_version": DELTA_VERSION, "survey_version": SURVEY_VERSION,
                "summary": self.summary_line(), "headline": self.headline(),
                "requires_operator_action": self.requires_operator_action,
                "blocking": self.counts()["blocking"],
                "confidence": self.confidence,
                "counts": self.counts(),
                "surfaces": self.surfaces,
                "files": [facts.as_dict() for facts in self.run.files],
                "findings": [finding.as_dict() for finding in self.findings],
                "suppressed": dict(sorted(self.suppressed.items())),
                "checklist": self.checklist.as_dict() if self.checklist else {},
                "notes": list(self.notes), "partials": list(self.partials),
                "config_digest": self.policy.config_digest}


def measure(episode: Episode, policy: Policy | None = None,
            *, classifier=None) -> NonCodeDelta:
    """Run every surface detector over one episode and roll the result up."""
    active = policy or Policy.built_in()
    observing = active.all_enabled()
    run = Run(episode, observing, classifier=classifier)
    produced: list[Finding] = []
    produced += ENV.detect(run)[0]
    produced += FLAGS.detect(run)[0]
    produced += MIGRATIONS.detect(run)[0]
    produced += CI.detect(run)
    produced += IAC.detect(run)
    produced += CONTAINER.detect(run)

    findings: list[Finding] = []
    suppressed: dict[str, int] = {}
    sources: dict[str, str] = {}
    for finding in produced:
        if active.enabled_for(finding.kind):
            findings.append(finding)
            continue
        suppressed[finding.kind] = suppressed.get(finding.kind, 0) + 1
        sources[finding.kind] = active.effective(finding.kind)[1]
    findings.sort(key=sort_key)

    delta = NonCodeDelta(episode_id=episode.id, title=episode.title, policy=active, run=run,
                         findings=findings, suppressed=suppressed, suppressed_source=sources,
                         surfaces=run.census())
    delta.notes = list(run.notes)
    delta.partials = _partials(run)
    delta.checklist = build_checklist(delta)
    return delta


def _partials(run: Run) -> list[str]:
    """Where a reader could not fully read a file — one row per file, never one per line.

    A Jenkinsfile the YAML reader cannot follow produces one honest sentence about the
    dialect plus a line per construct it skipped. Printing the eight lines as eight
    partials would say "eight files were not fully read" about one file, and would move
    `confidence` by eight tenths for one limitation. So: what the reader could not do, and
    how much of the file it happened to.
    """
    out: list[str] = []
    for facts in run.files:
        if not facts.notes:
            continue
        shape = [note for note in facts.notes if not note.startswith("line ")]
        skipped = [note for note in facts.notes if note.startswith("line ")]
        head = shape[0] if shape else "no reader claimed this file"
        if skipped:
            head += " (%d line(s) not followed, first: %s)" % (len(skipped), skipped[0])
        out.append("%s: %s" % (facts.path, head))
    return out


def aggregate(episodes, *, policy: Policy | None = None) -> dict:
    """Roll several episodes up: the shape `e2.10`'s card row and `e2.12`'s risk input want.

    Counts are summed and the confidence is weighted by findings, never averaged per episode:
    an episode with nothing on it would otherwise drag the number toward a confidence nobody
    earned.
    """
    active = policy or Policy.built_in()
    totals = {"episodes": 0, "findings": 0, "actions": 0, "blocking": 0,
              "requires_operator_action": 0}
    by_surface = {name: 0 for name in SURFACES}
    weighted = 0.0
    weight = 0.0
    for episode in episodes:
        view = measure(episode, active)
        counts = view.counts()
        totals["episodes"] += 1
        totals["findings"] += counts["findings"]
        totals["actions"] += counts["actions"]
        totals["blocking"] += counts["blocking"]
        totals["requires_operator_action"] += 1 if view.requires_operator_action else 0
        for name, count in counts["findings_by_surface"].items():
            by_surface[name] += count
        if counts["findings"]:
            weighted += view.confidence * counts["findings"]
            weight += counts["findings"]
    totals["findings_by_surface"] = by_surface
    totals["confidence"] = round(weighted / weight, 2) if weight else 1.0
    return totals
