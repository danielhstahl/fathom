"""Noise suppression (`e2.2` / `reco-zqf`): give the reviewer their attention back.

One question per episode, one answer, with receipts:

    from recommender.noise import Change, Episode, suppress

    view = suppress(Episode.from_pairs("ep-9c1", {"src/a.rs": (before, after)}))
    view.headline()        # 'ep-9c1 · 1 file · 0 semantic changes · collapsed'
    view.counts()          # {'files': 1, 'semantic_lines': 0, 'by_rule': {'reformat': 1}, …}
    render.render(view)    # the collapsed line, its evidence, and the key that expands it

The ticket's own framing is the spec: *detect* whitespace/formatting-only changes with an
AST- or token-normalised comparison per language rather than a whitespace flag; detect
generated-file churn and lockfile-only edits; collapse an episode that is "nothing but
noise" to a single line with evidence. Then the rule that outranks all of it:

> **suppression is a view, never a deletion** — the raw content is always one keypress
> away and the policy that hid it is printed inline.

That rule is why every object here carries its reason. A fold is not a boolean in this
package; it is a :class:`~recommender.noise.policy.Suppression` that refuses to exist
without a reason, at least one piece of evidence and the key that expands it, and whose
only rendering method prints all three. `render.audit` re-checks the rendered text, so the
invariant is asserted at both ends: the object cannot be built without a reason, and the
text cannot pass the test that hides one.

Modules, and the Rust home ADR-002 §9 gives them (team 5, `fathom-analyze`):

| module | Rust home | may do IO? |
|---|---|---|
| `norm.py`       | `fathom-analyze::noise::norm` | no — language table, per-language token models, the line-level fallback |
| `reformat.py`  | `fathom-analyze::noise::reformat` | no — the reformat claim and its strength |
| `generated.py` | `fathom-analyze::noise::generated` | no — stamp / re-order / bulk regeneration, and the hand-edit refusal |
| `lockfile.py`  | `fathom-analyze::noise::lockfile` | no — twelve lock formats (thirteen filenames) → moved entries, manifest pairing |
| `policy.py`    | `fathom-analyze::noise::policy` | no — the rule table, layers, the session override, `Suppression` |
| `episode.py`   | `fathom-analyze::noise::episode` | no — per-file folds, the episode collapse |
| `render.py`    | `fathom-tui::noise::render` | no — the pane lines and the audit |
| `corpus.py`    | `fathom-analyze::noise::corpus` (tests) | no — hand-authored before/after pairs with expected answers |
| `eval.py`      | `fathom-cli::verbs::noise::score` | no — precision/recall, the CI gate |
| `explain.py`   | `fathom-cli::verbs::noise` | no — prose for one verdict or one policy |
| `cli.py`       | `fathom-cli::verbs::noise` | **yes** — reads the two revisions and `fathom.toml` |

Everything except `cli.py` is pure (ADR-002 §4.5): no clock, no filesystem, no network, no
store. Episode history arrives as a number the caller passes and is used as corroboration,
never as a gate. `tests/test_noise_suppression_contract.py` greps the analysis half for
file and process access the way `e2.1`'s and `e0.9`'s suites grep theirs.
"""

from __future__ import annotations

from .episode import Change, Episode, EpisodeNoise, FileNoise, suppress
from .generated import Generated, inspect as inspect_generated
from .lockfile import LockfileSummary, summarize as summarize_lockfile
from .norm import (LANGUAGES, NORM_VERSION, Normal, language_for, normalize, same_tokens)
from .policy import (FILE_RULES, GENERATED_CHURN, LOCKFILE, NOISE_ONLY, REFORMAT,
                     RULES, InvisibleSuppression, Policy, Rule, Session, Suppression)
from .reformat import (CONTENT, IDENTICAL, UNAVAILABLE, UNDETERMINED, Reformat, compare)

__all__ = ["CONTENT", "Change", "Episode", "EpisodeNoise", "FILE_RULES", "FileNoise",
           "GENERATED_CHURN", "LOCKFILE", "LANGUAGES", "LockfileSummary", "NOISE_ONLY",
           "NORM_VERSION", "Normal", "Policy", "REFORMAT", "Reformat", "Rule", "RULES",
           "Session", "Suppression", "UNAVAILABLE", "UNDETERMINED", "IDENTICAL",
           "InvisibleSuppression", "compare", "Generated", "inspect_generated",
           "language_for", "normalize", "same_tokens", "summarize_lockfile", "suppress"]

#: The package's own version, folded into every evidence digest.
NOISE_VERSION = NORM_VERSION
