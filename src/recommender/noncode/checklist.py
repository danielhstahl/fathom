"""The roll-up: a per-episode **requires operator action** checklist (`e2.8` AC 4).

A finding is a fact with a location. An action is what somebody who is not in the code review
has to do, in an order, with their name on it. The ticket asks for the second one, and the
difference is not cosmetic: the reviewer reads the card once, and the checklist is the artefact
that survives into a release ticket, a chat message at 17:40, and the runbook of whoever gets
paged when it was not done.

So a checklist row is `[ ] severity owner action — subject at path:line`, and three rules make
it usable rather than merely long:

1. **One row per action, not per finding.** Four references to the same missing secret are one
   thing to do. Duplicate rows are how a checklist stops being read, so identical actions on
   the same subject merge and their evidence goes with them — the merge is visible, which is
   why the row says `(from 4 findings)`.
2. **Order is severity, then surface, then path** — the same order the card uses, so the
   checklist and the card can be read side by side. High severity rows come first because a
   deploy has a point of no return and the migration ordering row has to be seen before the
   deploy, not after.
3. **Nothing disappears.** Rows past `non_code.card.max_actions` are counted and named, and a
   finding whose switch is off is reported as suppressed rather than silently missing. An
   unchecked box you never saw is worse than a long list.

`audit()` re-checks the rendered text: a row without its checkbox, owner, action verb or
location fails, as does a render that dropped rows or omitted the "requires operator action"
verdict. The CLI runs the same audit on its own output and exits non-zero from it, and the
contract suite hands it deliberately broken renders so the invariant has a test that can
fail rather than a test that cannot.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from .policy import (OWNER_RELEASE, SEVERITY_HIGH, SEVERITY_NOTABLE, SEVERITY_ORDER,
                     SURFACES, Finding)

CHECKBOX = "[ ]"
DEPLOY_COORDINATION_WORDS = "needs deploy coordination"


@dataclass
class Action:
    """One thing an operator has to do, with where to check it and who owns it."""

    text: str
    subject: str
    owner: str
    severity: str
    surface: str
    kind: str
    path: str
    line: int
    confidence: float
    evidence: list[str] = field(default_factory=list)
    from_findings: int = 1
    surface_rank: int = 0

    @property
    def location(self) -> str:
        return "%s:%d" % (self.path, self.line) if self.line else self.path

    def render(self) -> str:
        parts = ["%s %-8s %-9s %s" % (CHECKBOX, self.severity, self.owner, self.text),
                 "— %s" % self.subject, "at %s" % self.location]
        if self.from_findings > 1:
            parts.append("(from %d findings)" % self.from_findings)
        if self.confidence < 0.999:
            parts.append("(confidence %.2f)" % self.confidence)
        return " · ".join(parts)

    def block(self, indent: int = 4) -> list[str]:
        pad = " " * indent
        return [pad + self.render()] + [pad + "  · %s" % item for item in self.evidence if item]

    def as_dict(self) -> dict:
        return {"action": self.text, "subject": self.subject, "owner": self.owner,
                "severity": self.severity, "surface": self.surface, "kind": self.kind,
                "path": self.path, "line": self.line, "location": self.location,
                "confidence": round(self.confidence, 3), "from_findings": self.from_findings,
                "evidence": list(self.evidence)}

    @property
    def order(self) -> tuple:
        return (SEVERITY_ORDER.index(self.severity), self.surface_rank, self.path, self.line,
                self.kind, self.subject)


@dataclass
class Checklist:
    """The episode's operator actions, ready to render, count and audit."""

    episode_id: str
    actions: list[Action] = field(default_factory=list)
    coordination: bool = False
    suppressed: dict[str, int] = field(default_factory=dict)
    totals: dict[str, int] = field(default_factory=dict)

    @property
    def requires_operator_action(self) -> bool:
        return bool(self.actions)

    def by_owner(self) -> dict[str, list[Action]]:
        out: dict[str, list[Action]] = {}
        for action in self.actions:
            out.setdefault(action.owner, []).append(action)
        return {key: out[key] for key in sorted(out)}

    def by_severity(self) -> dict[str, int]:
        out = {name: 0 for name in SEVERITY_ORDER}
        for action in self.actions:
            out[action.severity] += 1
        return out

    def header(self) -> list[str]:
        verdict = "yes" if self.requires_operator_action else "no"
        lines = ["requires operator action: %s (%d action%s)"
                 % (verdict, len(self.actions), "" if len(self.actions) == 1 else "s")]
        if self.coordination:
            lines.append("%s: yes — this episode changes durable state and needs an ordering "
                         "decision before it ships" % DEPLOY_COORDINATION_WORDS)
        if self.totals.get("suppressed"):
            lines.append("%d finding(s) not shown because their switch is off: %s"
                         % (self.totals["suppressed"],
                            ", ".join("%s=%d" % pair for pair in sorted(self.suppressed.items()))))
        return lines

    def lines(self, *, max_actions: int | None = None, header: bool = True) -> list[str]:
        out = self.header() if header else []
        cap = len(self.actions) if max_actions is None else max_actions
        for action in self.actions[:cap]:
            out.extend(action.block(indent=2))
        hidden = len(self.actions) - cap
        if hidden > 0:
            out.append("  … %d more: fathom noncode checklist %s --all"
                       % (hidden, self.episode_id or "<episode>"))
        if not self.actions:
            out.append("  nothing to do: no non-code surface this tool can see was changed")
        return out

    def as_dict(self) -> dict:
        return {"episode_id": self.episode_id,
                "requires_operator_action": self.requires_operator_action,
                "deploy_coordination": self.coordination,
                "actions": [action.as_dict() for action in self.actions],
                "by_severity": self.by_severity(),
                "by_owner": {key: [action.as_dict() for action in value]
                             for key, value in self.by_owner().items()},
                "suppressed": dict(sorted(self.suppressed.items())),
                "totals": dict(self.totals)}


