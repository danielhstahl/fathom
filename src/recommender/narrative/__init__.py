"""Spike `s5` (`reco-6p8`): does an optional LLM polish beat the deterministic narrative template?

This package is a **spike**, not a library. ADR-001 §8 non-goal 6 refuses a mandatory LLM call, and
ADR-002 §4.5 gives `fathom-analyze` a `net-llm` feature for "optional narrative polish (`e4.7`,
spike `s5`)" — a seam somebody drew before anybody measured. The thesis (§non-goal 6, row 6) names
the one condition that would justify it:

    Only if `s5` shows a measured, non-trivial win on trust and usefulness *with* hallucination
    counts at zero against the evidence set.

This is that measurement. Three arms compose a narrative for the same 20 real episodes out of the
same evidence set (the `fathom-delta/1` document `fathom analyze` prints):

* **A — template**: the deterministic composer (`template.py`). Every sentence cites the evidence
  item it renders, and it can only cite, so it cannot say anything the delta document does not.
* **B — polish**: an LLM rewrites A with the same citation index in front of it (`spike/s5/arms/`),
  which is exactly what the `net-llm` path would do.
* **C — why-line only**: the LLM is allowed one sentence — the *why* — and only from the commit
  subjects. A narrower seam than the one the ticket asks about, included because the interesting
  question is not "LLM or not" but "how much rope".

The measured parts are `audit.py` (claims are pulled back out of each narrative and checked against
the evidence set, which is where "hallucinated claims not supported by the evidence set" stops being
an opinion), `panel.py` (three blinded reviewer passes, with the arm identity sealed until the
scores are in), and `spike.py`, which writes `benchmarks/reports/s5-narrative-polish.json` and can
re-check it (`--check`) the way `s1` and `s2` can.

The written answer is [docs/concepts/narrative-polish.md](../../docs/concepts/narrative-polish.md),
and the decision lands as a beads decision issue linked from `reco-2rx` (`e4.7`).

Two honesty notes kept next to the code, because both bound what the numbers mean:

* the LLM arms were produced by the model running the spike session, with no provider SDK or API key
  reachable from the spike sandbox, so **cost and latency for arms B and C are modelled from
  measured token counts, not invoiced or wall-clocked** (`arms.py` records the provenance per file);
* the panel is three passes of the same base model under three rubrics, so it measures a *floor* on
  reviewer disagreement and cannot measure human disagreement at all. The mechanical audit is the
  load-bearing number; the panel is the soft one, and `spike.py` says so in the report.
"""

from __future__ import annotations
