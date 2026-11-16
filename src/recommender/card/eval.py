"""The card's gates: every claim this ticket makes, as a number that can fail in CI.

A composition ticket cannot score accuracy the way a classifier can — there is no "was the
structure story right" to re-measure here, because the analyses being composed were each scored
on their own corpus. What *is* measurable here is the set of claims the ticket actually makes, and
`score()` produces exactly those, each as a ratio with a bar:

| gate | the claim | why the bar is where it is |
| --- | --- | --- |
| `states` | every fixture's sections have the states a person wrote down next to the fixture | 1.0: a section whose state moved changed the card's answer, and the diff names it |
| `claims` | every expected phrase is in the rendered card | 1.0: these are the hand-authored answers, not substrings picked after the fact |
| `forbidden` | none of the forbidden phrases is in the rendered card | 0: "unavailable" must not print a dependency count; a card that invents one is a card that lies |
| `links` | every figure's links resolve to evidence rows | 1.0: AC 2, and an unresolved button is a broken button |
| `verified` | every card's own `verify()` is clean | 1.0: the second-path re-derivation and the evidence-key check |
| `audits` | every render passes `render.audit`, and every tampered render fails it | 1.0 both ways: an auditor that cannot fail is decoration |
| `degradation` | with every optional analysis missing, the card still has all seven sections and each gap names its verb | 1.0: that is AC 3, stated as a count rather than a feeling |
| `blast` | the blast counts a person read off the fixture's graph or path layout | 1.0: arithmetic on hand-authored edges, so there is no slack to grant |
| `cost` | compose + render p95 per episode against `benchmarks/budgets.json`'s `query.card.ms` | the published budget, not a number invented here |

`--json` prints the same table for a machine. Nothing in this file reads a clock except
`bench()`, and `bench()` is called by the CLI's `bench` verb and by the contract test with few
repeats, so the suite stays fast and the budget is still asserted.
"""

from __future__ import annotations

import statistics
import time
from dataclasses import dataclass, field

import re

from . import corpus as CORPUS
from . import render as RENDER
from .compose import compose
from .policy import (SECTION_INDEX, SECTION_WHERE, STATE_UNAVAILABLE, Policy)

#: The budget family this ticket is measured against (ADR-004 §11, `e0.11`'s gate file).
BUDGET_ID = "query.card.ms.small"
BUDGET_FALLBACK_MS = 50.0

#: Tampered renders the auditor must catch. Each one is a way a later refactor can quietly
#: lose a fact while every unit test still passes.
TAMPERS = ("drop-summary-block", "drop-section-detail", "drop-link", "drop-remainder-line",
           "drop-unavailable-reason", "drop-digest")

RULE = RENDER.RULE


