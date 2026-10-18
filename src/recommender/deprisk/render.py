"""The dependency-risk card, and an auditor that refuses a card which lost one of its facts.

`e2.10` composes the episode's one screen; what lives here is the *dependency risk* half of it,
rendered to text so the wording has one owner. Six claims are re-read out of the finished lines by
:func:`audit`, because each is the kind of line a later refactor quietly drops while every test
still passes:

1. the freshness line — the advisory answer always says how old its evidence is, including when
   there was no cache at all;
2. **every "who asked for this" row prints its answer**, and an unanswered one prints
   `not recorded` rather than an empty space;
3. every licence violation prints its **offending edge path**, which is the half a reviewer can act
   on (`license-denied` without `fathom-analyze → jwt → ring` is a complaint, not a finding);
4. every finding row prints, or the remainder is counted and names the verb that lists the rest — a
   cap on rows, never a deletion;
5. the stated confidence prints always, including on the clean episodes;
6. the counts in the header are the counts in `view.counts()`, not a summary of a summary.

Rows wrap with a hanging indent rather than clipping, for the same reason the siblings do it: the
token that gets clipped is the one at the end of the line, and on this card that token is a path.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from . import osv as OSV
from .model import SEVERITY_ORDER
from .policy import (
    ADVISORY_FIX, ADVISORY_MISSING, ADVISORY_STALE, ADVISORY_UNCHECKED, INTENT_NOT_CAPTURED,
    LICENSE_DENIED, LICENSE_DRIFT, LICENSE_UNKNOWN, NEW_DIRECT, PROVENANCE_UNKNOWN)

DEFAULT_COLS = 100
RULE = "─" * 8

_MARK = {"critical": "!!", "high": "!!", "notable": " ! ", "note": " . "}

#: Which findings the card prints in its own sections, before the general findings list.
_SECTIONS = (NEW_DIRECT, PROVENANCE_UNKNOWN, INTENT_NOT_CAPTURED, LICENSE_DENIED,
             LICENSE_UNKNOWN, LICENSE_DRIFT, ADVISORY_FIX, ADVISORY_MISSING, ADVISORY_STALE,
             ADVISORY_UNCHECKED)


@dataclass
class Audit:
    """What the auditor found, if anything."""

    problems: list[str] = field(default_factory=list)
    checked: int = 0

    @property
    def ok(self) -> bool:
        return not self.problems

    def as_dict(self) -> dict:
        return {"ok": self.ok, "checked": self.checked, "problems": list(self.problems)}


def render(view, *, cols: int = DEFAULT_COLS, show_all: bool = False,
           header: bool = True) -> list[str]:
    """The card. Deterministic: the same view in, the same bytes out."""
    counts = view.counts()
    lines: list[str] = []
    if header:
        lines.append("%s dependency risk · %s" % (RULE, view.episode.episode.id or "episode"))
        if view.episode.episode.title:
            lines.extend(_wrap("  %s" % view.episode.episode.title, cols))
    lines.extend(_wrap("  " + view.summary()["sentence"], cols))
    if view.delta.headline:
        lines.extend(_wrap("  delta: %s" % view.delta.headline, cols))
    lines.extend(_wrap("  %s · consent: %s" % (view.freshness.words,
                                               view.policy.network.reason), cols))
    lines.extend(_wrap("  confidence %.2f · %d stated partial(s) · %d finding(s)"
                       % (view.confidence, counts["partials"], counts["findings"]), cols))

    lines.append("who asked for this")
    cap = (len(view.answers) if show_all else view.policy.integer("prov_max_rows"))
    if not view.answers:
        lines.append("  no new direct dependency: nothing to answer")
    for answer in view.answers[:cap]:
        lines.append("  %s (%s, %s)" % (answer.package.text,
                                         answer.package.ecosystem or "unknown ecosystem",
                                         answer.package.attribution))
        for row in answer.rows():
            lines.extend(_wrap("    " + row, cols))
    if len(view.answers) > cap:
        lines.append("  … %d more: fathom deprisk provenance --all" % (len(view.answers) - cap))

    lines.append("advisories")
    for row in _advisory_rows(view, cols, show_all=show_all):
        lines.append(row)

    lines.append("licence")
    cap = len(view.rows) if show_all else view.policy.integer("lic_max_rows")
    shown = [row for row in view.rows if row.state != "removed"]
    if not shown:
        lines.append("  no package left to check")
    for row in shown[:cap]:
        for line in row.rows(max_evidence=None if show_all
                             else view.policy.integer("card_max_evidence")):
            lines.extend(_wrap("  " + line, cols))
    if len(shown) > cap:
        lines.append("  … %d more: fathom deprisk license --all" % (len(shown) - cap))

    findings = view.findings
    if findings:
        census = counts["findings_by_severity"]
        lines.append("findings (%d) · %s" % (
            counts["findings"],
            " · ".join("%s %d" % (name, census.get(name, 0))
                       for name in SEVERITY_ORDER if census.get(name, 0))))
        cap = len(findings) if show_all else view.policy.integer("card_max_findings")
        for finding in findings[:cap]:
            for line in finding.block(indent=2,
                                      max_evidence=None if show_all
                                      else view.policy.integer("card_max_evidence")):
                lines.extend(_wrap(line, cols))
        if len(findings) > cap:
            lines.append("  … %d more: fathom deprisk findings --all" % (len(findings) - cap))
    else:
        lines.append("findings: none")
    if counts["suppressed"]:
        lines.extend(_wrap("  %d finding(s) suppressed by their own switch: %s"
                           % (counts["suppressed"],
                              ", ".join("%s %d" % pair for pair in
                                        sorted(counts["suppressed_by_rule"].items()))), cols))
    if view.partials:
        lines.append("stated partials")
        for item in view.partials:
            lines.extend(_wrap("  - %s" % item, cols))
    if view.notes:
        lines.append("notes")
        for item in view.notes[:6 if not show_all else len(view.notes)]:
            lines.extend(_wrap("  - %s" % item, cols))
    return lines


def _advisory_rows(view, cols: int, show_all: bool = False) -> list[str]:
    freshness = view.freshness
    rows = view.advisory_rows
    if freshness.state == "offline":
        return ["  advisories were not consulted: `deps.advisories.mode` is `offline`"]
    if not rows:
        return ["  nothing to ask about: no package in this delta was queryable"]
    lines = ["  cache: %s · %s" % (freshness.path or "not configured", freshness.label)]
    if freshness.state == "expired":
        lines[-1] += (", past `stale_limit_hours` — this answer does not claim ‘no "
                      "advisories’")
    elif freshness.state == "stale":
        lines[-1] += ", past `max_age_hours` — used anyway, age stated"
    elif freshness.state == "missing":
        lines[-1] += " — no cache to read, so nothing was checked"
    if freshness.missed:
        lines.append("  %d package(s) had no cached entry; %d were checked"
                     % (freshness.missed, freshness.checked))
    checked = [row for row in rows if row.checked and row.advisories]
    clean = [row for row in rows if row.checked and not row.advisories and not row.failed]
    failed = [row for row in rows if row.checked and row.failed]
    unchecked = [row for row in rows if not row.checked]
    cap = len(checked) if show_all else view.policy.integer("card_max_findings")
    for row in checked[:cap]:
        for advisory in row.advisories:
            mark = _MARK.get(advisory.severity, " . ")
            lines.extend(_wrap("  %s %s %s · %s %s — %s"
                               % (mark, row.lookup.package, row.lookup.version, advisory.id,
                                  advisory.severity, advisory.summary or "(no summary)"), cols))
            if advisory.has_fix:
                lines.append("      fix: %s (installed %s)" % (advisory.fixed_in,
                                                               row.lookup.version))
    if len(checked) > cap:
        lines.append("  … %d more package(s) with advisories: fathom deprisk advisories"
                     % (len(checked) - cap))
    if clean and freshness.claims_clean:
        lines.append("  no advisories for %d checked package(s) (cache %s)"
                     % (len(clean), freshness.label))
    elif clean:
        lines.append("  %d package(s) checked against a cache %s old — stated as not checked "
                     "recently, not as clean" % (len(clean), freshness.label))
    if failed:
        lines.extend(_wrap("  %d package(s) came back as a failed query, not as clean: %s"
                           % (len(failed),
                              ", ".join("%s %s (%s)" % (item.lookup.package, item.lookup.version,
                                                       (item.entry.error if item.entry else "")
                                                       or "the request failed")
                                        for item in failed[:3])
                              + ("…" if len(failed) > 3 else "")), cols))
    if unchecked:
        grouped: dict[str, list] = {}
        for row in unchecked:
            grouped.setdefault(row.reason, []).append(row)
        for reason, group in sorted(grouped.items()):
            lines.extend(_wrap("  .  %d not checked (%s): %s"
                               % (len(group), reason,
                                  ", ".join("%s %s" % (item.lookup.package, item.lookup.version)
                                            for item in group[:3])
                                  + ("…" if len(group) > 3 else "")), cols))
    if not view.policy.network.may_fetch:
        lines.append("  (network: %s)" % view.policy.network.reason)
    return lines


def audit(view, lines: list[str]) -> Audit:
    """Re-read the rendered card and fail it if it lost something the model said."""
    text = "\n".join(lines)
    counts = view.counts()
    audit_ = Audit()

    def need(condition: bool, problem: str) -> None:
        audit_.checked += 1
        if not condition:
            audit_.problems.append(problem)

    need(freshness_line_present(view, text),
         "the card lost its advisory freshness line")
    need("confidence %.2f" % view.confidence in text,
         "the card lost its stated confidence")
    for answer in view.answers[:_shown_answers(view)]:
        need(_answer_present(answer, text),
             "the card lost the provenance answer for %s" % answer.package.text)
    if counts["new_direct"] and not view.answers:
        need(False, "%d new direct dependencies but no provenance section"
             % counts["new_direct"])
    for finding in view.findings[:_shown_findings(view)]:
        need(_finding_present(finding, text),
             "the card lost the finding %s / %s" % (finding.kind, finding.subject))
    for row in _violation_rows(view)[:_shown_licence_rows(view)]:
        need(row.edge in text or "offending edge path" in text,
             "the card lost the edge path for %s" % row.package.text)
    if counts["suppressed"]:
        need("suppressed" in text, "the card hid its suppressed-finding count")
    need(len([line for line in text.splitlines() if line.strip() == "who asked for this"]) == 1,
         "the provenance section header moved")
    return audit_


def _answer_present(answer, text: str) -> bool:
    """The answer must be on the card *as an answer*, not merely as a substring somewhere.

    The episode id appears in the header line anyway, so a check that stopped at `id in text` would
    pass on a card with the whole `who asked for this` block deleted — which is exactly the failure
    this auditor exists to catch.
    """
    for line in text.splitlines():
        if "introduced by" not in line:
            continue
        if answer.answered:
            if answer.episode_id and answer.episode_id in line:
                return True
        elif "not recorded" in line:
            return True
    return False


def freshness_line_present(view, text: str) -> bool:
    if view.freshness.state == "offline":
        return "not consulted" in text or "offline" in text
    if view.freshness.state == "missing":
        return "no cache" in text or "no advisory cache" in text
    return view.freshness.label in text or view.freshness.path in text


def _shown_answers(view) -> int:
    return len(view.answers)


def _shown_findings(view) -> int:
    return len(view.findings)


def _shown_licence_rows(view) -> int:
    return len(_violation_rows(view))


def _violation_rows(view) -> list:
    return [row for row in view.rows if row.violation]


def _finding_present(finding, text: str) -> bool:
    if finding.kind in (ADVISORY_FIX, ADVISORY_UNCHECKED, ADVISORY_MISSING, ADVISORY_STALE):
        return finding.subject in text or "advisories" in text
    if finding.kind in (LICENSE_DENIED, LICENSE_UNKNOWN, LICENSE_DRIFT):
        return finding.package in text or finding.subject in text
    key = finding.subject.split(" · ")[0]
    return key in text


def text_of(view, **kwargs) -> str:
    return "\n".join(render(view, **kwargs))


def _wrap(text: str, cols: int) -> list[str]:
    """Wrap with a hanging indent, keeping long paths whole on their own line where possible."""
    if cols <= 0 or len(text) <= cols:
        return [text]
    indent = text[: len(text) - len(text.lstrip())]
    body = text[len(indent):]
    width = max(20, cols - len(indent))
    words = body.split()
    if not words:
        return [text]
    lines: list[str] = []
    current = ""
    for word in words:
        candidate = word if not current else current + " " + word
        if len(candidate) <= width or not current:
            current = candidate
            continue
        lines.append(indent + current)
        current = word
    if current:
        lines.append(indent + current)
    return lines


def _clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[: max(1, width - 1)] + "…"
