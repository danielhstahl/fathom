"""The findings this ticket can raise, the shape vocabulary, and every number that decides one.

Three tables live here, and the ticket's argument is that they have to be in one file.

**The shape vocabulary.** "The agent reorganised the tree" is four different claims, and the
directory table alone cannot tell them apart — every one of them moves files between
directories. So a *story* is a named shape with a rule:

| shape | the rule that names it | fixture |
| --- | --- | --- |
| `package-rename` | a whole directory's files move to one destination, relative paths preserved, same parent, different name | `package_rename` |
| `subtree-move` | a whole directory's files (with their inner folders) move to one destination under a different parent, relative paths preserved | `subtree_move` |
| `module-split` | one directory's files land in ≥ `structure.split.min_destinations` destinations, and the source is emptied | `module_split` |
| `directory-flatten` | a directory's files move into one of its ancestors and the directory disappears | `directory_flatten` |
| `files-moved` | files moved between directories without any of the above shapes (partial move, renamed on the way, source not emptied) | `partial_move` |

A shape is reported with the numbers that triggered it and is never reported on a hunch
below a threshold: `structure.move.min_files` and `structure.move.min_share` are what stop
"two files happened to change folders" from being announced as a reorganisation.

**The finding table.** Eleven kinds, each with its own switch, severity and detector:

| id | key | severity | fires when |
| --- | --- | --- | --- |
| `subtree-moved` | `structure.findings.subtree_moved` | high | a `subtree-move` story fired |
| `churn-high` | `structure.findings.churn_high` | high | the structure-churn score reached `structure.churn.band.structural` |
| `package-renamed` | `structure.findings.package_renamed` | notable | a `package-rename` story fired |
| `module-split` | `structure.findings.module_split` | notable | a `module-split` story fired |
| `directory-flattened` | `structure.findings.directory_flattened` | notable | a `directory-flatten` story fired |
| `directory-removed` | `structure.findings.directory_removed` | notable | a directory that held files holds none, with its file count |
| `nesting-deepened` | `structure.findings.nesting_deepened` | note | mean directory depth rose by ≥ `structure.depth.mean_delta`, or max depth by ≥ `structure.depth.max_delta` |
| `concentration-up` | `structure.findings.concentration_up` | note | the top-`structure.metrics.top_dirs` share of code rose by ≥ `structure.concentration.delta` |
| `files-moved` | `structure.findings.files_moved` | note | a `files-moved` story fired |
| `directory-added` | `structure.findings.directory_added` | note | a directory appeared that holds files |
| `move-unconfirmed` | `structure.findings.move_unconfirmed` | note | some pairing behind this answer was a guess, not a fact — which half of the answer that makes is named |

Like `e2.9` and `e2.8` before it, a finding here **refuses to exist without an anchor and
without evidence** (:class:`UnanchoredFinding`). A structure finding is a claim about named
directories, so `structure-subtree-moved` without a `path` and with no `evidence` is not a
weaker finding, it is a summary of one — and the ticket asks for evidence lists, not
summaries.

**The knob table.** Every threshold above, every weight in the churn score, and the card's
row caps, declared in `e0.8`'s schema so they layer (flags > env > repo `fathom.toml` >
user config > built-in), validate, and print the layer that won:

```
structure.code_classes                    source, test    which path classes are "code" for the concentration metric
structure.metrics.top_dirs                3               the top-K in "share of code in the top-K directories"
structure.move.min_files                  3               pairings that may make a whole-directory story
structure.move.min_share                  0.60            share of the source directory that had to move
structure.move.require_emptied            true            the source must be left with no files for a whole-directory story
structure.move.infer_content              true            may this package infer pairings from bytes when the caller supplied none
structure.move.min_similarity             0.50            line-set Jaccard required to infer one
structure.move.fact_bar                   0.75            at or above this a caller's pairing is treated as a fact
structure.split.min_destinations          2               destinations a split needs
structure.split.min_files_per_destination 1              files a destination needs to count as one arm of a split
structure.flatten.min_levels              1               levels a move must climb to count as flattening
structure.churn.weight.moved_files        0.25            weight: share of touched files that moved
structure.churn.weight.moved_lines        0.25            weight: share of touched lines that travelled
structure.churn.weight.directory_churn    0.20            weight: directories added + removed over directories touched
structure.churn.weight.stories            0.20            weight: structural stories, saturated at the ceiling
structure.churn.weight.shape_shift        0.10            weight: depth and concentration movement
structure.churn.story_ceiling             2               stories at which that component saturates
structure.churn.depth_scale               4               mean-depth change at which that component saturates
structure.churn.band.local                0.15            under this the episode is `local`
structure.churn.band.structural           0.35            at or over this the episode is `structural`
structure.depth.mean_delta                0.5             mean depth delta that is worth a row
structure.depth.max_delta                 2               max depth delta that is worth a row
structure.concentration.delta             0.15            top-K code-share delta that is worth a row
structure.card.max_dirs                   12              directory rows per card
structure.card.max_stories                6               story rows per card
structure.card.max_evidence               4               evidence lines per finding
```

The two bands are the numbers AC 3 turns on, so they are stated and measured rather than
guessed: `tests/test_folder_structure_contract.py` re-derives the separation on the corpus
and fails if a 20-file local change ever scores within `structure.churn.band.structural` of
a 20-file cross-tree reshuffle.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field, replace

# --- shapes ----------------------------------------------------------------
SHAPE_PACKAGE_RENAME = "package-rename"
SHAPE_SUBTREE_MOVE = "subtree-move"
SHAPE_MODULE_SPLIT = "module-split"
SHAPE_FLATTEN = "directory-flatten"
SHAPE_FILES_MOVED = "files-moved"
SHAPES: tuple[str, ...] = (SHAPE_PACKAGE_RENAME, SHAPE_SUBTREE_MOVE, SHAPE_MODULE_SPLIT,
                           SHAPE_FLATTEN, SHAPE_FILES_MOVED)

#: The four shapes the ticket names as fixtures, in the order its acceptance criteria list
#: them. A shape with no fixture is a shape the contract suite cannot prove.
NAMED_SHAPES: tuple[str, ...] = (SHAPE_MODULE_SPLIT, SHAPE_FLATTEN, SHAPE_PACKAGE_RENAME,
                                 SHAPE_SUBTREE_MOVE)

# --- finding ids -----------------------------------------------------------
SUBTREE_MOVED = "subtree-moved"
PACKAGE_RENAMED = "package-renamed"
MODULE_SPLIT = "module-split"
DIRECTORY_FLATTENED = "directory-flattened"
FILES_MOVED = "files-moved"
DIRECTORY_ADDED = "directory-added"
DIRECTORY_REMOVED = "directory-removed"
CHURN_HIGH = "churn-high"
CONCENTRATION_UP = "concentration-up"
NESTING_DEEPENED = "nesting-deepened"
MOVE_UNCONFIRMED = "move-unconfirmed"

SEVERITY_HIGH = "high"
SEVERITY_NOTABLE = "notable"
SEVERITY_NOTE = "note"
SEVERITY_ORDER: tuple[str, ...] = (SEVERITY_HIGH, SEVERITY_NOTABLE, SEVERITY_NOTE)

BASIS_INVENTORY = "full-tree inventory"
BASIS_EPISODE = "episode file list only"

#: Where an episode-wide claim is anchored. A finding must name a place, and the honest place
#: for "the tree got deeper" is the repository root, spelled so a viewer can open it.
ROOT_ANCHOR = "./"


def anchor(path: str) -> str:
    """A finding's anchor for a directory: the root spells as `./`, never as the empty string."""
    text = str(path or "").replace("\\\\", "/").strip().rstrip("/")
    while text.startswith("./"):
        text = text[2:]
    return text or ROOT_ANCHOR