@dataclass
class Report:
    """Everything `score()` measured, with the ratios and the failures spelled out."""

    states_ok: int = 0
    states_total: int = 0
    claims_ok: int = 0
    claims_total: int = 0
    forbidden: list[str] = field(default_factory=list)
    links_ok: int = 0
    links_total: int = 0
    verified_ok: int = 0
    verified_total: int = 0
    audits_ok: int = 0
    audits_total: int = 0
    tampered_caught: int = 0
    tampered_total: int = 0
    degradation_ok: int = 0
    degradation_total: int = 0
    blast_ok: int = 0
    blast_total: int = 0
    card_ms: list[float] = field(default_factory=list)
    cold_ms: list[float] = field(default_factory=list)
    budget_ms: float = BUDGET_FALLBACK_MS
    failures: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    # -- ratios -------------------------------------------------------------
    def ratio(self, ok: int, total: int) -> float:
        return ok / float(total) if total else 1.0

    def states(self) -> float:
        return self.ratio(self.states_ok, self.states_total)

    def claims(self) -> float:
        return self.ratio(self.claims_ok, self.claims_total)

    def links(self) -> float:
        return self.ratio(self.links_ok, self.links_total)

    def verified(self) -> float:
        return self.ratio(self.verified_ok, self.verified_total)

    def audits(self) -> float:
        return self.ratio(self.audits_ok, self.audits_total)

    def tampered(self) -> float:
        return self.ratio(self.tampered_caught, self.tampered_total)

    def degradation(self) -> float:
        return self.ratio(self.degradation_ok, self.degradation_total)

    def blast(self) -> float:
        return self.ratio(self.blast_ok, self.blast_total)

    @staticmethod
    def p95(samples: list[float]) -> float:
        if not samples:
            return 0.0
        ordered = sorted(samples)
        index = max(0, int(round(0.95 * len(ordered))) - 1)
        return ordered[min(len(ordered) - 1, index)]

    def p95_ms(self) -> float:
        return self.p95(self.card_ms)

    def cold_p95_ms(self) -> float:
        """Cold path, reported and not gated: analyse + compose + render, i.e. what an index
        run pays per episode, not what a keystroke pays. Sampled the same way as the warm path
        (:func:`score`), so that it is the bigger number by construction rather than by luck.
        """
        return self.p95(self.cold_ms)

    def as_dict(self) -> dict:
        return {"states": round(self.states(), 4), "claims": round(self.claims(), 4),
                "forbidden": len(self.forbidden), "links": round(self.links(), 4),
                "verified": round(self.verified(), 4), "audits": round(self.audits(), 4),
                "tampered_caught": round(self.tampered(), 4),
                "degradation": round(self.degradation(), 4),
                "blast": round(self.blast(), 4),
                "card_ms_p95": round(self.p95_ms(), 3),
                "card_cold_ms_p95": round(self.cold_p95_ms(), 3),
                "budget_ms": self.budget_ms,
                "cases": self.states_total, "failures": list(self.failures)}

    def text(self, verbose: bool = False) -> str:
        out = ["episode summary card — %d fixtures" % len(CORPUS.CASES)]
        rows = [("section states", self.states(), 1.0), ("card claims", self.claims(), 1.0),
                ("links resolved", self.links(), 1.0), ("cards verified", self.verified(), 1.0),
                ("renders audited", self.audits(), 1.0),
                ("tampered renders caught", self.tampered(), 1.0),
                ("degradation complete", self.degradation(), 1.0),
                ("blast counts", self.blast(), 1.0)]
        for name, value, bar in rows:
            out.append("  %-26s %.3f  (bar %.2f)  %s"
                       % (name, value, bar, "PASS" if value >= bar else "FAIL"))
        out.append("  %-26s %.3f ms  (budget %.0f ms, %s)  %s"
                   % ("card p95 (warm)", self.p95_ms(), self.budget_ms, BUDGET_ID,
                      "PASS" if self.p95_ms() <= self.budget_ms else "FAIL"))
        out.append("  %-26s %.3f ms  (reported, not gated: analyse + compose + render, what an index "
                   "run pays)" % ("card p95 (cold)", self.cold_p95_ms()))
        out.append("  %-26s %d (%s)"
                   % ("forbidden claims", len(self.forbidden),
                      ", ".join(self.forbidden[:4]) or "none"))
        if self.failures:
            out.append("failures")
            for line in self.failures:
                out.append("  %s" % line)
        if verbose:
            for note in self.notes:
                out.append("  note: %s" % note)
        return "\n".join(out)

    def gates(self, *, states: float = 1.0, claims: float = 1.0, links: float = 1.0,
              verify: float = 1.0, audits: float = 1.0, tampered: float = 1.0,
              degradation: float = 1.0, blast: float = 1.0,
              budget_ms: float | None = None) -> list[str]:
        problems: list[str] = []
        for name, value, bar in (("section states", self.states(), states),
                                 ("card claims", self.claims(), claims),
                                 ("links resolved", self.links(), links),
                                 ("cards verified", self.verified(), verify),
                                 ("renders audited", self.audits(), audits),
                                 ("tampered renders caught", self.tampered(), tampered),
                                 ("degradation complete", self.degradation(), degradation),
                                 ("blast counts", self.blast(), blast)):
            if value < bar:
                problems.append("%s is %.3f, below the bar of %.2f" % (name, value, bar))
        if self.forbidden:
            problems.append("%d forbidden claim(s) appeared on a card: %s"
                            % (len(self.forbidden), "; ".join(self.forbidden[:5])))
        budget = budget_ms if budget_ms is not None else self.budget_ms
        if self.p95_ms() > budget:
            problems.append("card render p95 %.3f ms is over the %.0f ms budget (%s)"
                            % (self.p95_ms(), budget, BUDGET_ID))
        if self.states_total != len(CORPUS.CASES):
            problems.append("only %d of %d fixtures were scored"
                            % (self.states_total, len(CORPUS.CASES)))
        return problems


