"""Scoring the corpus: what folded, what it should not have, and what the line said.

`fathom noise score` is the CI line. It runs four tables — file pairs, generated-file cases,
lockfile cases, episode scenarios — and reports four numbers, each with the failure that
matters named separately from the accuracy number:

| number | gate | the failure it is watching |
|---|---|---|
| pair accuracy | `--min-accuracy` | the claim was wrong in some direction |
| **fold precision** | `--min-precision` | **a `content` pair was called `reformat`** — the fold that hides work |
| fold recall | `--min-recall` | a formatter commit was reported as change: noise the reviewer still has to scroll |
| episode accuracy | `--min-episodes` | an episode collapsed that should not have, or refused when it should have |

Precision is the one that would be set to 1.0 if a corpus could be trusted to stay that
large: the cost asymmetry is total. A missed fold costs a scroll; a false fold costs a
reviewer a change they believed was not there, and after the third time they stop trusting
the tool, which is the product failing rather than a row being red. Recall is allowed to be
lower because the conservative answers (a YAML flow-vs-block change, shfmt's quoting, a
raw-string rewrite) are deliberate, are listed in the corpus with a note, and are *reported*
as change rather than silently swallowed.

The episode table checks the headline's *words*, not just the boolean: a collapse that says
"0 semantic changes" without naming the formatter, or a lockfile collapse that omits the
manifest sentence, fails even though it collapsed in the right direction — because the AC's
claim is about what the line carries, not about the branch it took.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass, field

from . import generated as G
from . import lockfile as L
from . import reformat as R
from .corpus import EPISODES, GENERATED, LOCKS, PAIRS, GeneratedCase, LockCase, Pair
from .episode import Episode, suppress
from .policy import Policy

#: The bars. `fathom noise score` prints them next to the numbers it gates.
MIN_ACCURACY = 0.95
#: Precision is not a rate to average: every false fold is one real change the tool hid, and
#: the thesis (§7, "never hide silently") makes that the one forbidden failure. The bar is
#: therefore absolute, and `Report.gates` additionally names each false fold, so a red run
#: says which change was hidden rather than only that the average slipped.
MIN_PRECISION = 1.0
MIN_RECALL = 0.90
#: The collapse is the headline behaviour of the ticket: "a reformat-only commit is one
#: line". Half of a headline is not a feature.
MIN_EPISODES = 1.0


@dataclass
class Row:
    """One scored case, with what the detector answered and what the corpus expected."""

    table: str
    name: str
    shape: str
    expected: str
    got: str
    ok: bool
    detail: str = ""

    @property
    def hides_work(self) -> bool:
        """Whether this miss folded something that should have stayed visible."""
        return self.expected in ("content", "undetermined", "hand-edit") and \
            self.got in ("reformat", "churn", "collapsed")


@dataclass
class Report:
    """Everything `fathom noise score` prints."""

    pair_rows: list[Row] = field(default_factory=list)
    generated_rows: list[Row] = field(default_factory=list)
    lock_rows: list[Row] = field(default_factory=list)
    episode_rows: list[Row] = field(default_factory=list)

    # -- the numbers --------------------------------------------------------
    def accuracy(self) -> float:
        rows = self.pair_rows
        return _rate(row.ok for row in rows)

    def precision(self) -> float:
        """Of everything called `reformat`, how much really was."""
        called = [row for row in self.pair_rows if row.got == "reformat"]
        return _rate(row.expected == "reformat" for row in called)

    def recall(self) -> float:
        expected = [row for row in self.pair_rows if row.expected == "reformat"]
        return _rate(row.got == "reformat" for row in expected)

    def generated_accuracy(self) -> float:
        return _rate(row.ok for row in self.generated_rows)

    def lock_accuracy(self) -> float:
        return _rate(row.ok for row in self.lock_rows)

    def episode_accuracy(self) -> float:
        return _rate(row.ok for row in self.episode_rows)

    def false_folds(self) -> list[Row]:
        return [row for row in (*self.pair_rows, *self.generated_rows, *self.episode_rows)
                if row.hides_work]

    def gates(self, *, accuracy: float, precision: float, recall: float,
              episodes: float) -> list[str]:
        problems: list[str] = []
        if self.accuracy() < accuracy:
            problems.append("pair accuracy %.3f < %.3f" % (self.accuracy(), accuracy))
        if self.precision() < precision:
            problems.append("fold precision %.3f < %.3f" % (self.precision(), precision))
        if self.recall() < recall:
            problems.append("fold recall %.3f < %.3f" % (self.recall(), recall))
        if self.episode_accuracy() < episodes:
            problems.append("episode accuracy %.3f < %.3f"
                            % (self.episode_accuracy(), episodes))
        for row in self.false_folds():
            problems.append("a fold hid work: %s/%s expected %s, got %s"
                            % (row.table, row.name, row.expected, row.got))
        return problems

    # -- the text -----------------------------------------------------------
    def as_dict(self) -> dict:
        return {"pairs": {"total": len(self.pair_rows), "accuracy": round(self.accuracy(), 4),
                          "precision": round(self.precision(), 4),
                          "recall": round(self.recall(), 4)},
                "generated": {"total": len(self.generated_rows),
                              "accuracy": round(self.generated_accuracy(), 4)},
                "lockfiles": {"total": len(self.lock_rows),
                              "accuracy": round(self.lock_accuracy(), 4)},
                "episodes": {"total": len(self.episode_rows),
                             "accuracy": round(self.episode_accuracy(), 4)},
                "false_folds": [dataclasses.asdict(row) for row in self.false_folds()],
                "misses": [dataclasses.asdict(row) for row in self._misses()]}

    def text(self, *, verbose: bool = False) -> str:
        lines = ["noise suppression: %d pairs, %d generated cases, %d lockfiles, %d episodes"
                 % (len(self.pair_rows), len(self.generated_rows), len(self.lock_rows),
                    len(self.episode_rows))]
        lines.append("  pair accuracy    %.3f   (%d/%d)"
                     % (self.accuracy(), sum(r.ok for r in self.pair_rows), len(self.pair_rows)))
        lines.append("  fold precision   %.3f   (of %d folds, %d were real)"
                     % (self.precision(), sum(1 for r in self.pair_rows if r.got == "reformat"),
                        sum(1 for r in self.pair_rows
                            if r.got == "reformat" and r.expected == "reformat")))
        lines.append("  fold recall      %.3f   (of %d reformats, %d caught)"
                     % (self.recall(), sum(1 for r in self.pair_rows
                                           if r.expected == "reformat"),
                        sum(1 for r in self.pair_rows
                            if r.expected == "reformat" and r.got == "reformat")))
        lines.append("  generated        %.3f" % self.generated_accuracy())
        lines.append("  lockfiles        %.3f" % self.lock_accuracy())
        lines.append("  episodes         %.3f" % self.episode_accuracy())
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
        return [row for row in (*self.pair_rows, *self.generated_rows, *self.lock_rows,
                                *self.episode_rows) if not row.ok]


# ---------------------------------------------------------------------------
# the runs
# ---------------------------------------------------------------------------
def score(*, policy: Policy | None = None, verbose: bool = False) -> Report:
    """Run every table. Pure, deterministic, no IO."""
    report = Report()
    active = policy or Policy.built_in()
    for case in PAIRS:
        report.pair_rows.append(_score_pair(case))
    for case in GENERATED:
        report.generated_rows.append(_score_generated(case))
    for case in LOCKS:
        report.lock_rows.append(_score_lock(case))
    for case in EPISODES:
        report.episode_rows.append(_score_episode(case, active))
    return report


def _score_pair(case: Pair) -> Row:
    verdict = R.compare(case.path, case.before, case.after)
    return Row("pairs", case.name, case.shape, case.expect, verdict.claim,
               ok=verdict.claim == case.expect,
               detail=case.note or verdict.reason)


def _score_generated(case: GeneratedCase) -> Row:
    verdict = G.inspect(case.path, case.before, case.after, path_class=case.path_class,
                        band=case.band, generator_hint=case.hint)
    got = "churn" if verdict.is_churn else ("hand-edit" if verdict.claim == G.HAND_EDIT
                                            else verdict.claim)
    ok = got == case.expect and (not case.kind or verdict.kind == case.kind)
    detail = case.note
    if not ok and not detail:
        detail = "%s: %s" % (verdict.kind or verdict.claim,
                             "; ".join(verdict.evidence[:1]) or verdict.reason)
    return Row("generated", case.name, "generated", case.expect, got, ok=ok, detail=detail)


def _score_lock(case: LockCase) -> Row:
    changed = [case.path] + ([case.manifest] if case.manifest_changed else [])
    summary = L.summarize(case.path, case.before, case.after, changed_paths=changed)
    counts = summary.counts()
    ok = counts == case.expect
    if case.manifest and case.manifest_changed:
        ok = ok and summary.manifest and summary.manifest.lower().endswith(
            case.manifest.lower())
    if not case.expect and case.note:
        ok = ok and summary.unreadable
    detail = case.note or summary.headline()
    if not ok:
        detail = "expected %s, got %s (%s)" % (case.expect, counts, summary.note or "parsed")
    return Row("lockfiles", case.name, "lockfile",
               ",".join("%s×%d" % (kind, n) for kind, n in sorted(case.expect.items()))
               or "unreadable",
               ",".join("%s×%d" % (kind, n) for kind, n in sorted(counts.items()))
               + (" unreadable" if summary.unreadable else ""),
               ok=ok, detail=detail)


def _score_episode(case: EpisodeCase, policy: Policy) -> Row:
    files = []
    for path, (before, after) in sorted(case.files.items()):
        files.append(_change(path, before, after))
    view = suppress(Episode(id=case.name, files=tuple(files)), policy)
    collapsed = view.collapse is not None
    text = "\n".join([view.headline()] +
                      ([view.collapse.reason_line()] + list(view.collapse.evidence)
                       if view.collapse else []))
    missing = [needle for needle in case.must_mention if needle not in text]
    ok = collapsed == case.collapse and not missing
    detail = case.note or view.collapse_reason
    if missing:
        detail = "the collapsed line never said: %s" % ", ".join(missing)
    return Row("episodes", case.name, case.shape,
               "collapse" if case.collapse else "expanded",
               "collapsed" if collapsed else "expanded", ok=ok, detail=detail)


def _change(path: str, before: str | None, after: str | None):
    from .episode import Change
    change_type = "modified"
    if before is None:
        change_type = "added"
    if after is None:
        change_type = "deleted"
    return Change(path=path, before=before, after=after, change_type=change_type)


def _rate(values) -> float:
    values = list(values)
    if not values:
        return 1.0
    return sum(1 for value in values if value) / len(values)