@dataclass(frozen=True)
class Rule:
    """One row of the finding table."""

    id: str
    title: str
    config_key: str
    default: bool
    severity: str
    doc: str
    detector: str
    unit: str = "directory"

    @property
    def phrase(self) -> str:
        return self.title.lower()


RULES: tuple[Rule, ...] = (
    Rule(SUBTREE_MOVED, "Subtree moved", "structure.findings.subtree_moved", True,
         SEVERITY_HIGH,
         "A whole directory — files and inner folders — now lives under a different parent. "
         "Nothing inside it changed; the reviewable content of the episode is the move "
         "itself.", detector="moves.stories", unit="directory"),
    Rule(CHURN_HIGH, "Tree reorganised", "structure.findings.churn_high", True, SEVERITY_HIGH,
         "The structure-churn score reached `structure.churn.band.structural`. This is the "
         "'the agent reorganised the tree' row, and it is a number the card prints with its "
         "components beside it, not a vibe.", detector="churn.score", unit="episode"),
    Rule(PACKAGE_RENAMED, "Package renamed", "structure.findings.package_renamed", True,
         SEVERITY_NOTABLE,
         "A directory changed name in place with its contents intact, so every import of it "
         "moved and none of its logic did.", detector="moves.stories", unit="directory"),
    Rule(MODULE_SPLIT, "Module split", "structure.findings.module_split", True,
         SEVERITY_NOTABLE,
         "One directory's files now live in two or more destinations. A split is where "
         "responsibility moved, which is the reviewable question, and the destinations are "
         "named.", detector="moves.stories", unit="directory"),
    Rule(DIRECTORY_FLATTENED, "Directory flattened", "structure.findings.directory_flattened",
         True, SEVERITY_NOTABLE,
         "A directory moved its files up into an ancestor and vanished, so the tree is one "
         "level shallower where it used to be.", detector="moves.stories", unit="directory"),
    Rule(DIRECTORY_REMOVED, "Directory removed", "structure.findings.directory_removed", True,
         SEVERITY_NOTABLE,
         "A directory that held files holds none. Reported even when its files were moved, "
         "because a deleted directory is also a deleted namespace and a stale import.",
         detector="tree.novelty", unit="directory"),
    Rule(NESTING_DEEPENED, "Nesting deepened", "structure.findings.nesting_deepened", True,
         SEVERITY_NOTE,
         "Average (or maximum) directory depth grew by the configured delta. Deep trees are "
         "how a review starts hiding things from itself.", detector="metrics.shape",
         unit="episode"),
    Rule(CONCENTRATION_UP, "Code concentrated", "structure.findings.concentration_up", True,
         SEVERITY_NOTE,
         "The share of code living in the top-N directories grew by the configured delta — "
         "the tree is getting one big folder.", detector="metrics.shape", unit="episode"),
    Rule(FILES_MOVED, "Files moved", "structure.findings.files_moved", True, SEVERITY_NOTE,
         "Files changed directory without forming one of the four named shapes: a partial "
         "move, a move with a rename on the way, or a move out of a directory that still has "
         "files in it.", detector="moves.stories", unit="file"),
    Rule(DIRECTORY_ADDED, "Directory added", "structure.findings.directory_added", True,
         SEVERITY_NOTE,
         "A new directory now holds files. Loud enough to notice, quiet enough not to compete "
         "with the move that explains it.", detector="tree.novelty", unit="directory"),
    Rule(MOVE_UNCONFIRMED, "Move not confirmed", "structure.findings.move_unconfirmed", True,
         SEVERITY_NOTE,
         "Some pairing behind this answer was inferred from bytes or handed over below "
         "`structure.move.fact_bar` rather than asserted by git, so the directory rows built "
         "on it inherit that uncertainty and are listed.", detector="moves.tiers",
         unit="pairing"),
)

