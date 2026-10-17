"""The test surface delta (`e2.9` / `reco-6pl`): the testing contract per episode, not a coverage percentage.

One question per episode, answered with names and locations:

    from recommender.testsurface import Change, Episode, measure

    view = measure(Episode(id="ep-1", files=[Change(path=…, before=…, after=…)]))
    view.summary_line()
    # ep-1 · +34 test / +21 prod lines · ratio 0.618 · 4 tests added, 1 removed, 2 skipped
    #      · 1 assertions weakened · 5 findings · confidence 0.90 (parsed-cases)
    view.counts(); view.ratio(); view.proxy; view.findings

`[docs/concepts/test-surface.md](../../../docs/concepts/test-surface.md)` is the
specification; this package is its executable form. What the ticket demanded, and where each
part lives:

| demand | module |
|---|---|
| test lines vs production lines, ratio **and** absolute | `delta.py` (line attribution, incl. Rust `#[cfg(test)]` regions inside a `source` path) |
| test files created / renamed / deleted | `delta.py` (`counts()`, plus a file-level finding) |
| individual cases added or removed, per language | `parsers.py` (rust, python, go, ts/js, java), compared in `diff.py` |
| skipped and expected-failure cases | `parsers.py` (markers, decorators, runtime skips), `diff.py` (newly skipped vs un-skipped) |
| weakened assertions | `diff.py` (the strength/subject census comparison) |
| "new public logic, no new test" | `public.py` (what is public, what has logic) + `policy.py` (every threshold) + `delta.py` (coverage) |
| no framework present → line-count proxy with stated confidence | `delta.py`'s `Proxy`, whose three confidence numbers are config keys |

Modules, and the Rust home ADR-002 §9 gives them (team 5, `fathom-analyze`):

| module | Rust home | may do IO? |
|---|---|---|
| `scan.py`     | `fathom-analyze::testsurface::scan` | no — masking, brace blocks, subject keys |
| `parsers.py`  | `fathom-analyze::testsurface::parsers` | no — cases, skips, assertion census |
| `public.py`   | `fathom-analyze::testsurface::public` | no — public symbols and their logic |
| `diff.py`     | `fathom-analyze::testsurface::diff` | no — line ranges and case-level comparison |
| `policy.py`   | `fathom-analyze::testsurface::policy` | no — findings, knobs, layer resolution |
| `delta.py`    | `fathom-analyze::testsurface` | no — the episode run |
| `render.py`   | `fathom-cli::verbs::testsurface::render` | no — the card and its auditor |
| `explain.py`  | `fathom-cli::verbs::testsurface::explain` | no — prose |
| `corpus.py`   | `fathom-analyze::testsurface::corpus` (tests) | no — labelled fixtures |
| `eval.py`     | `fathom-cli::verbs::testsurface::score` | no — the gates |
| `cli.py`      | `fathom-cli::verbs::testsurface` | **yes** — reads blobs and `fathom.toml` |

`tests/test_test_surface_contract.py` greps the analysis half for `open(`, `os.environ` and
`subprocess` the way the taxonomy, change and noise contract suites grep their own, and the
Rust port must reproduce the blessed card text byte for byte.
"""

from __future__ import annotations

from .delta import (BASIS_LINES, BASIS_NO_TEST, BASIS_PARSED, BASIS_PARTIAL, SURFACE_VERSION,
                    Change, Episode, FileSurface, Proxy, SurfaceDelta, measure)
from .diff import CaseDiff, LineStats, Weakening, case_delta, line_stats
from .parsers import (CASES_VERSION, Assertion, Language, ParsedFile, TestCase, language_for,
                      parse)
from .policy import (ASSERTION_REMOVED, ASSERTION_WEAKENED, FINDING_IDS, KNOBS, RULES,
                     Finding, Policy, Rule, UnanchoredFinding)
from .public import PUBLIC_VERSION, Symbol, public_symbols
from .render import Audit, audit
from .render import render as render_card

__all__ = ["ASSERTION_REMOVED", "ASSERTION_WEAKENED", "Assertion", "Audit", "BASIS_LINES",
           "BASIS_NO_TEST", "BASIS_PARSED", "BASIS_PARTIAL", "CASES_VERSION", "CaseDiff",
           "Change", "Episode", "FINDING_IDS", "FileSurface", "Finding", "KNOBS", "Language",
           "LineStats", "PUBLIC_VERSION", "ParsedFile", "Policy", "Proxy", "Rule",
           "SURFACE_VERSION", "SurfaceDelta", "Symbol", "TestCase", "UnanchoredFinding",
           "Weakening", "aggregate", "audit", "case_delta", "language_for", "line_stats",
           "measure", "parse", "public_symbols", "render_card"]


def aggregate(episodes, *, policy: Policy | None = None) -> dict:
    """Roll several episodes up into one table — the shape `e2.10`'s card row wants.

    Kept here rather than in the CLI so the TUI, the export and the risk score all add up the
    same way: the ratio of a set is the set's total test lines over its total test + prod
    lines, never the mean of the per-episode ratios, which weights a two-line commit as much
    as a two-thousand-line one.
    """
    active = policy or Policy.built_in()
    totals = {"test_lines_added": 0, "test_lines_deleted": 0, "prod_lines_added": 0,
              "prod_lines_deleted": 0, "tests_added": 0, "tests_removed": 0,
              "tests_skipped": 0, "findings": 0, "episodes": 0}
    for episode in episodes:
        counts = measure(episode, active).counts()
        totals["episodes"] += 1
        for key in ("test_lines_added", "test_lines_deleted", "prod_lines_added",
                    "prod_lines_deleted", "tests_added", "tests_removed", "tests_skipped"):
            totals[key] += counts[key]
        totals["findings"] += sum(counts["findings"].values())
    total = totals["test_lines_added"] + totals["prod_lines_added"]
    totals["ratio"] = round(totals["test_lines_added"] / total, 3) if total else None
    return totals
