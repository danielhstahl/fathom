"""Accuracy scoring: the labelled fixture set, run and reported (AC 1).

`fathom change score` is the CI line. It prints overall accuracy, the per-class table, the
per-shape table, and every misclassification with the reason the classifier gave — because a
failing accuracy gate wants to be *diagnosable* in the CI log, not merely red.

Two numbers are reported, and only the first gates CI:

* **accuracy** — exact class agreement over every fixture row. That is AC 1's "at least 95
  percent", and the threshold is a CLI flag (`--min`, default `0.95`) so a reviewer can see
  the bar next to the number.
* **ambiguous-but-agreed / other-with-no-rule** — counts, not a gate. They are the two ways a
  classifier passes a label test while being useless: answering `other` everywhere would
  score well on a set with no `other` rows in it, so `tests/test_change_classifier_contract.py`
  asserts the corpus contains `other` rows and asserts the `other` answers are the ones the
  corpus says are ambiguous — never a path that had a rule.

Nothing here touches the network, a model, or the clock. It is a loop over `corpus.rows()`
through one classifier, which is what makes the number reproducible.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass, field

from recommender.taxonomy import classes as C

from .corpus import FIXTURES, counts, rows
from .engine import ChangeClassifier, default_classifier

#: AC 1's bar. A flag, so the number and the bar are printed together in the CI log.
MIN_ACCURACY = 0.95


@dataclass
class Row:
    shape: str
    path: str
    expected: str
    got: str
    confidence: float
    band: str
    reason: str
    path_only: str = ""
    source: str = ""

    @property
    def ok(self) -> bool:
        return self.expected == self.got

    @property
    def needs_content(self) -> bool:
        """Whether the path alone would have answered differently."""
        return bool(self.path_only) and self.path_only != self.got


@dataclass
class Report:
    rows: list[Row] = field(default_factory=list)
    classifier: dict = field(default_factory=dict)

    @property
    def total(self) -> int:
        return len(self.rows)

    @property
    def correct(self) -> int:
        return sum(1 for row in self.rows if row.ok)

    @property
    def accuracy(self) -> float:
        return (self.correct / self.total) if self.total else 0.0

    def per_class(self) -> dict[str, dict]:
        out: dict[str, dict] = {}
        for row in self.rows:
            entry = out.setdefault(row.expected, {"total": 0, "correct": 0, "got": {}})
            entry["total"] += 1
            entry["correct"] += 1 if row.ok else 0
            entry["got"][row.got] = entry["got"].get(row.got, 0) + 1
        return {cls: out[cls] for cls in C.PATH_CLASSES if cls in out}

    def per_shape(self) -> dict[str, dict]:
        out: dict[str, dict] = {}
        for row in self.rows:
            entry = out.setdefault(row.shape, {"total": 0, "correct": 0})
            entry["total"] += 1
            entry["correct"] += 1 if row.ok else 0
        for entry in out.values():
            entry["accuracy"] = round(entry["correct"] / entry["total"], 4)
        return dict(sorted(out.items()))

    def mistakes(self) -> list[Row]:
        return [row for row in self.rows if not row.ok]

    def content_dependent(self) -> list[Row]:
        """Rows the path alone would have got wrong — the fixture set's own proof."""
        return [row for row in self.rows if row.needs_content]

    @property
    def path_only_accuracy(self) -> float:
        if not self.rows:
            return 0.0
        hits = sum(1 for row in self.rows if row.path_only == row.expected)
        return hits / len(self.rows)

    def confusion(self) -> list[tuple[str, str, int]]:
        pairs: dict[tuple[str, str], int] = {}
        for row in self.mistakes():
            pairs[(row.expected, row.got)] = pairs.get((row.expected, row.got), 0) + 1
        return sorted(pairs.items(), key=lambda kv: (-kv[1], kv[0]))

    def as_dict(self) -> dict:
        return {"rows": self.total, "correct": self.correct,
                "accuracy": round(self.accuracy, 4), "min": MIN_ACCURACY,
                "pass": self.accuracy >= MIN_ACCURACY,
                "path_only_accuracy": round(self.path_only_accuracy, 4),
                "content_dependent_rows": len(self.content_dependent()),
                "classes": {cls: {"total": entry["total"], "correct": entry["correct"],
                                  "accuracy": round(entry["correct"] / entry["total"], 4),
                                  "got": entry["got"]}
                            for cls, entry in self.per_class().items()},
                "shapes": self.per_shape(), "classifier": self.classifier,
                "mistakes": [dataclasses.asdict(row) for row in self.mistakes()]}