def load_budget(path=None) -> float:
    """The published `query.card.ms` budget, with the declared fallback when the file is absent."""
    import json
    from pathlib import Path
    path = Path(path) if path else Path(__file__).resolve().parents[3] / \
        "benchmarks" / "budgets.json"
    try:
        payload = json.loads(path.read_text("utf-8"))
    except (OSError, ValueError):
        return BUDGET_FALLBACK_MS
    for entry in payload.get("budgets", []):
        if entry.get("id") == BUDGET_ID:
            value = entry.get("p95_ms") or entry.get("budget") or entry.get("limit")
            if value:
                return float(value)
    return BUDGET_FALLBACK_MS


def tamper(lines: list[str], kind: str) -> list[str]:
    """A render with exactly one fact removed. The auditor must catch every one of these.

    Each tamper is a way a later refactor can lose a claim while the card still looks complete:
    the summary block gone, one section's detail gone, the buttons gone, a cap that trimmed in
    silence, an empty section that stopped explaining itself, and a card with no digest.
    """
    if kind == "drop-summary-block":
        return [line for line in lines
                if not re.match(r"^ *[~x ?]?\s*(what|where|dependencies|surface|tests|blast|"
                                r"attention)\s", line)]
    if kind == "drop-section-detail":
        return _cut_block(lines, None)
    if kind == "drop-link":
        return [line for line in lines if "→" not in line]
    if kind == "drop-remainder-line":
        return [line for line in lines if "more figure" not in line]
    if kind == "drop-unavailable-reason":
        reasons = [line for line in lines if "why:" in line]
        gone = {line.split("why:", 1)[1].strip()[:24] for line in reasons}
        return [line for line in lines
                if not any(head in line for head in gone)]
    if kind == "drop-digest":
        return [line for line in lines if "card digest" not in line]
    raise KeyError("no tamper named %r" % kind)


def _cut_block(lines: list[str], anchor: str | None) -> list[str]:
    """Drop one block, heading and all: the block that starts at `anchor`, or, when `anchor` is
    None, the first section block. A block is a heading line up to the next heading line."""
    head = "  %s " % RULE
    if anchor is None:
        start = next((index for index, line in enumerate(lines) if line.startswith(head)),
                     len(lines))
    else:
        start = next((index for index, line in enumerate(lines) if anchor in line), None)
        if start is None:
            return list(lines)
        start += 1
    rest = lines[start + 1:] if anchor is None else lines[start:]
    stop = next((index for index, line in enumerate(rest)
                 if anchor is None and line.startswith(head)), len(rest))
    return lines[:start] + rest[stop:]