RULE_INDEX: dict[str, Rule] = {rule.id: rule for rule in RULES}
FINDING_IDS: tuple[str, ...] = tuple(rule.id for rule in RULES)
_BY_KEY_TAIL = {rule.config_key.rsplit(".", 1)[-1]: rule for rule in RULES}


@dataclass(frozen=True)
class Knob:
    """One number, glob list or switch that decides an answer."""

    name: str
    key: str
    kind: str                       # integer | number | boolean | string_list
    default: object
    doc: str
    units: str = ""


KNOBS: tuple[Knob, ...] = (
    Knob("code_classes", "structure.code_classes", "string_list", ["source", "test"],
         "Which path classes count as code when measuring where the code lives. `docs`, "
         "`config` and `generated` are out by default: a folder full of markdown moving is "
         "real, but it is not the same concentration claim as 4 000 lines of source.",
         units="path class"),
    Knob("top_dirs", "structure.metrics.top_dirs", "integer", 3,
         "The N in 'share of code in the top-N directories'. Three because the ticket names "
         "three, and because a top-1 share measures one folder while a top-10 share measures "
         "everything.", units="directories"),
    Knob("move_min_files", "structure.move.min_files", "integer", 3,
         "Pairings that may support a whole-directory story. Below this, two files changing "
         "folder is a `files-moved` row, not a reorganisation.", units="files"),
    Knob("move_min_share", "structure.move.min_share", "number", 0.6,
         "Share of the source directory's files that must have moved for the directory to be "
         "called moved. 1.0 would require a perfect sweep and lose every move with one straggler "
         "file; 0 would call any relocation a move.", units="share"),
    Knob("move_require_emptied", "structure.move.require_emptied", "boolean", True,
         "A whole-directory story additionally requires that the source holds no files after "
         "the episode.", units=""),
    Knob("infer_moves", "structure.move.infer_content", "boolean", True,
         "When the caller supplies no pairings, infer them from bytes (same basename, line-set "
         "Jaccard at or above `structure.move.min_similarity`). Turning this off makes the "
         "answer strictly narrower: no inferred moves, and the card says so.", units=""),
    Knob("move_min_similarity", "structure.move.min_similarity", "number", 0.5,
         "Line-set Jaccard required to infer a pairing, computed exactly (no sketches). Only "
         "used to accept or refuse a pairing, never printed as a similarity verdict.",
         units="0-1"),
    Knob("move_fact_bar", "structure.move.fact_bar", "number", 0.75,
         "At or above this confidence a caller-supplied pairing counts as a fact. Deliberately "
         "the same number as `rename.fact_bar`: s4 measured 0.75 as git's honest bar, and a "
         "structure card that trusted pairings git would not is a card that lies.",
         units="0-1"),
    Knob("split_min_destinations", "structure.split.min_destinations", "integer", 2,
         "Destinations a split needs. One destination is a move, not a split.", units="dirs"),
    Knob("split_min_files_per_destination", "structure.split.min_files_per_destination",
         "integer", 2,
         "Files a destination needs to count as one arm of a split. One is the natural floor "
         "and the wrong one: a directory of 17 files with one stray in a new folder is a move "
         "of 16 plus a `files-moved` row, not a split.", units="files"),
    Knob("flatten_min_levels", "structure.flatten.min_levels", "integer", 1,
         "How many directory levels a move must climb for the shape to be flattening rather "
         "than an ordinary move sideways.", units="levels"),
    Knob("w_moved_files", "structure.churn.weight.moved_files", "number", 0.25,
         "Churn weight: share of touched files that moved.", units="weight"),
    Knob("w_moved_lines", "structure.churn.weight.moved_lines", "number", 0.25,
         "Churn weight: share of touched lines that travelled with a move.", units="weight"),
    Knob("w_directory_churn", "structure.churn.weight.directory_churn", "number", 0.2,
         "Churn weight: directories added and removed over directories touched.",
         units="weight"),
    Knob("w_stories", "structure.churn.weight.stories", "number", 0.2,
         "Churn weight: structural stories found, saturated at `structure.churn.story_ceiling`.",
         units="weight"),
    Knob("w_shape_shift", "structure.churn.weight.shape_shift", "number", 0.1,
         "Churn weight: how much the tree's shape metrics (depth, top-N concentration) moved.",
         units="weight"),
    Knob("story_ceiling", "structure.churn.story_ceiling", "number", 2,
         "Stories at which the story component saturates. A third story is not 50 % worse than "
         "a second; it is the same story about a third folder.", units="stories"),
    Knob("depth_scale", "structure.churn.depth_scale", "number", 4,
         "Mean-depth change at which the shape component saturates.", units="levels"),
    Knob("band_local", "structure.churn.band.local", "number", 0.15,
         "Below this churn score the episode is `local`: it changed contents where they were.",
         units="score"),
    Knob("band_structural", "structure.churn.band.structural", "number", 0.35,
         "At or above this the episode is `structural`: the tree itself moved. Between the two "
         "is `transitional`, and the card says so rather than rounding to a verdict.",
         units="score"),
    Knob("depth_mean_delta", "structure.depth.mean_delta", "number", 0.5,
         "Mean directory-depth growth that is worth a `nesting-deepened` row.", units="levels"),
    Knob("depth_max_delta", "structure.depth.max_delta", "integer", 2,
         "Maximum-depth growth that is worth the same row on its own.", units="levels"),
    Knob("concentration_delta", "structure.concentration.delta", "number", 0.15,
         "Growth in the top-N code share that is worth a `concentration-up` row.",
         units="share"),
    Knob("max_dirs", "structure.card.max_dirs", "integer", 12,
         "Directory rows per card. The remainder is counted and named, never cut in silence.",
         units="rows"),
    Knob("max_stories", "structure.card.max_stories", "integer", 6,
         "Story rows per card before the remainder is counted.", units="rows"),
    Knob("max_evidence", "structure.card.max_evidence", "integer", 4,
         "Evidence lines printed per finding; the rest is counted, and all of it is in "
         "`--json` and in `findings`.", units="lines"),
)

