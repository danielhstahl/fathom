"""The structure-churn score: one number for "the agent reorganised the tree".

The ticket's third acceptance criterion is a *distinction*, not a threshold: a 20-file change
inside one directory and a 20-file reshuffle across the tree must not score the same, and the
difference has to be visible in the number rather than in a reviewer reading the table. So
the score is a weighted mean of five components, each in `[0, 1]`, each printed with its
weight and its raw reading, and none of them is "lines changed":

| component | the reading | why it is here |
| --- | --- | --- |
| `moved-files` | files moved ÷ files touched | mobility: a file that changed folder moved without changing |
| `moved-lines` | lines that travelled ÷ (added + removed + travelled) | the line-shaped version of the same claim: a reorganisation is line-cheap and path-expensive |
| `directory-churn` | (directories added + removed) ÷ directories touched | the tree's own edit distance |
| `stories` | whole-directory stories ÷ `structure.churn.story_ceiling` | the four named shapes, saturated because a third one is not 50 % worse than a second |
| `shape-shift` | movement in depth and in top-`structure.metrics.top_dirs` code share | a tree can be reshuffled without any single move being big |

Every weight is a config key, so a repository that thinks line mobility matters more than
directory churn can say so and the card prints which layer won. The bands (`local` under
`structure.churn.band.local`, `structural` at or over `structure.churn.band.structural`,
`transitional` in between) are the numbers the corpus is measured against, and
`tests/test_folder_structure_contract.py` fails if a local change and a cross-tree reshuffle
of the same size ever come within the gap the document claims.

Two honesty rules. The score never exceeds 1 and never goes below 0, and an episode whose
caller supplied no line counts still gets a score — with the `moved-lines` component
reporting what it could count and the card saying so, because a score that silently drops an
unread component reads as "nothing happened" instead of "nobody looked".
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .policy import BAND_LOCAL, BAND_STRUCTURAL, BAND_TRANSITIONAL, Policy


@dataclass(frozen=True)
class Component:
    """One weighted part of the score, with the raw numbers it came from."""

    name: str
    reading: str          # the raw numbers, in words
    value: float          # normalised into [0, 1]
    weight: float
    blind: bool = False   # the caller could not measure this at all

    @property
    def contribution(self) -> float:
        return self.value * self.weight

    def as_dict(self) -> dict:
        return {"name": self.name, "reading": self.reading, "value": round(self.value, 4),
                "weight": self.weight, "contribution": round(self.contribution, 4),
                "blind": self.blind}


@dataclass
class Churn:
    """The score, its band, and every component that moved it."""

    score: float = 0.0
    band: str = BAND_LOCAL
    components: list[Component] = field(default_factory=list)
    statement: str = ""
    weights_total: float = 1.0
    band_local: float = 0.15
    band_structural: float = 0.35

    def as_dict(self) -> dict:
        return {"score": self.score, "band": self.band, "statement": self.statement,
                "weights_total": round(self.weights_total, 4),
                "bands": {"local_below": self.band_local, "structural_at_or_above":
                          self.band_structural},
                "components": [component.as_dict() for component in self.components]}

    def lines(self) -> list[str]:
        out = ["structure churn %.3f (%s)" % (self.score, self.band)]
        for component in self.components:
            out.append("  %-15s %.3f × %.2f = %.3f  %s"
                       % (component.name, component.value, component.weight,
                          component.contribution, component.reading
                          + (" [not measurable]" if component.blind else "")))
        out.append("  %s" % self.statement)
        out.append("  bands: under %s is `local`, at or over %s is `structural` "
                   "(`structure.churn.band.local`, `structure.churn.band.structural`)"
                   % (_num(self.band_local), _num(self.band_structural)))
        return out


def score(*, files_touched: int, files_moved: int, lines_added: int, lines_removed: int,
          lines_moved: int, directories_touched: int, directories_added: int,
          directories_removed: int, stories: int, mean_depth_delta: float,
          max_depth_delta: int, top_share_delta: float, policy: Policy) -> Churn:
    """The weighted score. Every input is a count this package measured; nothing is estimated."""
    weight_total = policy.weight_total
    moved_files = _ratio(files_moved, files_touched)
    line_total = lines_added + lines_removed + lines_moved
    moved_lines = _ratio(lines_moved, line_total)
    dir_churn = _ratio(directories_added + directories_removed, directories_touched)
    ceiling = max(1.0, float(policy.number("story_ceiling")))
    story_value = _ratio(stories, ceiling)
    depth_scale = max(1.0, float(policy.number("depth_scale")))
    shape_parts = [min(1.0, abs(mean_depth_delta) / depth_scale),
                   min(1.0, abs(max_depth_delta) / depth_scale),
                   min(1.0, abs(top_share_delta))]
    shape_value = sum(shape_parts) / len(shape_parts)
    components = [
        Component("moved-files", "%d/%d file(s) moved" % (files_moved, files_touched),
                  moved_files, float(policy.number("w_moved_files")) / weight_total),
        Component("moved-lines", "%d/%d line(s) travelled with a move"
                  % (lines_moved, line_total), moved_lines,
                  float(policy.number("w_moved_lines")) / weight_total,
                  blind=line_total == 0),
        Component("directory-churn", "%d added + %d removed over %d touched"
                  % (directories_added, directories_removed, directories_touched), dir_churn,
                  float(policy.number("w_directory_churn")) / weight_total),
        Component("stories", "%d %s (saturates at %s)"
                  % (stories, "story" if stories == 1 else "stories", _num(ceiling)),
                  story_value,
                  float(policy.number("w_stories")) / weight_total),
        Component("shape-shift", "mean depth %+s, max depth %+d, top share %+s"
                  % (_num(round(mean_depth_delta, 2)), max_depth_delta,
                     _num(round(top_share_delta, 2))), shape_value,
                  float(policy.number("w_shape_shift")) / weight_total),
    ]
    total = sum(component.contribution for component in components)
    value = round(max(0.0, min(1.0, total)), 3)
    band = _band(value, policy)
    return Churn(score=value, band=band, components=components, weights_total=weight_total,
                 statement=_statement(value, band, components),
                 band_local=float(policy.number("band_local")),
                 band_structural=float(policy.number("band_structural")))


def _band(value: float, policy: Policy) -> str:
    if value >= float(policy.number("band_structural")):
        return BAND_STRUCTURAL
    if value >= float(policy.number("band_local")):
        return BAND_TRANSITIONAL
    return BAND_LOCAL


def _ratio(part: float, whole: float) -> float:
    if not whole:
        return 0.0
    return max(0.0, min(1.0, part / whole))


def _statement(value: float, band: str, components: list[Component]) -> str:
    blind = [component.name for component in components if component.blind]
    if band == BAND_STRUCTURAL:
        head = ("%.3f is at or over the structural band: the tree itself moved, and most of "
                "the episode's files are in a different place than they were" % value)
    elif band == BAND_TRANSITIONAL:
        head = ("%.3f sits between the bands: some layout moved, most of the episode still "
                "reads as contents changing where they were" % value)
    else:
        head = ("%.3f is under the local band: contents changed where they were, the tree did "
                "not move" % value)
    if blind:
        head += " — %s could not be measured from what the caller supplied and counts as 0" \
                % ", ".join(blind)
    return head


def _num(value: object) -> str:
    if isinstance(value, float):
        text = ("%.2f" % value).rstrip("0").rstrip(".")
        return text or "0"
    return str(value)