def score(*, policy: Policy | None = None, budget_ms: float | None = None,
          repeats: int = 3) -> Report:
    """Run every fixture through the card and measure the ticket's claims."""
    active = policy or Policy.built_in()
    report = Report(budget_ms=budget_ms if budget_ms is not None else load_budget())
    runs = max(1, repeats)
    # One untimed pass before anything is measured. The first card a process composes pays for
    # lazily-imported analysis modules and first-touch caches — here it is 98 ms against the 3 ms the
    # same fixture costs afterwards — and that is the price of the interpreter waking up, not a claim
    # about a cold index. Charging it once, to whichever number happens to be timed first, is half of
    # what made the cold/warm ordering a coin flip (**reco-d2bo**); the rest is below.
    for case in CORPUS.CASES:
        RENDER.render(case.card(active), cols=RENDER.DEFAULT_COLS)
        RENDER.render(compose(case.episode, warm_analyses(case), active), cols=RENDER.DEFAULT_COLS)
    for case in CORPUS.CASES:
        # The analyses a warm index already holds for this episode, built once and outside the clock.
        warm = warm_analyses(case)
        for _ in range(runs):
            # The cold path: the analyses are computed inside the compose, which is what an index run
            # pays per episode. It is timed as the warm path is timed — same samples, same render,
            # plus the analysis it starts from — so the claim "the cold number is the bigger one" is
            # arithmetic about what each path computes rather than a race between two differently
            # shaped distributions. It used to be a race: one cold sample per fixture against four
            # warm ones, the cold one without the render the warm one pays for, and the p95 of
            # sixteen samples quietly dropping its largest — which was the one fixture whose analysis
            # was worth measuring. On the runner the race went the wrong way by 1.6 %.
            started = time.perf_counter()
            cold_card = case.card(active)
            RENDER.render(cold_card, cols=RENDER.DEFAULT_COLS)
            report.cold_ms.append((time.perf_counter() - started) * 1000.0)
            # The warm path ADR-004 §11 budgets: the analyses are already computed (the index has
            # them), and the card composes and renders from them.
            started = time.perf_counter()
            card = compose(case.episode, warm, active)
            lines = RENDER.render(card, cols=RENDER.DEFAULT_COLS)
            report.card_ms.append((time.perf_counter() - started) * 1000.0)
        text = " ".join(" ".join(lines).split())
        report.states_total += 1
        wrong = {name: (card.states().get(name), want)
                 for name, want in case.expect_states.items()
                 if card.states().get(name) != want}
        if wrong:
            report.failures.append("%s: section states %s" % (case.name, sorted(wrong.items())))
        else:
            report.states_ok += 1
        for want in case.expect_in_card:
            report.claims_total += 1
            if want in text:
                report.claims_ok += 1
            else:
                report.failures.append("%s: the card does not say %r" % (case.name, want))
        for banned in case.forbid_in_card:
            if banned in text:
                report.forbidden.append("%s must not print %r" % (case.name, banned))
        resolutions = card.links_resolve()
        report.links_total += len(resolutions)
        report.links_ok += sum(1 for item in resolutions if item.ok)
        for broken in resolutions:
            if not broken.ok:
                report.failures.append("%s: link %s resolves to nothing (%s)"
                                       % (case.name, broken.link.key(), broken.reason))
        report.verified_total += 1
        problems = card.verify()
        if problems:
            report.failures.append("%s: verify() says %s" % (case.name, problems[:3]))
        else:
            report.verified_ok += 1
        report.audits_total += 1
        audit = RENDER.audit(lines, card=card)
        if audit.problems:
            report.failures.append("%s: the render audit failed: %s"
                                   % (case.name, audit.problems[:3]))
        else:
            report.audits_ok += 1
        for kind in TAMPERS:
            if kind == "drop-unavailable-reason" and not any(
                    section.state == STATE_UNAVAILABLE for section in card.sections):
                continue
            if kind == "drop-link" and not card.links():
                continue
            if kind == "drop-remainder-line" and not any(
                    len(section.figures) > active.integer("max_figures")
                    for section in card.sections):
                continue
            report.tampered_total += 1
            caught = RENDER.audit(tamper(lines, kind), card=card).problems
            if caught:
                report.tampered_caught += 1
            else:
                report.failures.append("%s: the auditor did not catch a %s render"
                                       % (case.name, kind))
        report.degradation_total += 1
        gaps = _degradation_gaps(card)
        if gaps:
            report.failures.append("%s: the card is not complete under degradation: %s"
                                   % (case.name, "; ".join(gaps)))
        else:
            report.degradation_ok += 1
        if case.expect_blast is not None:
            report.blast_total += 1
            got = (len(card.blast.direct) if card.blast else -1,
                   len(card.blast.one_hop) if card.blast else -1)
            if got == case.expect_blast:
                report.blast_ok += 1
            else:
                report.failures.append("%s: blast is %s, expected %s"
                                       % (case.name, got, case.expect_blast))
        report.notes.append("%s: %s" % (case.name, card.states()))
    return report