KNOB_INDEX: dict[str, Knob] = {knob.name: knob for knob in KNOBS}
CONFIG_KEYS: tuple[str, ...] = tuple(knob.key for knob in KNOBS) + tuple(
    rule.config_key for rule in RULES)

#: The card's bands, spelled for a human rather than a switch statement.
BAND_LOCAL = "local"
BAND_TRANSITIONAL = "transitional"
BAND_STRUCTURAL = "structural"


class UnanchoredFinding(ValueError):
    """A finding with no anchor or no evidence: refused, not softened."""


@dataclass
class Finding:
    """One structural finding: a named directory (or pairing), its anchor path, and receipts."""

    kind: str
    name: str
    path: str                                   # the directory or file the claim is anchored on
    detail: str = ""
    old_path: str = ""
    line: int = 0
    confidence: float = 1.0
    tier: str = ""                              # fact | guess | "" when the claim is not a pairing
    shape: str = ""
    evidence: list[str] = field(default_factory=list)
    numbers: dict[str, object] = field(default_factory=dict)

    def __post_init__(self) -> None:
        if self.kind not in RULE_INDEX:
            raise UnanchoredFinding("unknown structure finding kind %r" % self.kind)
        if not self.name:
            raise UnanchoredFinding("%s: a structure finding must name the directory it is "
                                    "about" % self.kind)
        if not self.path:
            raise UnanchoredFinding(
                "%s %s: a structure finding with no directory is a summary, not a finding"
                % (self.kind, self.name))
        if not 0.0 < self.confidence <= 1.0:
            raise UnanchoredFinding("%s %s: confidence must be in (0, 1]"
                                    % (self.kind, self.name))
        if not self.evidence:
            raise UnanchoredFinding("%s %s: a structure finding must carry the rows that "
                                    "support it" % (self.kind, self.name))
        if not self.detail:
            self.detail = self.rule.doc.split(". ")[0] + "."

    @property
    def rule(self) -> Rule:
        return RULE_INDEX[self.kind]

    @property
    def severity(self) -> str:
        return self.rule.severity

    @property
    def location(self) -> str:
        return "%s:%d" % (self.path, self.line) if self.line else self.path

    def render(self) -> str:
        parts = ["%s: %s" % (self.rule.title, self.name)]
        if self.detail:
            parts.append(self.detail)
        parts.append("at %s" % self.location)
        if self.tier:
            parts.append("[%s]" % self.tier)
        if self.confidence < 0.999:
            parts.append("(confidence %.2f)" % self.confidence)
        return " · ".join(parts)

    def block(self, indent: int = 4, max_evidence: int | None = None) -> list[str]:
        pad = " " * indent
        cap = max_evidence if max_evidence is not None else len(self.evidence)
        lines = [pad + self.render()]
        for item in self.evidence[:cap]:
            lines.append(pad + "  · %s" % item)
        hidden = len(self.evidence) - cap
        if hidden > 0:
            lines.append(pad + "  · … %d more line(s) of evidence: "
                         "fathom structure findings --all" % hidden)
        return lines

    def as_dict(self) -> dict:
        return {"kind": self.kind, "title": self.rule.title, "severity": self.severity,
                "name": self.name, "path": self.path, "old_path": self.old_path,
                "line": self.line, "location": self.location, "detail": self.detail,
                "shape": self.shape, "tier": self.tier,
                "confidence": round(self.confidence, 3),
                "config_key": self.rule.config_key, "detector": self.rule.detector,
                "evidence": list(self.evidence), "numbers": dict(self.numbers)}


