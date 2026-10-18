"""Where did this episode land? The folder-structure answer.

`e2.1` answers *what kind of file* an episode touched, and `e2.4` answers *which file became
which file*. Neither answers the question a reviewer actually asks about a large episode:
**did the agent move code around, and where did it end up?** A change that touches 20 files
inside one module and a change that reshuffles 20 files across the repository produce the same
counts in both of those answers, and they are not the same kind of change.

This package takes an episode's change list — plus the two file inventories the caller can
lend, when it can — and returns the directory-level answer: a per-directory table that sums
exactly to the episode totals, the structural stories the raw pairings add up to (package
renamed, subtree moved, module split, directory flattened), structural metrics for the whole
tree before and after, a bounded structure-churn score, and one deterministic sentence that
says where the episode landed.

```python
from recommender.structure import Episode, measure

view = measure(Episode.from_tree_pair("ep-1", before, after))
view.summary()["sentence"]        # 'ep-1 · `pkg/pay` was renamed to `pkg/billing` (6 file(s), …'
view.verify()                     # [] when the table reconciles to the episode totals
view.churn.score, view.churn.band # 0.800, 'structural'
```

Read it in layers:

* `model.py` — what this ticket reads and nothing else: `Episode`, `Change`, `Move`, `LineStats`,
  and the path arithmetic every other module shares. Nothing here is allowed to know the
  filesystem, the clock or the environment (ADR-002 §4.5), so the same episode means the same
  thing in the engine, the CLI, and the Rust port.
* `policy.py` — the shape table, the finding rules, and every knob, with the layer that won it.
* `tree.py` — the file inventories, the structural metrics measured off them, and the basis
  question: is this the whole tree, or only what this episode touched?
* `moves.py` — pairings in, shapes out. Pairings come from the change list, from the caller
  (typically `e2.4`), or from a labelled guess; this module groups them and refuses to invent any.
* `churn.py` — the structure-churn score and its components.
* `delta.py` — the whole episode answer: facts, directory rows, reconciliation, findings,
  confidence, and the one-sentence summary.
* `corpus.py` — the fixtures, with their numbers written by hand.
* `eval.py` — scoring the corpus: the shape gates, the sentence gates, and the reconciliation
  property over generated episodes.
* `render.py` — the deterministic card, plus the auditor that proves the card survived rendering.
* `explain.py` — the prose half.
* `cli.py` — the only IO in the package.

`reco-5fn`.
"""

from .churn import Churn, Component, score as score_churn
from .corpus import fixture
from .delta import DirRow, FileFact, StructureDelta, aggregate, measure
from .model import (Change, Episode, LineStats, Move, directory_of, leaf, line_stats,
                    normalise, similarity, split_lines)
from .moves import Pair, Story, find_stories, pair_episode
from .policy import (CONFIG_KEYS, FINDING_IDS, KNOBS, NAMED_SHAPES, RULES, SHAPES, Finding,
                     Policy, Rule, shape_rules)
from .render import audit as audit_card, render as render_card
from .tree import DirCensus, FileEntry, Metrics, Tree, build_trees, delta_metrics

__all__ = [
    "CONFIG_KEYS", "Change", "Churn", "Component", "DirCensus", "DirRow", "Episode",
    "FINDING_IDS", "FileEntry", "FileFact", "KNOBS", "LineStats", "Metrics", "Move", "NAMED_SHAPES",
    "Pair", "Policy", "RULES", "Rule", "SHAPES", "Story", "StructureDelta", "Tree",
    "aggregate", "audit_card", "build_trees", "delta_metrics", "directory_of", "find_stories",
    "fixture", "leaf", "line_stats", "measure", "normalise", "pair_episode", "render_card",
    "score_churn", "shape_rules", "similarity", "split_lines",
]
