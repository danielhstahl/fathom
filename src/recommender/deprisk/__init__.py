"""Who asked for this, and should I be worried? The dependency-risk answer.

`e2.6` answers *what moved* in the resolved dependency graph. A reviewer with that answer still has
two questions, and this package answers both:

* **who asked for this?** — which episode introduced the dependency, and what the agent was asked
  to do at the time;
* **should I be worried?** — is there a security advisory on it, and does its licence belong in the
  layer that now has to ship it?

```python
from recommender.deprisk import Policy, measure, parse_input, cache_for

view = measure(parse_input(payload), Policy.built_in(), cache=my_cache, now=1_760_000_000)
view.summary()["sentence"]     # 'ep-41 · 1 new direct dependency (jwt 2.1.0); intent recorded …'
view.verify()                  # [] when every printed number is traceable to a row
render(view)                   # the card, byte-deterministic
```

Read it in layers:

* `model.py` — what this ticket reads and nothing else: `Package`, `Change`, `Episode`, `Intent`,
  `Introduction`, `LicenceExpression`, `Finding`. No filesystem, no clock, no socket (ADR-002 §4.5).
* `policy.py` — the finding table, the licence matrix, every knob, and the three-gate consent rule.
* `licence.py` — licence classification, the layer question, and drift.
* `osv.py` — advisory records, the cache as a value, freshness, and what a refresh would fetch.
* `provenance.py` — the "who asked" ladder, and the exact shape of not having an answer.
* `delta.py` — the whole episode answer: rows, findings, counts, reconciliation, confidence.
* `render.py` — the deterministic card, plus the auditor that proves the card survived rendering.
* `explain.py` — the prose half: what the rules are, what the numbers in force are.
* `corpus.py` — the fixtures, with their numbers and OSV payloads written by hand.
* `eval.py` — scoring the corpus: the provenance, licence, advisory and reconciliation gates.
* `cli.py` — the only IO in the package: manifests, the cache file, config, and the one place a
  network request can ever be made.

`reco-yxi`.
"""

from .corpus import NOW, cache_for, fixture
from .delta import AdvisoryRow, RiskView, measure
from .licence import Row as LicenceRow
from .model import (
    Change, DependencyDelta, Episode, Finding, Input, Intent, Introduction, Package,
    parse_input)
from .osv import Advisory, Cache, Entry, Freshness, Lookup, Plan, deserialize, serialize
from .policy import CONFIG_KEYS, KNOBS, RULES, Policy, matrix_rows
from .provenance import Answer
from .render import audit
from .render import render as render_card

__all__ = [
    "Advisory", "AdvisoryRow", "Answer", "CONFIG_KEYS", "Cache", "Change", "DependencyDelta",
    "Episode", "Entry", "Finding", "Freshness", "Input", "Intent", "Introduction", "KNOBS",
    "LicenceRow", "Lookup", "NOW", "Package", "Plan", "Policy", "RULES", "RiskView",
    "audit", "cache_for", "deserialize", "fixture", "matrix_rows", "measure", "parse_input",
    "render_card", "serialize"]
