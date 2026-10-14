"""The noise axis: how much reviewable information a change to a path carries.

The level vocabulary is exactly the six values the store allows in
`git_commit_file.noise_class` (ADR-004 §4.3's `CHECK` constraint), and
`tests/test_taxonomy_contract.py` diffs this tuple against the DDL text so the two can
never disagree. That constraint is a gift: it forces the noise story to be said in six
words that the durable state already understands, instead of a scale nobody can query.

| level | meaning | who may claim it |
|---|---|---|
| `informative` | reviewable signal; the default | any rule, and every `!`-negation |
| `generated` | derived from something else in the tree (or from a generator) | path rules |
| `lockfile` | resolved dependency state | path rules |
| `vendored` | someone else's code | path rules |
| `reformat` | whitespace/format-only change | **content evidence only** — never a path rule |
| `unknown` | noise because a rule said so, with no named category: what a user's own `noise.globs` entry yields | path rules |

`reformat` is in the vocabulary but unreachable by a glob, and that is the point: "the
agent reindented 40 files" is a fact about the *diff*, not about the path, and `e2.2`
owns detecting it. A path rule that claimed `reformat` would be claiming something it
cannot know, so :func:`claimable_by_path_rule` refuses it and a test pins the refusal.

The `SCORE` column is a derived convenience, not a fact: *noise share* (thesis glossary)
is a mean over these weights, and `e2.2` may retune them. The level is what is stored and
explained; the number is what the risk vector consumes.
"""

from __future__ import annotations

from . import classes as C


class Noise:
    INFORMATIVE = "informative"
    GENERATED = "generated"
    LOCKFILE = "lockfile"
    REFORMAT = "reformat"
    VENDORED = "vendored"
    UNKNOWN = "unknown"


#: The six, in the order the store's CHECK lists them.
NOISE_LEVELS: tuple[str, ...] = (
    Noise.INFORMATIVE, Noise.GENERATED, Noise.LOCKFILE, Noise.REFORMAT,
    Noise.VENDORED, Noise.UNKNOWN,
)

#: Weight of one changed line, per level, for noise share. `informative` is 0.0 by
#: definition; the noisy levels are within 0.15 of each other on purpose, because the
#: honest claim is "this is noise" and not "this is 85 % of noise".
NOISE_SCORE: dict[str, float] = {
    Noise.INFORMATIVE: 0.0,
    Noise.GENERATED: 0.85,
    Noise.LOCKFILE: 0.80,
    Noise.REFORMAT: 0.90,
    Noise.VENDORED: 0.95,
    Noise.UNKNOWN: 0.75,
}

#: Levels no path rule may assert. See the module docstring.
CONTENT_ONLY_LEVELS: tuple[str, ...] = (Noise.REFORMAT,)


def is_known_level(level: str) -> bool:
    return level in NOISE_LEVELS


def claimable_by_path_rule(level: str) -> bool:
    return level in NOISE_LEVELS and level not in CONTENT_ONLY_LEVELS


def score_of(level: str) -> float:
    return NOISE_SCORE.get(level, NOISE_SCORE[Noise.UNKNOWN])


def is_noise(level: str) -> bool:
    return score_of(level) > 0.0


def noise_share(decisions, weights=None) -> float:
    """The share of a change set that carries no reviewable information, in ``0..1``.

    `decisions` is an iterable of :class:`~recommender.taxonomy.engine.Verdict` (or paths,
    which are classified with `taxonomy`); `weights` is a per-item mapping of changed
    lines (``n_add + n_del``), defaulting to one per item so a "files, not lines" caller
    still gets a sensible number.

    Line-weighting is the default because the number feeds a *review* decision: one
    regenerated 40 000-line file is 40 000 lines of noise, not one file of it. Empty
    input is ``0.0`` — no change is not noisy — and the result is clamped so a caller
    with weird weights cannot get a nonsense percentage.
    """
    total = 0.0
    noisy = 0.0
    for item in decisions:
        weight = 1.0
        if weights is not None:
            key = getattr(item, "path", item)
            try:
                weight = float(weights.get(key, 1.0))
            except AttributeError:                         # pragma: no cover
                raise TypeError("weights must be a mapping keyed by path") from None
        if weight <= 0:
            continue
        total += weight
        noisy += weight * score_of(getattr(item, "noise", item))
    if total <= 0:
        return 0.0
    return max(0.0, min(1.0, noisy / total))


def summary(levels_and_counts) -> dict[str, float]:
    """Per-level line totals as shares — the `fathom stats` noise breakdown (`e2.2`)."""
    totals: dict[str, float] = {}
    for level, count in levels_and_counts.items():
        totals[level] = totals.get(level, 0.0) + float(count)
    total = sum(totals.values())
    if not total:
        return {}
    return {level: round(count / total, 4) for level, count in sorted(totals.items())}


def level_for_class(cls: str) -> str:
    return C.default_noise(cls)