def sort_key(finding: Finding) -> tuple:
    return (SEVERITY_ORDER.index(finding.severity), finding.kind, finding.path,
            finding.old_path, finding.name, finding.line)


sort_findings = sort_key


@dataclass
class Policy:
    """The effective switches and knobs for one repository, each remembering its winning layer."""

    enabled: dict[str, bool] = field(default_factory=dict)
    source: dict[str, str] = field(default_factory=dict)
    knobs: dict[str, object] = field(default_factory=dict)
    knob_source: dict[str, str] = field(default_factory=dict)
    config_digest: str = ""

    @classmethod
    def built_in(cls) -> "Policy":
        return cls(enabled={rule.id: rule.default for rule in RULES},
                   knobs={knob.name: _coerce(knob, knob.default) for knob in KNOBS})

    @classmethod
    def from_config(cls, config=None) -> "Policy":
        policy = cls(enabled={}, source={}, knobs={}, knob_source={})
        for rule in RULES:
            value, source = _lookup(config, rule.config_key, rule.default)
            policy.enabled[rule.id] = rule.default if value is None else bool(value)
            policy.source[rule.id] = source
        for knob in KNOBS:
            value, source = _lookup(config, knob.key, knob.default)
            policy.knobs[knob.name] = knob.default if value is None else _coerce(knob, value)
            policy.knob_source[knob.name] = source
        if config is not None:
            # ADR-004 §5.5: the digest names everything the payload read. The store manifest's
            # own list for `structure.delta` predates this analyzer having knobs of its own, so
            # the digest is taken over CONFIG_KEYS — the same call `e2.8` documents.
            policy.config_digest = config.digest(list(CONFIG_KEYS))
        return policy

    # -- caller overrides ---------------------------------------------------
    def with_knobs(self, source: str = "caller override", **names) -> "Policy":
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source))
        for name, value in names.items():
            knob = KNOB_INDEX[name]
            clone.knobs[name] = _coerce(knob, value)
            clone.knob_source[name] = "%s (%s=%s)" % (source, name, _format(value))
        return clone

    def with_flag(self, rule_id: str, enabled: bool,
                  source: str = "caller override") -> "Policy":
        rule = RULE_INDEX.get(rule_id) or _BY_KEY_TAIL.get(rule_id)
        if rule is None:
            raise KeyError("no structure finding kind %r" % rule_id)
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source))
        clone.enabled[rule.id] = bool(enabled)
        clone.source[rule.id] = source
        return clone

    def all_enabled(self) -> "Policy":
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source))
        clone.enabled = {rule.id: True for rule in RULES}
        return clone

    # -- reads --------------------------------------------------------------
    def effective(self, rule_id: str) -> tuple[bool, str]:
        return self.enabled.get(rule_id, RULE_INDEX[rule_id].default), \
            self.source.get(rule_id, "built-in default")

    def enabled_for(self, rule_id: str) -> bool:
        return self.effective(rule_id)[0]

    def knob(self, name: str) -> object:
        return self.knobs.get(name, KNOB_INDEX[name].default)

    def number(self, name: str) -> float:
        value = self.knob(name)
        return value if isinstance(value, (int, float)) and not isinstance(value, bool) \
            else KNOB_INDEX[name].default

    def integer(self, name: str) -> int:
        return int(self.number(name))

    def flag(self, name: str) -> bool:
        return bool(self.knob(name))

    def words(self, name: str) -> list[str]:
        value = self.knob(name)
        if isinstance(value, str):
            return [item.strip() for item in value.split(",") if item.strip()]
        return [str(item) for item in (value or [])]

    @property
    def weight_total(self) -> float:
        total = sum(float(self.number(name)) for name in
                    ("w_moved_files", "w_moved_lines", "w_directory_churn", "w_stories",
                     "w_shape_shift"))
        return total if total > 0 else 1.0

    # -- the text -----------------------------------------------------------
    def describe(self) -> list[str]:
        out: list[str] = ["structure policy (highest precedence first)"]
        out.append("  findings")
        for rule in RULES:
            enabled, source = self.effective(rule.id)
            out.append("  %-22s %-5s %-44s %-9s %s"
                       % (rule.id, "on" if enabled else "off", rule.config_key,
                          rule.severity, source))
        out.append("  definition")
        for knob in KNOBS:
            out.append("  %-32s %-18s %-44s %s"
                       % (knob.name, _clip(_format(self.knob(knob.name)), 18), knob.key,
                          self.knob_source.get(knob.name, "built-in default")))
        return out

    def definition_lines(self) -> list[str]:
        return ["%s = %s  (%s)" % (knob.key, _format(self.knob(knob.name)), knob.doc)
                for knob in KNOBS]

    def as_dict(self) -> dict:
        return {"findings": {rule.id: {"enabled": self.enabled_for(rule.id),
                                       "source": self.source.get(rule.id, "built-in default"),
                                       "config_key": rule.config_key,
                                       "severity": rule.severity,
                                       "detector": rule.detector}
                             for rule in RULES},
                "knobs": {knob.name: {"value": _jsonable(self.knob(knob.name)), "key": knob.key,
                                      "default": _jsonable(knob.default), "units": knob.units,
                                      "source": self.knob_source.get(knob.name,
                                                                     "built-in default")}
                          for knob in KNOBS},
                "config_digest": self.config_digest}


