"""The non-code card: the surface summary and its checklist, with an auditor on the output.

`e2.10` composes the episode card; what lives here is the *non-code* half of it, rendered to
text so the wording has one owner. The auditor (:func:`audit`) re-reads the produced lines and
fails the render if it lost something the model said, because every one of these is the kind
of line a later refactor quietly drops while every test still passes:

1. the header counts — findings, actions, blocking — and the surfaces they are spread over;
2. **every finding row keeps its name and its `path:line`**, since the location is the part a
   reviewer clicks and a finding without one is a rumour (`policy.Finding` already refuses to
   exist without it; this says the render must not lose it either);
3. the checklist verdict — `requires operator action: yes (7 action(s))` — and, when the
   episode changed only durable state, the words **needs deploy coordination**;
4. the stated confidence, always, including the episodes where the answer is "nothing found
   and here is how much of it I could read";
5. rows cut off by `non_code.card.max_actions` are counted and named, never truncated in
   silence.

Rows wrap with a hanging indent instead of clipping. That is not cosmetics: the clipped token
was the location, which is the one thing on the line a reviewer needs, and a 40-column pane
must show the same facts as a 120-column one.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .policy import SURFACES, SEVERITY_ORDER
from .checklist import Audit, audit as audit_checklist
from .checklist import render_owner_summary

DEFAULT_COLS = 100
RULE = "─" * 8

_SEVERITY_MARK = {"high": "!!", "notable": " ! ", "note": " . "}


@dataclass
class Render:
    """One card render plus the audit that goes with it."""

    lines: list[str] = field(default_factory=list)
    problems: list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.problems

    def text(self) -> str:
        return "\n".join(self.lines)


def render(view, *, cols: int = DEFAULT_COLS, max_actions: int | None = None,
           header: bool = True, show_all: bool = False) -> list[str]:
    """The card. Deterministic: same episode in, same bytes out."""
    lines: list[str] = []
    if header:
        lines.append("%s non-code surfaces · %s" % (RULE, view.episode_id or "episode"))
        if view.title:
            lines.extend(_wrap("  %s" % view.headline(), cols))
    lines.extend(_wrap("  " + view.summary_line(), cols))
    counts = view.counts()
    census = view.surfaces
    lines.extend(_wrap("  files: %d changed · %d on a non-code surface (%s)"
                       % (counts["files"], counts["files_with_surface"],
                          " · ".join("%s %d" % (name, census.get(name, 0))
                                     for name in SURFACES)), cols))
    lines.extend(_wrap("  findings by severity: %s"
                       % (" · ".join("%s %d" % (name, counts["findings_by_severity"][name])
                                     for name in SEVERITY_ORDER) or "none"), cols))
    lines.extend(_finding_rows(view, cols))
    cap = len(view.checklist.actions) if show_all else (
        max_actions if max_actions is not None
        else int(view.policy.number("max_actions")))
    lines.extend(_wrap("  " + " ".join(view.checklist.header()), cols))
    for action in view.checklist.actions[:cap]:
        lines.extend(_wrap_lines(action.block(indent=2), cols))
    hidden = len(view.checklist.actions) - cap
    if hidden > 0:
        lines.append("    … %d more: fathom noncode checklist %s --all"
                     % (hidden, view.episode_id or "<episode>"))
    owners = render_owner_summary(view.checklist)
    if owners:
        lines.extend(_wrap("  owners: %s" % " · ".join(owners), cols))
    lines.extend(_wrap("  confidence: %.2f (%s)" % (view.confidence, _basis(view)), cols))
    for kind, count in sorted(view.suppressed.items()):
        lines.extend(_wrap("  %d %s finding(s) not reported: %s is off (%s)"
                           % (count, kind, kind,
                              view.suppressed_source.get(kind, "policy")), cols))
    for note in view.notes:
        lines.extend(_wrap("  note: %s" % note, cols))
    for partial in view.partials:
        lines.extend(_wrap("  partial: %s" % partial, cols))
    return lines


def _finding_rows(view, cols: int) -> list[str]:
    lines: list[str] = []
    if not view.findings:
        return ["  findings: none"]
    lines.append("  findings (%d)" % len(view.findings))
    current = None
    for finding in view.findings:
        if finding.surface != current:
            current = finding.surface
            lines.append("    %s" % current)
        lines.extend(_wrap("      %s %s" % (_SEVERITY_MARK[finding.severity],
                                            finding.render()), cols))
        for evidence in finding.evidence:
            if evidence and evidence != finding.detail:
                lines.extend(_wrap("        · %s" % evidence, cols))
    return lines


def _basis(view) -> str:
    if not view.findings:
        if view.partials:
            return "%d file(s) could not be fully read, so nothing-found is a weaker claim " \
                   "than it looks" % len(view.partials)
        return "nothing found on the surfaces surveyed, and every reader finished"
    return "severity-weighted mean of the %d findings' own confidence" % len(view.findings)


def _clip(text: str, cols: int) -> str:
    if cols <= 0 or len(text) <= cols:
        return text
    return text[: max(1, cols - 1)] + "…"


def _wrap_lines(blocks: list[str], cols: int) -> list[str]:
    out: list[str] = []
    for block in blocks:
        out.extend(_wrap(block, cols))
    return out


def _wrap(text: str, cols: int) -> list[str]:
    if cols <= 0 or len(text) <= cols:
        return [text]
    pad = " " * (len(text) - len(text.lstrip(" ")))
    words = [word for word in text.strip().split(" ") if word]
    lines: list[str] = []
    current = ""
    for word in words:
        candidate = ("%s %s" % (current, word)) if current else word
        if current and len(pad + candidate) > cols:
            lines.append(pad + current)
            current = word
        else:
            current = candidate
    lines.append(pad + current)
    return lines


def audit(lines: list[str], view=None, max_actions: int | None = None,
          show_all: bool = False) -> Audit:
    """Fail a card that lost its counts, its locations, its verdict or its confidence."""
    if view is None:
        result = Audit(problems=["audit refused: no delta to audit against"])
        result.checked = len(lines)
        return result
    if show_all:
        cap = len(view.checklist.actions)
    else:
        cap = max_actions if max_actions is not None else int(view.policy.number("max_actions"))
    result = audit_checklist(lines, checklist=view.checklist, max_actions=cap)
    flat = " ".join(" ".join(lines).split())
    counts = view.counts()
    if "%d finding" % counts["findings"] not in flat:
        result.problems.append("the finding count is missing from the render")
    if "%d need an operator" % counts["actions"] not in flat:
        result.problems.append("the operator-action count is missing from the render")
    if "confidence:" not in flat:
        result.problems.append("the card states no confidence")
    if "%.2f" % view.confidence not in flat:
        result.problems.append("the stated confidence number is missing")
    for finding in view.findings:
        if finding.name not in flat:
            result.problems.append("finding %s (%s) lost its name"
                                   % (finding.kind, finding.name))
        if finding.location not in flat:
            result.problems.append("finding %s (%s) lost its location"
                                   % (finding.kind, finding.name))
    for kind, count in view.suppressed.items():
        if "%s is off" % kind not in flat:
            result.problems.append("suppressed %s findings were not explained" % kind)
    return result
