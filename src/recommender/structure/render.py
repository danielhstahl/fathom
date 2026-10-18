"""The structure card: the directory table, the stories, the churn score, and an auditor.

`e2.10` composes the episode card; what lives here is the *where did this land* half of it,
rendered to text so the wording has one owner. The order is the order a reviewer reads:

1. the one-sentence answer, wrapped with a hanging indent (never clipped — the clipped token
   would have been a directory name, which is the thing they were reading for);
2. the tree basis and the confidence, with **why** the confidence is what it is, because
   "the census is about your change set, not the repository" has to be said out loud;
3. the directory table, active rows only, sorted by how much happened there, with the
   reconciliation line the ticket's second acceptance criterion demands: the table says what
   it sums to and says whether it matches the episode totals;
4. the structural stories, each with the pairings and census numbers that triggered it;
5. the churn score with all five components printed, so a reviewer who disagrees with the
   number can point at the component they think is wrong;
6. the before → after metrics;
7. the findings, then the suppressed switches, then the notes and partials.

The auditor (:func:`audit`) re-reads the produced lines and fails the render when it lost
something the model said. Four things are checked because each is a real way a later refactor
quietly drops a claim while every unit test still passes: the reconciliation line, the churn
number, a stated confidence and basis, and a name for every story and finding that survived
into the render. A capped table or finding list must name its remainder. The CLI runs the
auditor on its own output and exits non-zero from it, and the contract suite hands it three
tampered renders so the invariant has a test that can fail.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .policy import RULE_INDEX

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


def render(view, *, cols: int = DEFAULT_COLS, max_dirs: int | None = None,
           max_findings: int | None = None, show_all: bool = False,
           header: bool = True) -> list[str]:
    """The card. Deterministic: the same episode in gives the same bytes out."""
    lines: list[str] = []
    if header:
        lines.append("%s folder structure · %s" % (RULE, view.episode_id or "episode"))
        if view.title:
            lines.extend(_wrap("  %s" % view.title, cols))
    summary = view.summary()
    lines.extend(_wrap("  " + summary["sentence"], cols))
    lines.extend(_wrap("  %s" % view.headline(), cols))
    lines.append("  tree basis: %s · confidence %.2f" % (view.basis, view.confidence))
    for reason in view.confidence_reasons():
        lines.extend(_wrap("    · %s" % reason, cols))
    lines.extend(_table(view, cols=cols, max_dirs=max_dirs, show_all=show_all))
    lines.extend(_stories(view, cols=cols, show_all=show_all))
    for line in view.churn.lines():
        lines.extend(_wrap("  %s" % line, cols))
    lines.extend(_metrics(view, cols=cols))
    lines.extend(_findings(view, cols=cols, max_findings=max_findings, show_all=show_all))
    for kind, count in sorted(view.suppressed.items()):
        rule = RULE_INDEX.get(kind)
        key = rule.config_key if rule else kind
        lines.extend(_wrap("  %d %s finding(s) not reported: %s is off (%s)"
                           % (count, kind, key,
                              view.suppressed_source.get(kind, "policy")), cols))
    for note in view.notes:
        lines.extend(_wrap("  note: %s" % note, cols))
    for partial in view.partials:
        lines.extend(_wrap("  partial: %s" % partial, cols))
    return lines


def active_rows(view, limit: int | None = None) -> list:
    """The rows worth a line: the ones where something happened, busiest first."""
    rows = [row for row in view.rows if row.active]
    rows.sort(key=lambda row: (-(row.files_created + row.files_deleted + row.files_moved_in
                                 + row.files_moved_out + row.lines_moved_in
                                 + row.lines_added + row.lines_removed), row.path))
    if limit is None:
        return rows
    return rows[:limit]


def _table(view, *, cols: int, max_dirs: int | None, show_all: bool) -> list[str]:
    active = active_rows(view)
    cap = int(view.policy.number("max_dirs")) if max_dirs is None else max_dirs
    limit = len(active) if show_all else min(len(active), cap)
    shown = active[:limit]
    problems = view.verify()
    totals = view.totals()
    sums = view.row_sums()
    lines = ["  directories (%d shown of %d row(s))" % (len(shown), len(view.rows))]
    header = "    %-34s %6s %9s %9s %8s %6s" % ("directory", "files", "added", "removed",
                                                "moved in", "moved out")
    lines.append(header)
    for row in shown:
        lines.append("    %-34s %3d→%-3d %6d %9d %8d %6d%s"
                     % (_clip(row.display, 34), row.files_before, row.files_after,
                        row.lines_added, row.lines_removed, row.lines_moved_in,
                        row.lines_moved_out, _flags(row)))
    hidden = len(active) - len(shown)
    if hidden > 0:
        lines.append("    … %d more director%s: fathom structure dirs %s --all"
                     % (hidden, "y" if hidden == 1 else "ies",
                        view.episode_id or "<episode>"))
    if problems:
        lines.append("  table does NOT reconcile with the episode totals:")
        for problem in problems:
            lines.extend(_wrap("    · %s" % problem, cols))
    else:
        lines.append("  table reconciles to the episode totals: %d file(s) moved / %d created "
                     "/ %d deleted, %d line(s) added / %d removed / %d moved, %d directory(s) "
                     "added / %d removed"
                     % (totals["files_moved"], totals["files_created"],
                        totals["files_deleted"], sums["lines_added"], sums["lines_removed"],
                        sums["lines_moved_in"], sums["directories_added"],
                        sums["directories_removed"]))
    return lines


def _flags(row) -> str:
    flags = []
    if row.directory_added:
        flags.append("new")
    if row.directory_removed:
        flags.append("gone")
    if row.files_created:
        flags.append("+%d created" % row.files_created)
    if row.files_deleted:
        flags.append("−%d deleted" % row.files_deleted)
    return ("  [" + ", ".join(flags) + "]") if flags else ""


def _stories(view, *, cols: int, show_all: bool) -> list[str]:
    if not view.stories:
        return ["  structural stories: none — nothing changed directory"]
    cap = len(view.stories) if show_all else int(view.policy.number("max_stories"))
    lines = ["  structural stories (%d)" % len(view.stories)]
    for story in view.stories[:cap]:
        lines.extend(_wrap("    %s · %s [%s, %.2f]"
                           % (story.shape, story.summary_clause(), story.tier,
                              story.confidence), cols))
        for evidence in story.evidence[:int(view.policy.number("max_evidence"))]:
            lines.extend(_wrap("      · %s" % evidence, cols))
    hidden = len(view.stories) - cap
    if hidden > 0:
        lines.append("    … %d more stor%s: fathom structure stories %s --all"
                     % (hidden, "y" if hidden == 1 else "ies", view.episode_id or "<episode>"))
    return lines


def _metrics(view, *, cols: int) -> list[str]:
    before, after, delta = view.metrics_before, view.metrics_after, view.metrics_delta
    lines = ["  metrics (before → after)"]
    lines.extend(_wrap("    dirs %d → %d (%+d) · files %d → %d (%+d) · files/dir %.2f → %.2f"
                       % (before.dirs, after.dirs, delta["dirs"], before.files, after.files,
                          delta["files"], before.files_per_dir, after.files_per_dir), cols))
    lines.extend(_wrap("    depth max %d → %d (%+d), mean %.2f → %.2f (%+.2f) · fanout max %d "
                       "→ %d, mean %.2f → %.2f"
                       % (before.max_depth, after.max_depth, delta["max_depth"],
                          before.mean_depth, after.mean_depth, delta["mean_depth"],
                          before.max_fanout, after.max_fanout, before.mean_fanout,
                          after.mean_fanout), cols))
    lines.extend(_wrap("    top-%d %s share %.3f → %.3f (%+.3f)"
                       % (after.top_k, after.top_unit, before.top_share, after.top_share,
                          delta["top_share"]), cols))
    where = ", ".join("%s (%s)" % (row["path"], row["units"]) for row in after.top_dirs)
    if where:
        lines.extend(_wrap("    top-%d directories after: %s (counted in %s)"
                           % (after.top_k, where, after.top_unit), cols))
    return lines


def _findings(view, *, cols: int, max_findings: int | None, show_all: bool) -> list[str]:
    if not view.findings:
        return ["  findings: none"]
    cap = len(view.findings) if show_all else (
        max_findings if max_findings is not None
        else int(view.policy.number("max_stories")) + int(view.policy.number("max_dirs")))
    lines = ["  findings (%d)" % len(view.findings)]
    shown = view.findings[:cap]
    for finding in shown:
        lines.extend(_wrap_lines(finding.block(indent=4, max_evidence=int(
            view.policy.number("max_evidence"))), cols))
    hidden = len(view.findings) - cap
    if hidden > 0:
        lines.append("    … %d more finding(s): fathom structure findings %s --all"
                     % (hidden, view.episode_id or "<episode>"))
    return lines


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


def _clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[: max(1, width - 1)] + "…"


def audit(lines: list[str], view=None, *, max_dirs: int | None = None,
          show_all: bool = False) -> Render:
    """Fail a card that lost its reconciliation, its score, its confidence or its names."""
    problems: list[str] = []
    if view is None:
        return Render(lines=list(lines),
                      problems=["audit refused: no delta to audit against"])
    flat = " ".join(" ".join(lines).split())
    totals = view.totals()
    if view.verify():
        if "does NOT reconcile" not in flat:
            problems.append("the table does not reconcile and the card did not say so")
    elif "table reconciles to the episode totals" not in flat:
        problems.append("a reconciling table did not say it reconciles")
    if "churn %.3f" % view.churn.score not in flat:
        problems.append("the churn score is missing from the render")
    if "(%s)" % view.churn.band not in flat:
        problems.append("the churn band is missing from the render")
    if "confidence %.2f" % view.confidence not in flat:
        problems.append("the card states no confidence")
    if view.basis not in flat:
        problems.append("the card does not say what its directory census is about")
    sentence = view.summary()["sentence"]
    if sentence.split(" · ")[-1].strip(".") not in flat:
        problems.append("the one-sentence summary lost its churn clause")
    active = active_rows(view)
    limit = len(active) if show_all else min(len(active),
                                             int(view.policy.number("max_dirs")))
    for row in active[:limit]:
        if row.display not in flat:
            problems.append("directory %s lost its row" % row.display)
    if len(active) > limit and "more director" not in flat:
        problems.append("the directory table was capped without naming the remainder")
    stories = view.stories
    cap = len(stories) if show_all else int(view.policy.number("max_stories"))
    for story in stories[:cap]:
        if story.source and ("/" + story.source) not in flat and story.source not in flat:
            problems.append("story on %s lost its source directory" % story.source)
        for destination in story.destinations:
            if destination and destination not in flat:
                problems.append("story on %s lost a destination (%s)"
                                % (story.source, destination))
    if len(stories) > cap and "more stor" not in flat:
        problems.append("the story list was capped without naming the remainder")
    findings_cap = len(view.findings) if show_all else (
        int(view.policy.number("max_stories")) + int(view.policy.number("max_dirs")))
    for finding in view.findings[:findings_cap]:
        if finding.name not in flat:
            problems.append("finding %s lost its name (%s)" % (finding.kind, finding.name))
    for kind, count in view.suppressed.items():
        rule = RULE_INDEX.get(kind)
        key = rule.config_key if rule else kind
        if "%s is off" % key not in flat:
            problems.append("suppressed %s findings were not explained" % kind)
    return Render(lines=list(lines), problems=problems)