def build(delta) -> Checklist:
    """Fold a delta's findings into the checklist: merge, order, count."""
    merged: dict[tuple, Action] = {}
    for finding in delta.findings:
        key = (_normal(finding.action), finding.owner, finding.name, finding.surface)
        existing = merged.get(key)
        if existing is None:
            merged[key] = Action(text=finding.action, subject=finding.headline,
                                 owner=finding.owner, severity=finding.severity,
                                 surface=finding.surface, kind=finding.kind,
                                 path=finding.path, line=finding.line,
                                 confidence=finding.confidence,
                                 evidence=list(finding.evidence),
                                 surface_rank=SURFACES.index(finding.surface))
            continue
        existing.from_findings += 1
        existing.confidence = max(existing.confidence, finding.confidence)
        for item in finding.evidence:
            if item not in existing.evidence:
                existing.evidence.append(item)
    actions = sorted(merged.values(), key=lambda action: action.order)
    coordination = any(finding.kind == "deploy-coordination" for finding in delta.findings)
    counts = delta.counts() if hasattr(delta, "counts") else {}
    return Checklist(episode_id=delta.episode_id, actions=actions, coordination=coordination,
                     suppressed=dict(delta.suppressed),
                     totals={"actions": len(actions),
                             "findings": counts.get("findings", len(delta.findings)),
                             "suppressed": counts.get("suppressed",
                                                      sum(delta.suppressed.values()))})


@dataclass
class Audit:
    """What the auditor found wrong with one render."""

    problems: list[str] = field(default_factory=list)
    checked: int = 0

    @property
    def ok(self) -> bool:
        return not self.problems

    def as_dict(self) -> dict:
        return {"ok": self.ok, "checked": self.checked, "problems": list(self.problems)}


def audit(lines: list[str], checklist: Checklist | None = None,
          max_actions: int | None = None) -> Audit:
    """Fail a checklist render that lost a row, a verb, an owner or a location."""
    result = Audit()
    result.checked = len(lines)
    if checklist is None:
        result.problems.append("audit refused: no checklist to audit against")
        return result
    flat = " ".join(" ".join(lines).split())
    if "requires operator action:" not in flat:
        result.problems.append("the render never states whether an operator is required")
    if checklist.coordination and DEPLOY_COORDINATION_WORDS not in flat:
        result.problems.append("the episode needs deploy coordination and the render does not "
                               "say so")
    cap = len(checklist.actions) if max_actions is None else max_actions
    shown = checklist.actions[:cap]
    rows = [line for line in lines if line.strip().startswith(CHECKBOX)]
    if len(rows) < len(shown):
        result.problems.append("%d action row(s) rendered without a checkbox"
                               % (len(shown) - len(rows)))
    for action in shown:
        if action.owner not in flat:
            result.problems.append("action %r lost its owner" % action.text)
        if _clip(action.text, 24) not in flat:
            result.problems.append("action %r lost its verb" % action.text)
        if action.location not in flat:
            result.problems.append("action %r (%s) lost its location"
                                   % (action.text, action.subject))
    hidden = len(checklist.actions) - len(shown)
    if hidden > 0 and "%d more" % hidden not in flat:
        result.problems.append("%d action(s) were dropped without saying so" % hidden)
    for kind, count in checklist.suppressed.items():
        if kind not in flat:
            result.problems.append("%d suppressed %s finding(s) were not explained"
                                   % (count, kind))
    return result


def _clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[:width]


def _normal(text: str) -> str:
    return re.sub(r"\s+", " ", (text or "").strip().lower())


def render_owner_summary(checklist: Checklist) -> list[str]:
    """`platform 3 · database 2 · release 1` — the line that goes in a release ticket."""
    return ["%s %d" % (owner, len(actions)) for owner, actions in
            sorted(checklist.by_owner().items())]
