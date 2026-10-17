"""The per-episode card, and an auditor that refuses a card which hides its own evidence.

`e2.10` composes the final summary card; what lives here is the *test surface* half of it,
rendered to text so the wording has one owner. Three claims are checked by :func:`audit`,
because each is the sort of line a refactor quietly drops:

1. the ratio row is present and carries **both** absolutes, not just the quotient — a
   reviewer needs to see that `0.02` came from `+4 test / +180 prod`, not from `+4 / +200`
   and not from a denominator nobody defined;
2. every finding row names its test or symbol **and** a `path:line`, so the row is clickable
   rather than informative;
3. the coverage proxy row is present, with its stated confidence, always — including the
   episodes where the answer is "line count only", which is exactly when it matters most.

An episode whose findings exceed `test.card.max_findings` prints the remainder as a count
and the verb that lists them. That is a cap on rows, not a deletion: `audit` fails a card
that dropped rows without saying so.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .delta import SurfaceDelta

DEFAULT_COLS = 100

RULE = "─" * 8


@dataclass
class Audit:
    """The result of auditing one render."""

    problems: list[str] = field(default_factory=list)
    checked: int = 0

    @property
    def ok(self) -> bool:
        return not self.problems

    def as_dict(self) -> dict:
        return {"ok": self.ok, "checked": self.checked, "problems": list(self.problems)}


def render(view: SurfaceDelta, *, cols: int = DEFAULT_COLS,
           max_findings: int | None = None, header: bool = True) -> list[str]:
    """The card. Deterministic: the same episode in, the same bytes out."""
    lines: list[str] = []
    if header:
        lines.append("%s test surface · %s" % (RULE, view.episode_id or "episode"))
        if view.title:
            lines.extend(_wrap("  %s" % _clip(view.headline(), cols * 2), cols))
    lines.extend(_wrap("  " + view.summary_line(), cols))
    ratio = view.ratio()
    lines.extend(_wrap("  ratio: %s · test %s / prod %s lines added (the ratio denominator "
                       "is test + production only)"
                       % (_ratio_text(ratio), ratio["test_lines_added"],
                          ratio["prod_lines_added"]), cols))
    counts = view.counts()
    lines.extend(_wrap("  files: %d changed (%d created, %d deleted, %d renamed, %d modified)"
                       " · test files: %d created, %d deleted, %d renamed, %d modified"
                       % (counts["files"], counts["files_created"], counts["files_deleted"],
                          counts["files_renamed"], counts["files_modified"],
                          counts["test_files_created"], counts["test_files_deleted"],
                          counts["test_files_renamed"], counts["test_files_modified"]), cols))
    lines.extend(_wrap("  cases: +%d added, −%d removed, %s skipped, %s un-skipped, %d focused · "
                       "assertions: %d weakened, %d lost, %d added"
                       % (counts["tests_added"], counts["tests_removed"], counts["tests_skipped"],
                          counts["tests_unskipped"], counts["tests_focused"],
                          counts["assertions_weakened"], counts["assertions_lost"],
                          counts["assertions_added"]), cols))
    lines.extend(_file_rows(view, cols))
    lines.extend(_finding_rows(view, cols, max_findings))
    for kind, count in sorted(view.suppressed.items()):
        lines.extend(_wrap("  %d %s finding(s) not reported: %s is off (%s)"
                           % (count, kind, kind,
                              view.suppressed_source.get(kind, "policy")), cols))
    lines.extend(_wrap("  confidence: %s" % view.proxy.statement, cols))
    for evidence in view.proxy.evidence:
        lines.extend(_wrap("    · %s" % evidence, cols))
    for note in view.notes:
        lines.extend(_wrap("  note: %s" % note, cols))
    return lines


def _file_rows(view: SurfaceDelta, cols: int) -> list[str]:
    lines: list[str] = []
    for item in view.files:
        marker = {"added": "+", "deleted": "−", "renamed": "→"}.get(item.change_type, "~")
        detail = "%s %s (%s%s)" % (marker, item.path, item.cls,
                                   ", %s" % item.language if item.language else "")
        lines.extend(_wrap(_clip(detail, cols), cols))
        lines.extend(_wrap("    +%d/−%d lines · %d test / %d prod / %d other added"
                           % (item.lines.added, item.lines.deleted, item.test_added,
                              item.prod_added, item.other_added), cols))
        if item.case_diff and item.case_diff.language:
            case_counts = item.case_diff.counts()
            lines.extend(_wrap("    %s: +%d cases, −%d, %d skipped, %d weakened"
                               % (item.case_diff.framework or item.case_diff.language,
                                  case_counts["added"], case_counts["removed"],
                                  case_counts["skipped"], case_counts["weakened"]), cols))
        for symbol in item.new_symbols:
            lines.extend(_wrap("    new public %s %s at %s:%d (%d body, %d logic)"
                               % (symbol.kind, symbol.name, item.path, symbol.line,
                                  symbol.body_lines, symbol.logic_lines), cols))
        for note in item.notes:
            lines.extend(_wrap("    note: %s" % note, cols))
    return lines


def _finding_rows(view: SurfaceDelta, cols: int, max_findings: int | None) -> list[str]:
    cap = max_findings if max_findings is not None else int(view.policy.number("max_findings"))
    if not view.findings:
        return ["  findings: none"]
    lines = ["  findings (%d)" % len(view.findings)]
    for finding in view.findings[:cap]:
        lines.extend(_wrap("    %s" % finding.render(), cols))
        for evidence in finding.evidence:
            if evidence and evidence != finding.detail:
                lines.extend(_wrap("      · %s" % evidence, cols))
    hidden = len(view.findings) - cap
    if hidden > 0:
        lines.append("    … %d more: fathom testsurface card --all %s"
                     % (hidden, view.episode_id or "<episode>"))
    return lines


def _ratio_text(ratio: dict) -> str:
    if ratio["ratio"] is None:
        return "no test or production lines added"
    return "%.3f (%s)" % (ratio["ratio"], ratio["test_to_prod"])


def _clip(text: str, cols: int) -> str:
    if cols <= 0 or len(text) <= cols:
        return text
    return text[: max(1, cols - 1)] + "…"


def _wrap(text: str, cols: int, indent: int | None = None) -> list[str]:
    """Wrap instead of clipping.

    A clipped finding row loses the location at its end, which is the one part a reviewer has
    to click, so the invariant rows wrap with a hanging indent instead.
    """
    if cols <= 0 or len(text) <= cols:
        return [text]
    pad = " " * (indent if indent is not None else len(text) - len(text.lstrip(" ")))
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


def audit(lines: list[str], view: SurfaceDelta | None = None,
          max_findings: int | None = None) -> Audit:
    """Fail a render that lost its ratio, its locations, or its confidence statement."""
    result = Audit()
    result.checked = len(lines)
    if view is None:
        result.problems.append("audit refused: no delta to audit against")
        return result
    ratio = view.ratio()
    # Wrapped rows are rejoined before searching: the invariant is about the card's content,
    # and a 40-column pane must not fail an audit that a 120-column pane passes.
    flat = " ".join(" ".join(lines).split())
    if ratio["ratio"] is not None:
        if "%.3f" % ratio["ratio"] not in flat:
            result.problems.append("the ratio number is missing from the render")
        absolutes = "test %d / prod %d lines added" % (ratio["test_lines_added"],
                                                       ratio["prod_lines_added"])
        if absolutes not in flat:
            result.problems.append("the absolutes behind the ratio (%s) are missing: a ratio "
                                   "alone is not evidence" % absolutes)
    if "confidence:" not in flat:
        result.problems.append("the coverage proxy row is missing: every card states a "
                               "confidence")
    if "%.2f" % view.proxy.confidence not in flat:
        result.problems.append("the stated confidence number is missing")
    cap = max_findings if max_findings is not None else int(view.policy.number("max_findings"))
    shown = view.findings[:cap]
    for finding in shown:
        if finding.name not in flat:
            result.problems.append("finding %s (%s) lost its name"
                                   % (finding.kind, finding.name))
        if finding.location not in flat:
            result.problems.append("finding %s (%s) lost its location"
                                   % (finding.kind, finding.name))
    hidden = len(view.findings) - len(shown)
    if hidden > 0 and "%d more" % hidden not in flat:
        result.problems.append("%d finding(s) were dropped without saying so" % hidden)
    for kind, count in view.suppressed.items():
        if "%s is off" % kind not in flat:
            result.problems.append("suppressed %s findings were not explained" % kind)
    return result
