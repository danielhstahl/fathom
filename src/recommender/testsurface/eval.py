"""Scoring the corpus: what was claimed, what was true, and which claim was the forbidden one.

`fathom testsurface score` is the CI line. Four tables, four gates, and each gate is a
number *with a reason attached*:

| table | gate | the failure it is watching |
| --- | --- | --- |
| parse | `--min-parse` 0.95 | a case name missed or invented in one of the five languages |
| pairs | `--min-pairs` 1.0 | a case-level delta that disagrees with the hand-written label — a deletion, a skip, a weakening |
| coverage | `--min-precision` 1.0, `--min-recall` 1.0 | **a false `untested-public`**, which is the finding a reviewer learns to ignore, or a missed one, which is the finding the ticket exists to produce |
| episodes | `--min-episodes` 1.0 | a count, a ratio, a proxy basis or a stated confidence that moved |

Precision and recall are both absolute here, which is unusual and deliberate: this corpus is
*hand-labelled from the fixtures*, not inferred from the implementation, so every miss is a
defect rather than a limitation of the sample. The cost argument that makes noise suppression
tolerate 0.90 recall does not apply: there is no unseen traffic behind these rows.

The episode table also checks the card's *words* (`must_say`), not only its numbers — the
proxy has to say "line count only" in the case where that is the honest answer, and a card
that rounds that away has failed AC 4 even if every count is right.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass, field

from . import corpus as C
from . import diff as D
from . import delta as DE
from . import parsers as P
from . import render
from .policy import (KNOB_INDEX, UNTESTED_PUBLIC, Policy, Rule)

MIN_PARSE = 0.95
MIN_PAIRS = 1.0
MIN_PRECISION = 1.0
MIN_RECALL = 1.0
MIN_EPISODES = 1.0

#: The three ways a coverage row can be wrong, named separately because they are different
#: bugs: a symbol that fired but should not have, one that should have and did not, and one
#: nobody labelled at all (which means the fixture grew).
FALSE_POSITIVE = "false-positive"
MISSED = "missed"
UNLABELLED = "unlabelled"


@dataclass
class Row:
    """One scored case: expectation, answer, and the note that explains a miss."""

    table: str
    name: str
    expected: str
    got: str
    ok: bool
    detail: str = ""
    kind: str = ""

    @property
    def hides_a_false_finding(self) -> bool:
        return self.kind == FALSE_POSITIVE


@dataclass
class Report:
    """Everything `fathom testsurface score` prints."""

    parse_rows: list[Row] = field(default_factory=list)
    pair_rows: list[Row] = field(default_factory=list)
    coverage_rows: list[Row] = field(default_factory=list)
    episode_rows: list[Row] = field(default_factory=list)

    # -- the numbers --------------------------------------------------------
    def parse_accuracy(self) -> float:
        return _rate(row.ok for row in self.parse_rows)

    def pair_accuracy(self) -> float:
        return _rate(row.ok for row in self.pair_rows)

    def coverage_precision(self) -> float:
        fired = sum(1 for row in self.coverage_rows if row.got not in ("", "—"))
        true_fires = sum(1 for row in self.coverage_rows
                         if row.kind in ("", "fired") and row.ok)
        return (true_fires / fired) if fired else 1.0

    def coverage_recall(self) -> float:
        expected = [row for row in self.coverage_rows if row.kind != "covered-only"]
        return _rate(row.ok for row in expected)

    def episode_accuracy(self) -> float:
        return _rate(row.ok for row in self.episode_rows)

    def false_findings(self) -> list[Row]:
        return [row for row in self.coverage_rows if row.hides_a_false_finding]

    def gates(self, *, parse: float, pairs: float, precision: float, recall: float,
              episodes: float) -> list[str]:
        problems: list[str] = []
        if self.parse_accuracy() < parse:
            problems.append("parse accuracy %.3f < %.3f" % (self.parse_accuracy(), parse))
        if self.pair_accuracy() < pairs:
            problems.append("pair accuracy %.3f < %.3f" % (self.pair_accuracy(), pairs))
        if self.coverage_precision() < precision:
            problems.append("coverage precision %.3f < %.3f"
                            % (self.coverage_precision(), precision))
        if self.coverage_recall() < recall:
            problems.append("coverage recall %.3f < %.3f" % (self.coverage_recall(), recall))
        if self.episode_accuracy() < episodes:
            problems.append("episode accuracy %.3f < %.3f"
                            % (self.episode_accuracy(), episodes))
        for row in self.false_findings():
            problems.append("a finding fired that should not have: %s — %s"
                            % (row.name, row.detail))
        return problems

    # -- the text -----------------------------------------------------------
    def as_dict(self) -> dict:
        return {"parse": {"total": len(self.parse_rows),
                          "accuracy": round(self.parse_accuracy(), 4)},
                "pairs": {"total": len(self.pair_rows),
                          "accuracy": round(self.pair_accuracy(), 4)},
                "coverage": {"total": len(self.coverage_rows),
                             "precision": round(self.coverage_precision(), 4),
                             "recall": round(self.coverage_recall(), 4)},
                "episodes": {"total": len(self.episode_rows),
                             "accuracy": round(self.episode_accuracy(), 4)},
                "false_findings": [dataclasses.asdict(row) for row in self.false_findings()],
                "misses": [dataclasses.asdict(row) for row in self._misses()]}

    def text(self, *, verbose: bool = False) -> str:
        lines = ["test surface corpus: %d parses, %d pairs, %d coverage cases, %d episodes"
                 % (len(self.parse_rows), len(self.pair_rows), len(self.coverage_rows),
                    len(self.episode_rows))]
        lines.append("  parse accuracy      %.3f   (%d/%d)"
                     % (self.parse_accuracy(), sum(r.ok for r in self.parse_rows),
                        len(self.parse_rows)))
        lines.append("  pair accuracy       %.3f   (%d/%d)"
                     % (self.pair_accuracy(), sum(r.ok for r in self.pair_rows),
                        len(self.pair_rows)))
        lines.append("  coverage precision  %.3f   (%d false finding(s))"
                     % (self.coverage_precision(), len(self.false_findings())))
        lines.append("  coverage recall     %.3f" % self.coverage_recall())
        lines.append("  episode accuracy    %.3f   (%d/%d)"
                     % (self.episode_accuracy(), sum(r.ok for r in self.episode_rows),
                        len(self.episode_rows)))
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
        return [row for row in (*self.parse_rows, *self.pair_rows, *self.coverage_rows,
                                *self.episode_rows) if not row.ok]


# ---------------------------------------------------------------------------
# the runs
# ---------------------------------------------------------------------------
def score(*, policy: Policy | None = None, verbose: bool = False) -> Report:
    """Run every table. Pure, deterministic, no IO."""
    report = Report()
    for case in C.PARSE_CASES:
        report.parse_rows.append(_score_parse(case))
    for case in C.PAIR_CASES:
        report.pair_rows.append(_score_pair(case))
    for case in C.COVERAGE_CASES:
        report.coverage_rows.extend(_score_coverage(case))
    for case in C.EPISODE_CASES:
        report.episode_rows.append(_score_episode(case))
    return report


def _score_parse(case: C.ParseCase) -> Row:
    parsed = P.parse(case.path, case.text)
    names = {entry.qualified for entry in parsed.cases}
    skipped = {entry.qualified for entry in parsed.cases if entry.skipped}
    focused = {entry.qualified for entry in parsed.cases if entry.focused}
    ok = (names == set(case.cases) and skipped == set(case.skipped)
          and focused == set(case.focused))
    if case.state:
        ok = ok and parsed.state == case.state
    detail = case.note or "; ".join(parsed.notes + parsed.evidence)
    if not ok:
        detail = "expected %s, got %s (skipped %s vs %s, state %s)" % (
            sorted(case.cases), sorted(names), sorted(skipped), sorted(case.skipped),
            parsed.state)
    return Row("parse", case.name, "%d case(s)" % len(case.cases),
               "%d case(s)" % len(names), ok, detail=detail)


def _score_pair(case: C.PairCase) -> Row:
    before = P.parse(case.path, case.before)
    after = P.parse(case.path, case.after)
    pair = D.case_delta(case.path, before, after)
    got = {
        "added": {entry.qualified for entry in pair.added},
        "removed": {entry.qualified for entry in pair.removed},
        "skipped": {entry.qualified for entry, _ in pair.newly_skipped},
        "unskipped": {entry.qualified for entry in pair.unskipped},
        "weakened": {item.case for item in pair.weakened},
        "lost": {item.case for item in pair.assertions_lost},
        "focused": {entry.qualified for entry in pair.focused},
    }
    want = {"added": set(case.added), "removed": set(case.removed),
            "skipped": set(case.skipped), "unskipped": set(case.unskipped),
            "weakened": set(case.weakened), "lost": set(case.lost),
            "focused": set(case.focused)}
    wrong = [key for key in want if got[key] != want[key]]
    detail = case.note
    if wrong:
        detail = "; ".join("%s: expected %s, got %s"
                           % (key, sorted(want[key]), sorted(got[key])) for key in wrong)
    return Row("pairs", case.name, _expected_label(want), _got_label(got), not wrong,
               detail=detail)


def _score_coverage(case: C.CoverageCase) -> list[Row]:
    policy = Policy.built_in().with_knobs(source="corpus knob", **case.knobs)
    episode = DE.Episode(id=case.name,
                         files=tuple(DE.Change.from_pair(path, before, after)
                                     for path, (before, after) in case.files.items()))
    view = DE.measure(episode, policy)
    fired = {finding.name for finding in view.findings if finding.kind == UNTESTED_PUBLIC}
    rows: list[Row] = []
    for symbol in case.untested:
        ok = symbol in fired
        rows.append(Row("coverage", "%s/%s" % (case.name, symbol), "fires",
                        "fires" if ok else "silent", ok, kind="fired",
                        detail=case.note if not ok else ""))
    for symbol in case.covered:
        ok = symbol not in fired
        rows.append(Row("coverage", "%s/%s" % (case.name, symbol), "silent",
                        "silent" if ok else "fires", ok,
                        kind="" if ok else FALSE_POSITIVE,
                        detail=case.note if ok else
                        "the finding fired for `%s` even though the case says it is covered "
                        "(or exempt): %s" % (symbol, case.note)))
    unexpected = sorted(fired - set(case.untested) - set(case.covered))
    for symbol in unexpected:
        rows.append(Row("coverage", "%s/%s" % (case.name, symbol), "unlabelled", "fires",
                        False, kind=FALSE_POSITIVE,
                        detail="the fixture grew a public symbol nobody labelled"))
    if not rows:
        rows.append(Row("coverage", case.name, "nothing", "nothing", True))
    return rows


def _score_episode(case: C.EpisodeCase) -> Row:
    episode = DE.Episode(id=case.name,
                         files=tuple(DE.Change.from_pair(path, before, after)
                                     for path, (before, after) in case.files.items()))
    view = DE.measure(episode)
    counts = view.counts()
    ratio = view.ratio()
    problems: list[str] = []
    for key, want in case.counts.items():
        if counts.get(key) != want:
            problems.append("%s: expected %r, got %r" % (key, want, counts.get(key)))
    if case.test_lines is not None and counts["test_lines_added"] != case.test_lines:
        problems.append("test_lines_added: expected %d, got %d"
                        % (case.test_lines, counts["test_lines_added"]))
    if case.prod_lines is not None and counts["prod_lines_added"] != case.prod_lines:
        problems.append("prod_lines_added: expected %d, got %d"
                        % (case.prod_lines, counts["prod_lines_added"]))
    if case.ratio is not None and (ratio["ratio"] is None
                                   or abs(ratio["ratio"] - case.ratio) > 0.002):
        problems.append("ratio: expected %s, got %s" % (case.ratio, ratio["ratio"]))
    if case.basis and view.proxy.basis != case.basis:
        problems.append("proxy basis: expected %s, got %s" % (case.basis, view.proxy.basis))
    if case.confidence is not None and abs(view.proxy.confidence - case.confidence) > 0.001:
        problems.append("stated confidence: expected %.2f, got %.2f"
                        % (case.confidence, view.proxy.confidence))
    text = "\n".join(render.render(view))
    missing = [needle for needle in case.must_say if needle not in text]
    if missing:
        problems.append("the card never said: %s" % ", ".join(missing))
    return Row("episodes", case.name,
               "basis=%s conf=%s %s" % (case.basis or "—", case.confidence,
                                        case.counts or ""),
               "basis=%s conf=%.2f %s" % (view.proxy.basis, view.proxy.confidence,
                                          {key: counts[key] for key in case.counts}),
               not problems, detail="; ".join(problems) or case.note)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _expected_label(want: dict) -> str:
    return ", ".join("%s=%s" % (key, ",".join(sorted(values)) or "—")
                     for key, values in sorted(want.items()) if values) or "nothing"


def _got_label(got: dict) -> str:
    return ", ".join("%s=%s" % (key, ",".join(sorted(values)) or "—")
                     for key, values in sorted(got.items()) if values) or "nothing"


def _rate(values) -> float:
    values = list(values)
    if not values:
        return 1.0
    return sum(1 for value in values if value) / len(values)


def rule_ids() -> tuple[str, ...]:
    """Every finding id the tables score against (used by the contract test)."""
    from .policy import RULES
    return tuple(rule.id for rule in RULES)


def knob_names() -> tuple[str, ...]:
    return tuple(KNOB_INDEX)
