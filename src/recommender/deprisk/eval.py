"""Scoring the corpus: the gates that make 'it renders' not the same claim as 'it is right'.

Every number here is a *judgement* checked against a hand-authored expectation, not a golden
string compared with itself. The bars, in the order the ticket's criteria pose them:

* **provenance accuracy** — for every fixture with a provenance expectation, the answer's state and
  its intent state must be the ones the case names. `MIN_PROVENANCE = 1.0`, because the difference
  between `intent: "…"` and `intent: not recorded` is the acceptance criterion, and getting it wrong
  half the time would be getting it wrong;
* **licence precision and recall** — every package the corpus says violates must be flagged, and
  nothing else may be. Precision is the trap half: `dual_licence_ok`, `copyleft_in_application`,
  `integrity_only_change` and `removed_dependency` exist to catch a checker that fires on everything;
* **advisory recall** — every advisory id in a recorded OSV response must appear on the row, and no
  advisory may be invented. Includes the unseverity one, which a severity-only parser would drop;
* **freshness accuracy** — the same delta under three cache states must produce three different,
  correct claims, and the offline case must still reconcile;
* **reconciliation** — every fixture and every generated episode must pass `RiskView.verify()`.

`false_findings()` lists what a gate would have failed on, so a red CI run says which claim broke
rather than only that one did.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from . import corpus as C
from . import render
from .delta import measure
from .licence import STATE_REMOVED

MIN_PROVENANCE = 1.0
MIN_LICENCE_PRECISION = 1.0
MIN_LICENCE_RECALL = 1.0
MIN_ADVISORY_RECALL = 1.0
MIN_FRESHNESS = 1.0
MIN_RECONCILE = 1.0
MIN_FINDINGS = 1.0
MIN_CASES = 28.0
FUZZ_SEEDS = 12

#: The dimensions, in the order the report prints them.
DIMENSIONS: tuple[str, ...] = ("provenance", "intent", "licence", "advisory", "freshness",
                               "findings", "reconcile", "determinism")


@dataclass(frozen=True)
class Row:
    """One case scored on one dimension."""

    case: str
    dimension: str
    ok: bool
    detail: str = ""

    @property
    def label(self) -> str:
        return "%s/%s" % (self.case, self.dimension)


@dataclass
class Report:
    """The whole scoring run: rows, and the gates over them."""

    rows: list[Row] = field(default_factory=list)
    cases: int = 0
    fuzz: int = 0

    def _rate(self, dimension: str) -> float:
        rows = [row for row in self.rows if row.dimension == dimension]
        if not rows:
            return 0.0
        return sum(1 for row in rows if row.ok) / float(len(rows))

    def provenance_accuracy(self) -> float:
        return self._rate("provenance")

    def intent_accuracy(self) -> float:
        return self._rate("intent")

    def licence_recall(self) -> float:
        rows = [row for row in self.rows if row.dimension == "licence"]
        wanted = sum(1 for row in rows if "recall" in row.detail)
        hits = sum(1 for row in rows if "recall" in row.detail and row.ok)
        return hits / float(wanted) if wanted else 1.0

    def licence_precision(self) -> float:
        rows = [row for row in self.rows if row.dimension == "licence"]
        judged = [row for row in rows if "precision" in row.detail]
        if not judged:
            return 1.0
        return sum(1 for row in judged if row.ok) / float(len(judged))

    def advisory_recall(self) -> float:
        rows = [row for row in self.rows if row.dimension == "advisory"]
        wanted = [row for row in rows if "expected" in row.detail]
        if not wanted:
            return 1.0
        return sum(1 for row in wanted if row.ok) / float(len(wanted))

    def freshness_accuracy(self) -> float:
        rows = [row for row in self.rows if row.dimension == "freshness"]
        if not rows:
            return 1.0
        return sum(1 for row in rows if row.ok) / float(len(rows))

    def findings_accuracy(self) -> float:
        return self._rate("findings")

    def reconcile_rate(self) -> float:
        rows = [row for row in self.rows if row.dimension == "reconcile"]
        if not rows:
            return 0.0
        return sum(1 for row in rows if row.ok) / float(len(rows))

    def determinism_rate(self) -> float:
        return self._rate("determinism")

    def false_findings(self) -> list[Row]:
        """Everything that fired where the corpus said it should not."""
        return [row for row in self.rows if not row.ok]

    def gates(self, *, provenance: float = MIN_PROVENANCE,
              licence_precision: float = MIN_LICENCE_PRECISION,
              licence_recall: float = MIN_LICENCE_RECALL,
              advisory_recall: float = MIN_ADVISORY_RECALL,
              freshness: float = MIN_FRESHNESS,
              reconcile: float = MIN_RECONCILE,
              findings: float = MIN_FINDINGS,
              min_cases: float = MIN_CASES) -> list[str]:
        """Each gate as a sentence naming its own number, so a failure is a sentence too."""
        wanted = [
            (self.provenance_accuracy(), provenance, "provenance accuracy"),
            (self.intent_accuracy(), provenance, "intent accuracy"),
            (self.licence_precision(), licence_precision, "licence precision"),
            (self.licence_recall(), licence_recall, "licence recall"),
            (self.advisory_recall(), advisory_recall, "advisory recall"),
            (self.freshness_accuracy(), freshness, "advisory freshness accuracy"),
            (self.findings_accuracy(), findings, "finding-table accuracy"),
            (self.reconcile_rate(), reconcile, "reconciliation"),
            (self.determinism_rate(), 1.0, "determinism"),
            (float(self.cases), min_cases, "cases scored"),
        ]
        return ["%s is %.3f, under the bar %.3f" % (name, got, bar)
                for got, bar, name in wanted if got + 1e-12 < bar]

    def as_dict(self) -> dict:
        return {"cases": self.cases, "fuzz_episodes": self.fuzz,
                "provenance_accuracy": self.provenance_accuracy(),
                "intent_accuracy": self.intent_accuracy(),
                "licence_precision": self.licence_precision(),
                "licence_recall": self.licence_recall(),
                "advisory_recall": self.advisory_recall(),
                "freshness_accuracy": self.freshness_accuracy(),
                "findings_accuracy": self.findings_accuracy(),
                "reconcile_rate": self.reconcile_rate(),
                "determinism_rate": self.determinism_rate(),
                "misses": [{"case": row.case, "dimension": row.dimension, "detail": row.detail}
                           for row in self.false_findings()]}

    def text(self, *, verbose: bool = False) -> str:
        lines = [
            "dependency-risk corpus: %d authored episode(s), %d generated" % (self.cases,
                                                                             self.fuzz),
            "  provenance accuracy  %.3f  (%d row(s))"
            % (self.provenance_accuracy(),
               len([row for row in self.rows if row.dimension == "provenance"])),
            "  intent accuracy      %.3f" % self.intent_accuracy(),
            "  licence precision    %.3f" % self.licence_precision(),
            "  licence recall       %.3f" % self.licence_recall(),
            "  advisory recall      %.3f" % self.advisory_recall(),
            "  freshness accuracy   %.3f" % self.freshness_accuracy(),
            "  finding table        %.3f" % self.findings_accuracy(),
            "  reconciliation       %.3f" % self.reconcile_rate(),
            "  determinism          %.3f" % self.determinism_rate(),
        ]
        misses = self.false_findings()
        if misses:
            lines.append("  %d miss(es):" % len(misses))
            for row in misses[: (0 if not verbose else 200) or len(misses)]:
                lines.append("    %-34s %-11s %s" % (row.label, "", row.detail))
        else:
            lines.append("  no misses")
        return "\n".join(lines)


def score(*, policy=None, verbose: bool = False) -> Report:
    """Score every fixture under the policy it names, then the generated episodes."""
    report = Report(cases=len(C.CASES))
    for case in C.CASES:
        active = policy or C.policy_for(case.name)
        cache = C.cache_of(case.name, now=C.NOW)
        view = measure(case.input, active, cache=cache, now=C.NOW)
        report.rows.extend(_score_case(case, view))
        report.rows.extend(_score_determinism(case, active, cache))
    for seed in range(FUZZ_SEEDS):
        for name, episode in C.fuzz_cases(seed, 6):
            active = policy or C.policy()
            cache = C.cache_for("fresh", now=C.NOW) if seed % 2 else None
            view = measure(episode, active, cache=cache, now=C.NOW)
            report.fuzz += 1
            problems = view.verify()
            report.rows.append(Row(name, "reconcile", not problems,
                                   "; ".join(problems) or "reconciles"))
    report.rows.extend(_score_reconciliation())
    return report


def _score_case(case: C.Case, view) -> list[Row]:
    rows: list[Row] = []
    for package, want in case.answers.items():
        answer = view.answer_for(package)
        rows.append(Row(case.name, "provenance", answer is not None and answer.state == want,
                        "expected provenance %r for %s, got %s"
                        % (want, package, answer.state if answer else "no row")))
    for package, want in case.intents.items():
        answer = view.answer_for(package)
        rows.append(Row(case.name, "intent", answer is not None and answer.intent_state == want,
                        "expected intent %r for %s, got %s"
                        % (want, package, answer.intent_state if answer else "no row")))
    violations = {row.package.text for row in view.rows if row.violation}
    for package in case.violated:
        rows.append(Row(case.name, "licence", package in violations,
                        "recall: expected a licence violation for %s, saw %s"
                        % (package, sorted(violations) or "none")))
    for package in case.clean:
        rows.append(Row(case.name, "licence", package not in violations,
                        "precision: %s must not violate, violations were %s"
                        % (package, sorted(violations) or "none")))
    for package, want in case.states.items():
        row = next((item for item in view.rows if item.package.text == package), None)
        rows.append(Row(case.name, "licence", row is not None and row.state == want,
                        "expected row state %r for %s, got %s"
                        % (want, package, row.state if row else "no row")))
    for package, wanted_ids in case.advisories.items():
        row = next((item for item in view.advisory_rows
                    if "%s %s" % (item.lookup.package, item.lookup.version) == package), None)
        if not wanted_ids:
            rows.append(Row(case.name, "advisory", row is not None and not row.advisories,
                            "expected %s to come back clean, got %s"
                            % (package, [one.id for one in row.advisories] if row else "no row")))
            continue
        found = {one.id for one in (row.advisories if row else ())}
        rows.append(Row(case.name, "advisory",
                        set(wanted_ids) <= found and found == set(wanted_ids),
                        "expected advisories %s for %s, got %s"
                        % (list(wanted_ids), package, sorted(found) or "none")))
    if case.freshness:
        rows.append(Row(case.name, "freshness", view.freshness.state == case.freshness,
                        "expected freshness %r, got %r" % (case.freshness,
                                                           view.freshness.state)))
    kinds = {finding.kind for finding in view.findings}
    missing = [kind for kind in case.kinds if kind not in kinds]
    extra = [kind for kind in case.absent if kind in kinds]
    rows.append(Row(case.name, "findings", not missing and not extra,
                    "missing %s; unexpected %s" % (missing or "none", extra or "none")))
    for fragment in case.sentence:
        rows.append(Row(case.name, "findings", fragment in view.summary()["sentence"],
                        "summary sentence is missing %r" % fragment))
    problems = view.verify()
    rows.append(Row(case.name, "reconcile", not problems, "; ".join(problems) or "reconciles"))
    return rows


def _score_determinism(case: C.Case, policy, cache) -> list[Row]:
    first = measure(case.input, policy, cache=cache, now=C.NOW)
    second = measure(case.input, policy, cache=cache, now=C.NOW)
    left = "\n".join(render.render(first, show_all=True))
    right = "\n".join(render.render(second, show_all=True))
    return [Row(case.name, "determinism", left == right and first.as_dict() == second.as_dict(),
                "two renders of the same fixture differ" if left != right else "identical bytes")]


def _score_reconciliation() -> list[Row]:
    """The corpus-wide properties that no single case can show on its own."""
    rows: list[Row] = []
    # The same delta and the same episode under three cache states must make three different
    # claims. Holding everything else fixed is the point: a reviewer comparing two episodes should
    # be able to read the difference in the freshness line and nowhere else.
    delta = C.CASE_INDEX["advisory_hot"].input
    plan_cache = {"fresh": "fresh", "missing": "", "stale": "stale"}
    views = {}
    for name, cache_name in plan_cache.items():
        views[name] = measure(delta, C.policy(), cache=C.cache_for(cache_name, now=C.NOW),
                              now=C.NOW)
    expected = {"missing": "missing", "fresh": "fresh", "stale": "expired"}
    for name, want in expected.items():
        view = views[name]
        rows.append(Row("three-states", "freshness", view.freshness.state == want,
                        "%s cache should read %s, read %r" % (name, want, view.freshness.state)))
    if len({view.freshness.state for view in views.values()}) != 3:
        rows.append(Row("three-states", "freshness", False,
                        "the three cache states did not produce three distinct claims"))
    # 'No advisories found' must be a claim the freshness state actually licenses.
    rows.append(Row("stale-never-claims-clean", "freshness", not views["stale"].freshness
                    .claims_clean, "an expired cache may not claim 'no advisories'"))
    rows.append(Row("missing-never-claims-clean", "freshness",
                    not views["missing"].freshness.claims_clean,
                    "a cache nobody has filled may not claim 'no advisories'"))
    rows.append(Row("fresh-claims-clean", "freshness", views["fresh"].freshness.claims_clean,
                    "a fresh cache should be allowed to claim 'no advisories'"))
    # Every removable row: a removed package is never a policy question.
    for case in C.CASES:
        for package, want in case.states.items():
            if want != STATE_REMOVED:
                continue
            view = measure(case.input, C.policy_for(case.name),
                           cache=C.cache_of(case.name, now=C.NOW), now=C.NOW)
            row = next((item for item in view.rows if item.package.text == package), None)
            rows.append(Row(case.name, "licence", row is not None and not row.violation,
                            "a removed package must not be a policy question"))
    return rows


def summary_line(report: Report) -> str:
    return ("provenance %.3f · licence p/r %.3f/%.3f · advisory %.3f · freshness %.3f · "
            "reconcile %.3f"
            % (report.provenance_accuracy(), report.licence_precision(), report.licence_recall(),
               report.advisory_recall(), report.freshness_accuracy(), report.reconcile_rate()))