def score(classifier: ChangeClassifier | None = None, *,
          fixtures: dict[str, dict] | None = None) -> Report:
    """Run every fixture row through `classifier` and tabulate the agreement.

    Each row is then run a second time with content evidence switched off, and the pair is
    kept on the row. That is the check that this fixture set measures *this ticket* rather than
    `e0.9`: if every row were answerable from the path, the classifier could pass with the
    content half deleted, and `report_text` would show the two accuracies equal.
    """
    clf = classifier or default_classifier()
    path_only = ChangeClassifier(clf.taxonomy, config=clf.config, use_content=False,
                                 enable_cache=False)
    report = Report(classifier=clf.stats())
    for shape, path, expected, content in rows():
        decision = clf.classify_path(path, content)
        report.rows.append(Row(shape=shape, path=path, expected=expected, got=decision.cls,
                               confidence=decision.confidence, band=decision.band,
                               reason=decision.reason,
                               path_only=path_only.classify_path(path).cls,
                               source=(decision.winner or {}).get("source", "")))
    return report


def report_text(report: Report, *, minimum: float = MIN_ACCURACY, verbose: bool = False
                ) -> str:
    """The CI report. Fixed-width, sorted, and complete enough to debug from."""
    lines: list[str] = []
    stats = report.classifier
    lines.append("fathom change score · classifier v%s · taxonomy v%s (digest %s) · "
                 "markers v%s" % (stats.get("classifier_version"), stats.get("taxonomy_version"),
                                  str(stats.get("table_digest", ""))[:8],
                                  stats.get("markers_version")))
    lines.append("%d labelled paths over %d repository shapes"
                 % (report.total, len(report.per_shape())))
    lines.append("")
    verdict = "PASS" if report.accuracy >= minimum else "FAIL"
    lines.append("accuracy  %.1f%% (%d/%d)  ·  bar %.0f%%  ·  %s"
                 % (report.accuracy * 100, report.correct, report.total, minimum * 100, verdict))
    lines.append("          the same set scored from paths alone: %.1f%% — %d row%s need the "
                 "content half of the classifier to be answered correctly"
                 % (report.path_only_accuracy * 100, len(report.content_dependent()),
                    "" if len(report.content_dependent()) == 1 else "s"))
    lines.append("")
    lines.append("class         total  correct   acc   answered")
    for cls, entry in report.per_class().items():
        answered = ", ".join("%s=%d" % kv for kv in
                             sorted(entry["got"].items(), key=lambda kv: -kv[1]))
        acc = entry["correct"] / entry["total"]
        lines.append("%-12s %6d %8d  %5.0f%%   %s"
                     % (cls, entry["total"], entry["correct"], acc * 100, answered))
    lines.append("")
    lines.append("shape                      acc")
    for shape, entry in report.per_shape().items():
        note = FIXTURES[shape].get("note", "")
        lines.append("%-26s %5.0f%%   %s" % (shape, entry["accuracy"] * 100, note))
    mistakes = report.mistakes()
    lines.append("")
    if not mistakes:
        lines.append("no misclassifications")
    else:
        lines.append("misclassified (%d):" % len(mistakes))
        for row in mistakes:
            lines.append("  %-44s expected %-10s got %-10s %.2f %s"
                         % (row.path, row.expected, row.got, row.confidence, row.band))
            if verbose:
                lines.append("      %s" % row.reason)
        lines.append("")
        lines.append("confusion (expected → got):")
        for (expected, got), count in report.confusion():
            lines.append("  %-11s → %-11s %d" % (expected, got, count))
    unclassified = [row for row in report.rows if row.got == C.PathClass.OTHER]
    lines.append("")
    lines.append("answered `other` on %d path%s; the corpus expects `other` on %d of them"
                 % (len(unclassified), "" if len(unclassified) == 1 else "s",
                    sum(1 for row in report.rows if row.expected == C.PathClass.OTHER)))
    content_rows = report.content_dependent()
    if content_rows:
        lines.append("")
        lines.append("content evidence decided %d row%s (path alone said something else):"
                     % (len(content_rows), "" if len(content_rows) == 1 else "s"))
        for row in content_rows:
            lines.append("  %-44s %-10s  ← path said %-10s  via %s"
                         % (row.path, row.got, row.path_only, row.source))
    lines.append("corpus class coverage: %s"
                 % ", ".join("%s=%d" % kv for kv in counts().items() if kv[1]))
    empty = [cls for cls, count in counts().items() if not count]
    if empty:
        lines.append("WARNING: no fixture rows for %s" % ", ".join(empty))
    return "\n".join(lines) + "\n"


def main(argv=None) -> int:
    """`fathom change score` entry point: exit non-zero when the bar is missed."""
    import argparse

    ap = argparse.ArgumentParser(prog="fathom change score")
    ap.add_argument("--min", type=float, default=MIN_ACCURACY)
    ap.add_argument("--verbose", "-v", action="store_true")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args(argv)
    report = score()
    if args.json:
        import json
        print(json.dumps(report.as_dict(), indent=2, sort_keys=True))
    else:
        print(report_text(report, minimum=args.min, verbose=args.verbose), end="")
    return 0 if report.accuracy >= args.min else 1