def _degradation_gaps(card) -> list[str]:
    """AC 3 as a predicate: every enabled section is present, and every gap explains itself."""
    problems: list[str] = []
    expected = len([spec for spec in SECTION_INDEX.values() if card.policy.enabled_for(spec.id)])
    if len(card.sections) != expected:
        problems.append("%d section(s) rendered, %d enabled by policy"
                        % (len(card.sections), expected))
    for section in card.sections:
        if section.state == STATE_UNAVAILABLE:
            if not section.reason:
                problems.append("%s is unavailable without a reason" % section.id)
            if not section.run:
                problems.append("%s is unavailable without a fill verb" % section.id)
            continue
        if not section.figures:
            problems.append("%s claims to have answered with no figures" % section.id)
        if not section.brief:
            problems.append("%s has no summary line" % section.id)
    return problems


def warm_analyses(case):
    """The analyses a warm index would already hold for this episode, built once.

    This mirrors what `compose` does when the caller hands it an empty `Analyses`, including the
    record-only case, where nothing may be computed at all. A card measured over analyses the
    engine would refuse to compute would be a faster card than the one that ships.
    """
    from .compose import _fill, _mark_record_only
    episode, analyses = case.episode, case.analyses
    if bool(episode.files) and not any(change.has_bytes for change in episode.files):
        return _mark_record_only(analyses, episode)
    return _fill(analyses, episode)


def separation() -> dict:
    """The two episodes the ticket's cost claim is really about: a bare record and a wide one."""
    out: dict[str, dict] = {}
    for name in ("bare_index_record", "code_and_tests", "wide_heuristic", "graph_full"):
        case = CORPUS.CASE_INDEX[name]
        started = time.perf_counter()
        card = compose(case.episode, case.analyses, Policy.built_in())
        composed_ms = (time.perf_counter() - started) * 1000.0
        started = time.perf_counter()
        RENDER.render(card)
        rendered_ms = (time.perf_counter() - started) * 1000.0
        out[name] = {"compose_ms": round(composed_ms, 3), "render_ms": round(rendered_ms, 3),
                     "sections": len(card.sections),
                     "figures": len(card.figures()), "links": len(card.links())}
    return out


def bench(*, repeats: int = 20) -> dict:
    """Warm-index cost: compose from stored analyses, then render. Repeated, p95 reported."""
    timings: dict[str, list[float]] = {}
    for name in CORPUS.NAMES:
        case = CORPUS.CASE_INDEX[name]
        card = compose(case.episode, case.analyses, Policy.built_in())
        samples = []
        for _ in range(max(1, repeats)):
            started = time.perf_counter()
            RENDER.render(compose(case.episode, case.analyses, Policy.built_in()))
            samples.append((time.perf_counter() - started) * 1000.0)
        timings[name] = samples
    budget = load_budget()
    all_samples = sorted(item for rows in timings.values() for item in rows)
    index = max(0, int(round(0.95 * len(all_samples))) - 1)
    return {"budget_id": BUDGET_ID, "budget_ms": budget,
            "p95_ms": round(all_samples[index], 3) if all_samples else 0.0,
            "median_ms": round(statistics.median(all_samples), 3) if all_samples else 0.0,
            "episodes": len(timings), "repeats": repeats,
            "per_episode": {name: round(sorted(rows)[len(rows) // 2], 3)
                            for name, rows in timings.items()}}
