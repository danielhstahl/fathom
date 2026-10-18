"""The episode summary card (`e2.10` / `reco-4ey`): what, where, what it depends on, what surface, what is tested, and how far it reaches.

One call answers the ticket:

```python
from recommender.card import Episode, compose, render_card

card = compose(Episode.from_pairs("ep-7c1", {"src/cart.py": (before, after)}))
card.headline()        # 'what 3 file(s) · where 0 stories, churn 0.000 · deps … · blast …'
render_card(card)      # the one-screen block, then every figure with its evidence link
card.verify()          # [] when the card can prove every number it printed
card.digest()          # '9f2c…' — byte-identical for the same episode and policy
```

The plan's one-line definition is the specification, and every word of it is a module here:

> **Episode card (summary card)** — the one-screen answer for an episode: what changed
> semantically, where it landed, what it now depends on, which non-code surfaces it touched, what
> is tested, and a blast-radius estimate — every figure linking to its evidence, byte-deterministic
> for a given episode.

Two things make this different from the other epic-2 tickets. First, **almost nothing here is a
new measurement**: five of the seven sections relay an analysis that already exists and already
has its own corpus and gates (`e2.2` noise, `e2.3` structure, `e2.6` dependencies, `e2.8`
non-code, `e2.9` test surface). The card's job is composition, and the failure modes of
composition are specific: a figure that drifts from its source, a number with no evidence behind
it, and a missing analysis rendered as a zero. Each of those is refused by a type in this
package and asserted in `tests/test_summary_card_contract.py`. Second, **the one thing it does
measure is the blast radius** — direct dependents plus a one-hop estimate from a module graph
when the caller lends one, and from named path heuristics when it does not, with the reduced
confidence the plan demands printed rather than assumed (`blast.py`).

Modules, and the Rust home ADR-002 §9 gives each (teams 5 and 6):

| module | Rust home | may do IO? |
| --- | --- | --- |
| `model.py`   | `fathom-analyze::card::model` | no — the episode, the analyses the caller lends, line arithmetic |
| `policy.py`  | `fathom-analyze::card::policy` | no — section table, knobs, layer resolution, the refusal types |
| `blast.py`   | `fathom-analyze::blast` | no — the reach walk, graph or heuristics, with its partials |
| `links.py`   | `fathom-analyze::card::evidence` | no — the link model, the view registry, the resolver |
| `compose.py` | `fathom-analyze::card` | no — the seven builders, the card, `verify()` |
| `render.py`  | `fathom-tui::card::render` + `fathom-cli::verbs::card::render` | no — the card and its auditor |
| `explain.py` | `fathom-cli::verbs::card::explain` | no — prose |
| `corpus.py`  | `fathom-analyze::card::corpus` (tests) | no — fifteen fixtures with hand-written expectations |
| `eval.py`    | `fathom-cli::verbs::card::score` | no — the gates, and the cost against `query.card.ms` |
| `cli.py`     | `fathom-cli::verbs::card` | **yes** — reads two trees, a manifest, and `fathom.toml` |

`cli.py` is the only file that reads the world, so the same card comes out of the engine, the
CLI, the TUI's card pane and the Rust port (ADR-002 §4.5), and the blessed text in the contract
suite is the porting artefact.
"""

from __future__ import annotations

from .blast import Blast, Candidate, measure as measure_blast
from .compose import Attention, Card, Figure, Section, collect_findings, compose
from .corpus import Case, fixture
from .links import EvidenceMap, Link, Resolution, View, broken_links, walk
from .model import (Absent, Analyses, Change, DepsDelta, Episode, ModuleGraph, RiskScore,
                    line_stats)
from .policy import (BLAST_KNOBS, CARD_KNOBS, CONFIG_KEYS, KNOBS, SECTIONS, SECTION_IDS,
                     SECTION_INDEX, Policy)
from .render import Render, audit as audit_card, render as render_card

__all__ = [
    "Absent", "Analyses", "Attention", "BLAST_KNOBS", "Blast", "CARD_KNOBS", "Candidate",
    "CONFIG_KEYS", "Card", "Case", "Change", "DepsDelta", "Episode", "EvidenceMap", "Figure",
    "KNOBS", "Link", "ModuleGraph", "Policy", "Render", "Resolution", "RiskScore", "SECTIONS",
    "SECTION_IDS", "SECTION_INDEX", "Section", "View", "audit_card", "broken_links",
    "collect_findings", "compose", "fixture", "line_stats", "measure_blast", "render_card",
    "walk",
]