def describe_rules() -> list[str]:
    return ["%-22s %-8s %-44s %s" % (rule.id, rule.severity, rule.config_key, rule.doc)
            for rule in RULES]


def shape_rules() -> list[str]:
    """The shape vocabulary with the numbers currently in force — printed by `shapes`."""
    return [
        "%-17s a directory's files move to one destination with their relative paths kept, "
        "same parent, new name — ≥ %s file(s), ≥ %s of the directory"
        % (SHAPE_PACKAGE_RENAME, KNOB_INDEX["move_min_files"].default,
           KNOB_INDEX["move_min_share"].default),
        "%-17s the same, under a different parent (a whole package relocated)"
        % SHAPE_SUBTREE_MOVE,
        "%-17s one directory's files land in ≥ %s destinations, each with ≥ %s file(s), "
        "source emptied"
        % (SHAPE_MODULE_SPLIT, KNOB_INDEX["split_min_destinations"].default,
           KNOB_INDEX["split_min_files_per_destination"].default),
        "%-17s a directory's files move up ≥ %s level(s) into an ancestor and it disappears"
        % (SHAPE_FLATTEN, KNOB_INDEX["flatten_min_levels"].default),
        "%-17s anything that moved and matches none of the above" % SHAPE_FILES_MOVED,
    ]


