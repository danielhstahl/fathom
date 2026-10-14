"""The path taxonomy and noise policy engine (`e0.9` / `reco-5vz`): one place that decides
what a path **is** and how noisy it is, so no classifier or detector ever carries its own
glob list.

This package is the Python contract form of `fathom-analyze`'s `src/taxonomy/` and
`src/noise/` modules (ADR-002 §9, team 5 `analyze-change`). The split matters as much as
the code:

| module | Rust home | may do IO? |
|---|---|---|
| `classes.py` | `fathom-analyze::taxonomy::class` | **no** — the class vocabulary |
| `noise.py` | `fathom-analyze::noise::level` | no — the noise vocabulary and noise share |
| `rules.py` | `fathom-analyze::taxonomy::rule` | no — rules, specificity, the lookup index |
| `builtins.py` | `fathom-analyze::taxonomy::builtins` | no — the built-in table, versioned |
| `engine.py` | `fathom-analyze::taxonomy` | no — resolves a path against a table |
| `corpus.py` | `fathom-analyze::taxonomy::corpus` (tests) | no — the decision-table corpus |
| `explain.py` | `fathom-cli::verbs::explain` | no — renders a verdict |
| `cli.py` | `fathom-cli::verbs::explain` | **yes** — it is the only module that loads `fathom.toml` |

ADR-002 §4.5's rule — *"no crate below `fathom-cli` reads configuration, environment
variables or files to find out how it should behave"* — is kept literally: the engine takes
a resolved `Config` as a value. `tests/test_taxonomy_contract.py` greps the analysis half
for `open(`, `os.environ` and `subprocess` the way the config contract suite greps
`schema.py`.

Public surface:

    from recommender.taxonomy import Taxonomy, default_taxonomy

    tax = Taxonomy.from_config(config)          # or default_taxonomy() for built-ins only
    v = tax.classify("dist/app.min.js")
    v.cls                                       # "generated"
    v.noise, v.noise_score                      # "generated", 0.85
    v.rule_id, v.layer                          # "gen.dir", "defaults"
    [m.rule_id for m in v.runners_up()]         # everything it beat
    tax.noise_share(paths, weights={"a": 12})  # the glossary's *noise share*
"""

from __future__ import annotations

from .builtins import BUILTINS_VERSION, BUILTIN_RULES, BUILTIN_SOURCE
from .classes import (ASSERTABLE_CLASSES, CLASS_INFO, PATH_CLASSES, PathClass,
                      assertable, default_noise, is_diffable, is_known_class,
                      is_reviewable)
from .engine import (FALLBACK, Match, Taxonomy, Verdict, default_taxonomy,
                     reset_default_taxonomy)
from .noise import (NOISE_LEVELS, NOISE_SCORE, CONTENT_ONLY_LEVELS, Noise,
                    claimable_by_path_rule, is_noise, noise_share, score_of)
from .rules import Rule, RuleIndex, specificity, table_digest

__all__ = ["ASSERTABLE_CLASSES", "BUILTINS_VERSION", "BUILTIN_RULES", "BUILTIN_SOURCE",
           "CLASS_INFO", "CONTENT_ONLY_LEVELS", "FALLBACK", "Match", "NOISE_LEVELS",
           "NOISE_SCORE", "PATH_CLASSES", "Noise", "PathClass", "Rule", "RuleIndex",
           "Taxonomy", "Verdict", "assertable", "claimable_by_path_rule", "default_noise",
           "default_taxonomy", "is_diffable", "is_known_class", "is_noise", "is_reviewable",
           "noise_share", "reset_default_taxonomy", "score_of", "specificity",
           "table_digest"]
