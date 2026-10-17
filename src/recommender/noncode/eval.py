"""Scoring the corpus: what fired, what was supposed to, and which mismatch is forbidden.

`fathom noncode score` is the CI line. Four gates, each with a reason attached:

| table | gate | the failure it is watching |
| --- | --- | --- |
| rules | `--min-rules` 1.0 | a fixture that no longer produces the finding it was written for — every one of the 26 kinds has at least one, so a dead detector is a red run rather than a quiet gap |
| coverage | `--min-surfaces` 6 | a surface losing its fixture, which would let AC 1 pass on five surfaces while nobody noticed the sixth |
| traps | `--min-precision` 1.0 | **a finding that fired and should not have** — the trap corpus is the only thing standing between this package and the wall of noise that makes a reviewer stop reading it |
| episodes | `--min-episodes` 1.0 | a count, a checklist verdict, a coordination warning or a required-card phrase that moved |

Every bar is absolute, which is affordable here for a specific reason: the labels are written
against the *fixtures*, by hand, not inferred from the implementation and not sampled from
traffic. A miss is a defect, not a limitation of the sample. The only row with slack in the
CI invocation is none — there is no 0.95 line in this suite, because a non-code tool that is
sometimes right about a migration is not a tool, it is a rumour with a UI.

A trap case that fires *something else* is also reported, but only the kinds it names are
scored as false: a fixture designed to test "no `ci-secret-new`" is allowed to fire
`env-added` for an unrelated reason, and the row prints what it fired so an unrelated
regression still shows up in the log rather than being silently allowed.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass, field

from . import corpus as C
from .episode import Episode, measure
from .policy import FINDING_IDS, Policy
from .surfaces import Change

MIN_RULES = 1.0
MIN_PRECISION = 1.0
MIN_EPISODES = 1.0
MIN_SURFACES = 6


@dataclass
class Row:
    """One scored case: expectation, answer, and the sentence that explains a miss."""

    table: str
    name: str
    expected: str
    got: str
    ok: bool
    kind: str = ""
    detail: str = ""


@dataclass
class Report:
    """Everything `fathom noncode score` prints."""

    rule_rows: list[Row] = field(default_factory=list)
    trap_rows: list[Row] = field(default_factory=list)
    episode_rows: list[Row] = field(default_factory=list)
    surface_rows: list[Row] = field(default_factory=list)
    uncovered: list[str] = field(default_factory=list)

    # -- the numbers --------------------------------------------------------
    def rule_accuracy(self) -> float:
        return _rate(row.ok for row in self.rule_rows)

    def precision(self) -> float:
        return _rate(row.ok for row in self.trap_rows)

    def episode_accuracy(self) -> float:
        return _rate(row.ok for row in self.episode_rows)

    def surfaces_covered(self) -> int:
        return sum(1 for row in self.surface_rows if row.ok)

    def false_findings(self) -> list[Row]:
        return [row for row in self.trap_rows if not row.ok]

    def gates(self, *, rules: float, precision: float, episodes: float,
              surfaces: int) -> list[str]:
        problems: list[str] = []
        if self.rule_accuracy() < rules:
            problems.append("rule accuracy %.3f < %.3f" % (self.rule_accuracy(), rules))
        if self.precision() < precision:
            problems.append("precision %.3f < %.3f (%d trap(s) fired)"
                            % (self.precision(), precision, len(self.false_findings())))
        if self.episode_accuracy() < episodes:
            problems.append("episode accuracy %.3f < %.3f"
                            % (self.episode_accuracy(), episodes))
        if self.surfaces_covered() < surfaces:
            problems.append("only %d of %d surfaces have a fixture that fires"
                            % (self.surfaces_covered(), surfaces))
        if self.uncovered:
            problems.append("finding kinds with no fixture: %s" % ", ".join(self.uncovered))
        for row in self.false_findings():
            problems.append("a finding fired that should not have: %s — %s"
                            % (row.name, row.detail))
        return problems

    # -- the text -----------------------------------------------------------
    def as_dict(self) -> dict:
        return {"rules": {"total": len(self.rule_rows),
                          "accuracy": round(self.rule_accuracy(), 4)},
                "traps": {"total": len(self.trap_rows),
                          "precision": round(self.precision(), 4)},
                "episodes": {"total": len(self.episode_rows),
                             "accuracy": round(self.episode_accuracy(), 4)},
                "surfaces": {"covered": self.surfaces_covered(), "total": len(
                    self.surface_rows)},
                "uncovered_kinds": list(self.uncovered),
                "false_findings": [dataclasses.asdict(row) for row in self.false_findings()],
                "misses": [dataclasses.asdict(row) for row in self._misses()]}

    def text(self, *, verbose: bool = False) -> str:
        lines = ["non-code corpus: %d rule fixtures, %d traps, %d episodes, %d surfaces"
                 % (len(self.rule_rows), len(self.trap_rows), len(self.episode_rows),
                    len(self.surface_rows))]
        lines.append("  rule accuracy       %.3f   (%d/%d)"
                     % (self.rule_accuracy(), sum(r.ok for r in self.rule_rows),
                        len(self.rule_rows)))
        lines.append("  precision           %.3f   (%d trap(s) fired)"
                     % (self.precision(), len(self.false_findings())))
        lines.append("  episode accuracy    %.3f   (%d/%d)"
                     % (self.episode_accuracy(), sum(r.ok for r in self.episode_rows),
                        len(self.episode_rows)))
        lines.append("  surfaces covered    %d/%d   (%s)"
                     % (self.surfaces_covered(), len(self.surface_rows),
                        ", ".join("%s %s" % (row.name, "ok" if row.ok else "MISSING")
                                  for row in self.surface_rows)))
        if self.uncovered:
            lines.append("  kinds with no fixture: %s" % ", ".join(self.uncovered))
        misses = self._misses()
        if misses:
            lines.append("  misses:")
            for row in misses:
                lines.append("    [%s] %s — expected %s, got %s"
                             % (row.table, row.name, row.expected, row.got))
                if row.detail:
                    lines.append("      %s" % row.detail.replace("\n", "\n      "))
        elif verbose:
            lines.append("  no misses")
        return "\n".join(lines)

    def _misses(self) -> list[Row]:
        return [row for row in (*self.rule_rows, *self.trap_rows, *self.episode_rows)
                if not row.ok]


# ---------------------------------------------------------------------------
# the runs
# ---------------------------------------------------------------------------
def score(*, policy: Policy | None = None, verbose: bool = False) -> Report:
    """Run every table. Pure, deterministic, no IO."""
    base = policy or Policy.built_in()
    report = Report()
    fired: set[str] = set()
    for case in C.RULE_CASES:
        row, kind = _score_rule(case, base)
        fired.add(case.rule)
        report.rule_rows.append(row)
    for case in C.TRAP_CASES:
        report.trap_rows.append(_score_trap(case, base))
    for case in C.EPISODE_CASES:
        report.episode_rows.append(_score_episode(case, base))
    for surface in ("env", "flag", "migration", "ci", "iac", "container"):
        hits = [case for case in C.RULE_CASES
                if C.SURFACE_OF_RULE.get(case.rule) == surface and
                any(row.kind == case.rule and row.ok for row in report.rule_rows)]
        report.surface_rows.append(Row(
            "surfaces", surface, "≥1 fixture that fires", "%d fixture(s)" % len(hits),
            bool(hits), detail="AC 1 asks for a fixture per surface"))
    report.uncovered = [kind for kind in FINDING_IDS if kind not in fired]
    return report


def _episode_for(files: dict, *, repo_env_refs=(), repo_flag_reads=()) -> Episode:
    return Episode(id="case",
                   files=tuple(Change.from_pair(path, before, after)
                               for path, (before, after) in sorted(files.items())),
                   repo_env_refs=tuple(repo_env_refs), repo_flag_reads=tuple(repo_flag_reads))


def _measure(case, base: Policy):
    policy = base.with_knobs(source="corpus knob", **getattr(case, "knobs", {})) \
        if getattr(case, "knobs", None) else base
    return measure(_episode_for(case.files,
                                repo_env_refs=getattr(case, "repo_env_refs", ()),
                                repo_flag_reads=getattr(case, "repo_flag_reads", ())),
                   policy)


def _score_rule(case: C.RuleCase, base: Policy) -> tuple[Row, str]:
    view = _measure(case, base)
    matching = [finding for finding in view.findings if finding.kind == case.rule]
    if case.expect_name:
        matching = [finding for finding in matching if case.expect_name in finding.name]
    text = "\n".join(finding.render() + "\n" + "\n".join(finding.evidence)
                      for finding in view.findings)
    text += "\n" + "\n".join(view.checklist.lines())
    missing_says = [needle for needle in case.expect_says if needle not in text]
    problems: list[str] = []
    if not matching:
        problems.append("the finding never fired")
    if missing_says:
        problems.append("it never said: %s" % ", ".join(missing_says))
    detail = case.note or "; ".join(finding.detail for finding in matching[:2])
    if problems:
        detail = "%s — fired kinds: %s" % ("; ".join(problems),
                                           ", ".join(sorted({item.kind for item
                                                              in view.findings})) or "none")
    return Row("rules", "%s/%s" % (case.name, case.rule),
               "%s named %r" % (case.rule, case.expect_name or "anything"),
               "fired" if matching else "silent", not problems, kind=case.rule,
               detail=detail), case.rule


def _score_trap(case: C.TrapCase, base: Policy) -> Row:
    view = _measure(case, base)
    policy = base.with_knobs(source="corpus knob", **case.knobs) if case.knobs else base
    fired = sorted({finding.kind for finding in view.findings
                    if policy.enabled_for(finding.kind)})
    offenders = [kind for kind in case.must_not_fire if kind in fired]
    detail = case.note or ""
    if offenders:
        named = [finding.render() for finding in view.findings
                 if finding.kind in offenders][:3]
        detail = "fired %s: %s" % (", ".join(offenders), " | ".join(named))
    return Row("traps", case.name, "silent on %s" % ", ".join(case.must_not_fire),
               ", ".join(fired) or "silent", not offenders, kind=",".join(offenders),
               detail=detail)


def _score_episode(case: C.EpisodeCase, base: Policy) -> Row:
    view = _measure(case, base)
    counts = view.counts()
    text = "\n".join(_card_lines(view))
    problems: list[str] = []
    for key, want in case.counts.items():
        if counts.get(key) != want:
            problems.append("%s: expected %r, got %r" % (key, want, counts.get(key)))
    if case.requires_operator_action is not None and \
            view.requires_operator_action != case.requires_operator_action:
        problems.append("requires_operator_action: expected %r, got %r"
                        % (case.requires_operator_action, view.requires_operator_action))
    if case.coordination is not None and view.checklist.coordination != case.coordination:
        problems.append("deploy coordination: expected %r, got %r"
                        % (case.coordination, view.checklist.coordination))
    missing = [needle for needle in case.must_say if needle not in text]
    if missing:
        problems.append("the card never said: %s" % ", ".join(missing))
    present = [needle for needle in case.must_not_say if needle in text]
    if present:
        problems.append("the card said something it should not have: %s" % ", ".join(present))
    return Row("episodes", case.name,
               "%s %s" % (case.counts or "", case.must_say or ""),
               "findings=%d actions=%d coordination=%s"
               % (counts["findings"], counts["actions"], view.checklist.coordination),
               not problems, detail="; ".join(problems) or case.note)


def _card_lines(view) -> list[str]:
    from . import render
    return render.render(view, show_all=True) + view.checklist.lines()


def _rate(values) -> float:
    values = list(values)
    return sum(1 for value in values if value) / len(values) if values else 1.0


def rule_coverage() -> dict[str, str]:
    """Which fixture covers which finding kind (used by the contract test)."""
    out: dict[str, str] = {}
    for case in C.RULE_CASES:
        out.setdefault(case.rule, case.name)
    return out
