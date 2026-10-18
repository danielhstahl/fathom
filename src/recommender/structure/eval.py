"""Scoring the corpus: did the shape appear, did the sentence match, did the table reconcile.

`fathom structure score` is the CI line. Seven gates, each watching a specific failure:

| table | gate | the failure it is watching |
| --- | --- | --- |
| shapes | `--min-shapes` 1.0 | a fixture that no longer produces the shape it was written for — including its moved-file and moved-line counts, because a shape that fires on the wrong numbers is still the wrong shape |
| summaries | `--min-summaries` 1.0 | **AC 1 and AC 4 together**: the one-sentence answer, byte for byte, on every named shape. A wording change is a diff here, not a surprise in a review |
| traps | `--min-precision` 1.0 | a structural finding that fired and should not have — a rename inside one directory called a move, two files called a reorganisation, a deletion beside an unrelated creation called a move |
| episodes | `--min-episodes` 1.0 | counts that moved, or a card that stopped saying something it owes the reviewer |
| reconciliation | `--min-reconcile` 1.0 | **AC 2**: the directory table must sum to the episode totals on every fixture *and* on 400 generated episodes |
| separation | `--min-separation` 0.3 | **AC 3**: the 20-file local change and the 20-file cross-tree reshuffle must land in different bands, with a stated gap |
| shape coverage | `--min-shapes-covered` 4 | one of the four shapes the ticket names losing its fixture |

Every bar is absolute except none — the same argument `e2.8` and `e2.9` make: the labels are
hand-written against fixtures whose numbers the author typed, so a miss is a defect and not
sampling noise. The one number with a tolerance in it is `--min-separation`, and that is a
tolerance on *how far apart* two measured scores must stay, which is a floor on the tool's
distinguishing power rather than on its accuracy.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass, field

from . import corpus as C
from . import render
from .delta import measure
from .policy import (NAMED_SHAPES, SHAPE_FILES_MOVED, SHAPE_FLATTEN, SHAPE_MODULE_SPLIT,
                     SHAPE_PACKAGE_RENAME, SHAPE_SUBTREE_MOVE, Policy)

MIN_SHAPES = 1.0
MIN_SUMMARIES = 1.0
MIN_PRECISION = 1.0
MIN_EPISODES = 1.0
MIN_RECONCILE = 1.0
MIN_SEPARATION = 0.3
MIN_SHAPES_COVERED = 4
FUZZ_SEEDS = 400


@dataclass
class Row:
    """One scored case: what was expected, what happened, and the sentence that explains a miss."""

    table: str
    name: str
    expected: str
    got: str
    ok: bool
    detail: str = ""


@dataclass
class Report:
    """Everything `fathom structure score` prints."""

    shape_rows: list[Row] = field(default_factory=list)
    summary_rows: list[Row] = field(default_factory=list)
    trap_rows: list[Row] = field(default_factory=list)
    episode_rows: list[Row] = field(default_factory=list)
    reconcile_rows: list[Row] = field(default_factory=list)
    coverage_rows: list[Row] = field(default_factory=list)
    separation: dict = field(default_factory=dict)

    # -- the numbers --------------------------------------------------------
    def shape_accuracy(self) -> float:
        return _rate(row.ok for row in self.shape_rows)

    def summary_accuracy(self) -> float:
        return _rate(row.ok for row in self.summary_rows)

    def precision(self) -> float:
        return _rate(row.ok for row in self.trap_rows)

    def episode_accuracy(self) -> float:
        return _rate(row.ok for row in self.episode_rows)

    def reconcile_rate(self) -> float:
        return _rate(row.ok for row in self.reconcile_rows)

    def shapes_covered(self) -> int:
        return sum(1 for row in self.coverage_rows if row.ok)

    def false_findings(self) -> list[Row]:
        return [row for row in self.trap_rows if not row.ok]

    def misses(self) -> list[Row]:
        return [row for row in (*self.shape_rows, *self.summary_rows, *self.trap_rows,
                                *self.episode_rows, *self.reconcile_rows) if not row.ok]

    def gates(self, *, shapes: float, summaries: float, precision: float, episodes: float,
              reconcile: float, separation: float,
              shapes_covered: int = MIN_SHAPES_COVERED) -> list[str]:
        problems: list[str] = []
        if self.shape_accuracy() < shapes:
            problems.append("shape accuracy %.3f < %.3f" % (self.shape_accuracy(), shapes))
        if self.summary_accuracy() < summaries:
            problems.append("summary accuracy %.3f < %.3f"
                            % (self.summary_accuracy(), summaries))
        if self.precision() < precision:
            problems.append("precision %.3f < %.3f (%d trap(s) fired)"
                            % (self.precision(), precision, len(self.false_findings())))
        if self.episode_accuracy() < episodes:
            problems.append("episode accuracy %.3f < %.3f"
                            % (self.episode_accuracy(), episodes))
        if self.reconcile_rate() < reconcile:
            problems.append("reconciliation rate %.3f < %.3f"
                            % (self.reconcile_rate(), reconcile))
        gap = self.separation.get("gap", 0.0)
        if gap < separation:
            problems.append("churn separation %.3f < %.3f (local %.3f, reshuffle %.3f)"
                            % (gap, separation, self.separation.get("local", 0.0),
                               self.separation.get("reshuffle", 0.0)))
        if self.separation.get("local_band") not in (None, "local"):
            problems.append("the 20-file local change did not land in the `local` band "
                            "(it landed in %r)" % self.separation.get("local_band"))
        if self.separation.get("reshuffle_band") != "structural":
            problems.append("the 20-file cross-tree reshuffle did not land in the "
                            "`structural` band (it landed in %r)"
                            % self.separation.get("reshuffle_band"))
        if self.shapes_covered() < shapes_covered:
            problems.append("only %d of %d named shapes have a fixture that produces it"
                            % (self.shapes_covered(), shapes_covered))
        for row in self.false_findings():
            problems.append("a finding fired that should not have: %s — %s"
                            % (row.name, row.detail))
        return problems

    # -- the text -----------------------------------------------------------
    def as_dict(self) -> dict:
        return {"shapes": {"total": len(self.shape_rows),
                           "accuracy": round(self.shape_accuracy(), 4)},
                "summaries": {"total": len(self.summary_rows),
                              "accuracy": round(self.summary_accuracy(), 4)},
                "traps": {"total": len(self.trap_rows), "precision": round(self.precision(), 4)},
                "episodes": {"total": len(self.episode_rows),
                             "accuracy": round(self.episode_accuracy(), 4)},
                "reconciliation": {"total": len(self.reconcile_rows),
                                   "rate": round(self.reconcile_rate(), 4)},
                "shapes_covered": {"covered": self.shapes_covered(),
                                   "total": len(self.coverage_rows)},
                "separation": dict(self.separation),
                "false_findings": [dataclasses.asdict(row) for row in self.false_findings()],
                "misses": [dataclasses.asdict(row) for row in self.misses()]}

    def text(self, *, verbose: bool = False) -> str:
        lines = ["structure corpus: %d shape fixtures, %d traps, %d episodes, %d "
                 "reconciliation cases" % (len(self.shape_rows), len(self.trap_rows),
                                           len(self.episode_rows), len(self.reconcile_rows))]
        lines.append("  shape accuracy      %.3f   (%d/%d)"
                     % (self.shape_accuracy(), sum(r.ok for r in self.shape_rows),
                        len(self.shape_rows)))
        lines.append("  summary accuracy    %.3f   (%d/%d)  the one-sentence answer, byte exact"
                     % (self.summary_accuracy(), sum(r.ok for r in self.summary_rows),
                        len(self.summary_rows)))
        lines.append("  precision           %.3f   (%d trap(s) fired)"
                     % (self.precision(), len(self.false_findings())))
        lines.append("  episode accuracy    %.3f   (%d/%d)"
                     % (self.episode_accuracy(), sum(r.ok for r in self.episode_rows),
                        len(self.episode_rows)))
        lines.append("  reconciliation      %.3f   (%d/%d)  the table sums to the totals"
                     % (self.reconcile_rate(), sum(r.ok for r in self.reconcile_rows),
                        len(self.reconcile_rows)))
        lines.append("  shapes covered      %d/%d   (%s)"
                     % (self.shapes_covered(), len(self.coverage_rows),
                        ", ".join("%s %s" % (row.name, "ok" if row.ok else "MISSING")
                                  for row in self.coverage_rows)))
        lines.append("  churn separation    %.3f   (local %.3f [%s] vs reshuffle %.3f [%s])"
                     % (self.separation.get("gap", 0.0), self.separation.get("local", 0.0),
                        self.separation.get("local_band", "?"),
                        self.separation.get("reshuffle", 0.0),
                        self.separation.get("reshuffle_band", "?")))
        misses = self.misses()
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


# ---------------------------------------------------------------------------
# the runs
# ---------------------------------------------------------------------------
def score(*, policy: Policy | None = None, fuzz_seeds: int = FUZZ_SEEDS,
          verbose: bool = False) -> Report:
    """Run every table. Pure, deterministic, no IO."""
    base = policy or Policy.built_in()
    report = Report()
    produced_shapes: set[str] = set()
    for case in C.SHAPE_CASES:
        view = measure(C.fixture(case.fixture_name), base)
        shapes = [story.shape for story in view.stories]
        totals = view.totals()
        problems: list[str] = []
        if not shapes or shapes[0] != case.shape:
            problems.append("first story was %s, expected %s"
                            % (shapes or "none", case.shape))
        for key, want in (("files_moved", case.moved_files), ("lines_moved", case.moved_lines),
                          ("directories_added", case.directories_added),
                          ("directories_removed", case.directories_removed)):
            if totals[key] != want:
                problems.append("%s: expected %s, got %s" % (key, want, totals[key]))
        if view.churn.band != case.band:
            problems.append("band: expected %s, got %s (%.3f)"
                            % (case.band, view.churn.band, view.churn.score))
        ok = not problems
        if ok:
            produced_shapes.add(case.shape)
        report.shape_rows.append(Row(
            "shapes", "%s/%s" % (case.name, case.shape), case.shape,
            "%s · churn %.3f (%s)" % (shapes or "none", view.churn.score, view.churn.band),
            ok, detail="; ".join(problems) or case.note))
        sentence = view.summary()["sentence"]
        report.summary_rows.append(Row(
            "summaries", case.name, case.sentence, sentence, sentence == case.sentence,
            detail=case.note))
    for case in C.TRAP_CASES:
        report.trap_rows.append(_score_trap(case, base))
    for case in C.EPISODE_CASES:
        report.episode_rows.append(_score_episode(case, base))
    for shape in (SHAPE_PACKAGE_RENAME, SHAPE_SUBTREE_MOVE, SHAPE_MODULE_SPLIT, SHAPE_FLATTEN,
                  SHAPE_FILES_MOVED):
        named = shape in NAMED_SHAPES
        report.coverage_rows.append(Row(
            "coverage", shape, "a fixture that produces it" if named else "optional",
            "produced" if shape in produced_shapes else "not produced",
            shape in produced_shapes or not named))
    for name in C.NAMES:
        view = measure(C.fixture(name), base)
        problems = view.verify()
        report.reconcile_rows.append(Row(
            "reconcile", name, "table sums to the episode totals",
            "reconciles" if not problems else "; ".join(problems), not problems))
    for seed in range(fuzz_seeds):
        episode = C.fuzz_episode(seed)
        view = measure(episode, base)
        problems = view.verify()
        if not 0.0 <= view.churn.score <= 1.0:
            problems.append("churn %.3f outside [0,1]" % view.churn.score)
        if len(view.summary()["sentence"]) == 0:
            problems.append("empty summary")
        report.reconcile_rows.append(Row(
            "reconcile", "fuzz-%d" % seed, "table sums to the episode totals",
            "reconciles" if not problems else "; ".join(problems), not problems,
            detail=C.fuzz_plan_text(seed)))
    report.separation = _separation(base)
    return report


def _score_trap(case: C.TrapCase, base: Policy) -> Row:
    policy = base.with_knobs(source="corpus knob", **case.knobs) if case.knobs else base
    view = measure(C.fixture(case.fixture_name), policy)
    fired = sorted({finding.kind for finding in view.findings
                    if policy.enabled_for(finding.kind)})
    offenders = [kind for kind in case.must_not_fire if kind in fired]
    detail = case.note
    if offenders:
        named = [finding.render() for finding in view.findings
                 if finding.kind in offenders][:3]
        detail = "fired %s: %s" % (", ".join(offenders), " | ".join(named))
    return Row("traps", case.name, "silent on %s" % ", ".join(case.must_not_fire),
               ", ".join(fired) or "silent", not offenders, detail=detail)


def _score_episode(case: C.EpisodeCase, base: Policy) -> Row:
    view = measure(C.fixture(case.fixture_name), base)
    totals = view.totals()
    text = "\n".join(render.render(view, show_all=True))
    problems: list[str] = []
    for key, want in case.counts.items():
        if totals.get(key) != want:
            problems.append("%s: expected %r, got %r" % (key, want, totals.get(key)))
    missing = [needle for needle in case.must_say if needle not in text]
    if missing:
        problems.append("the card never said: %s" % ", ".join(missing))
    present = [needle for needle in case.must_not_say if needle in text]
    if present:
        problems.append("the card said something it should not have: %s" % ", ".join(present))
    return Row("episodes", case.name, "%s %s" % (case.counts, list(case.must_say)[:2]),
               "files_moved=%s stories=%s churn=%.3f"
               % (totals["files_moved"], totals["stories"], view.churn.score),
               not problems, detail="; ".join(problems) or case.note)


def _separation(policy: Policy) -> dict:
    """AC 3, as a number: the same twenty files, in place versus moved across the tree."""
    local = measure(C.fixture("local_change"), policy)
    reshuffle = measure(C.fixture("cross_tree_reshuffle"), policy)
    return {"local": local.churn.score, "local_band": local.churn.band,
            "reshuffle": reshuffle.churn.score, "reshuffle_band": reshuffle.churn.band,
            "gap": round(reshuffle.churn.score - local.churn.score, 3),
            "local_files": local.totals()["files"],
            "reshuffle_files": reshuffle.totals()["files"]}


def _rate(values) -> float:
    values = list(values)
    return sum(1 for value in values if value) / len(values) if values else 1.0


def separation(policy: Policy | None = None) -> dict:
    """The two headline churn numbers, exposed for `explain` and the contract test."""
    return _separation(policy or Policy.built_in())