def _jsonable(value: object) -> object:
    return list(value) if isinstance(value, (list, tuple)) else value


def _clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[: max(1, width - 1)] + "…"


def _format(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        return ("%.2f" % value).rstrip("0").rstrip(".")
    if isinstance(value, (list, tuple)):
        return ", ".join(str(item) for item in value) or "—"
    return str(value)


def _coerce(knob: Knob, value: object) -> object:
    if knob.kind == "integer":
        try:
            return int(value)
        except (TypeError, ValueError):
            return knob.default
    if knob.kind == "number":
        try:
            return float(value)
        except (TypeError, ValueError):
            return knob.default
    if knob.kind == "boolean":
        if isinstance(value, bool):
            return value
        return str(value).strip().lower() in ("1", "true", "yes", "on")
    if knob.kind == "string_list":
        if isinstance(value, str):
            return [item.strip() for item in value.split(",") if item.strip()]
        return [str(item) for item in (value or [])]
    return value


def _lookup(config, key: str, default):
    """(value, source) for one key, from a resolved `Config` or a bare mapping."""
    if config is None:
        return default, "built-in default"
    decision = None
    getter = getattr(config, "decision", None)
    if callable(getter):
        decision = getter(key)
    if decision is not None:
        value = getattr(decision, "value", None)
        if value is None:
            value = default
        return value, _layer_label(getattr(decision, "winner", "defaults"),
                                   getattr(decision, "source", ""),
                                   getattr(decision, "from_defaults", False))
    values = getattr(config, "values", config)
    cursor: object = values
    for segment in key.split("."):
        if not isinstance(cursor, dict) or segment not in cursor:
            return default, "built-in default"
        cursor = cursor[segment]
    return cursor, "config values (unresolved)"


def _layer_label(layer: str, source: str, from_defaults: bool) -> str:
    if from_defaults or layer in ("defaults", "default", ""):
        return "built-in default"
    return "%s layer%s" % (layer, " (%s)" % source if source else "")
