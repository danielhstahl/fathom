"""What the card prints, what it may leave out, and every number that decides a blast radius.

Three tables live here, because the ticket's argument is that a card is a *policy* and not a
layout: which questions it answers, what it is allowed to say when the answer is missing, and
how far the blast-radius walk goes before it declares a partial.

**The section table.** Seven sections, in the order the ticket lists them, each owned by the
analysis that feeds it:

| id | question | whose answer |
| --- | --- | --- |
| `what` | what changed, after noise suppression | `e2.2` |
| `where` | where it landed | `e2.3` |
| `dependencies` | what the tree now depends on | `e2.6` |
| `surface` | which non-code surfaces it touches | `e2.8` |
| `tests` | what is tested | `e2.9` |
| `blast` | how far it reaches | `e2.10` (this package, `blast.py`) |
| `attention` | why it is in front of you | `e2.10`, from the findings the others raised |

Every section key is a switch, and switching one off is a *stated* omission: the card names the
section it was told to drop and the key that dropped it, and `render.audit` fails a render that
drops one silently. That is the difference between a repository narrowing its own card and a
card quietly losing a question.

**The blast rules.** Blast radius is the only section this package computes, and the two bases
are not interchangeable: a module graph answers with edges and answers at
`blast.confidence.graph` (0.90), path heuristics answer with named rules and answer at
`blast.confidence.heuristic` (0.40). Every heuristic weight is a knob, every counted candidate
names the rule that counted it, and a candidate below `blast.heuristic.min_score` is not
counted at all — a reach estimate that counted "it's in the same repository" would always be
wide.

**The knob table.** Row caps for the card, the blast walk depth (`blast.hops`, whose overflow is
a stated partial and not a hang), the heuristic weights, the two confidence numbers, and the
cost guard that stops a heuristics walk over a 40 000-file tree from being the reason the card
missed its 50 ms. Declared in `e0.8`'s schema, so they layer (flags > env > repo `fathom.toml`
> user config > built-in), validate, and print the layer that won.

```
card.sections.what                  true            print the what-changed section
card.max_figures                    6               figure rows per section before the remainder is named
card.max_findings                   8               attention rows before the remainder is named
card.max_evidence                   3               evidence lines printed per figure
card.attention.severity             notable         lowest severity that earns an attention row
blast.hops                          1               rings beyond the direct one; beyond it is a partial
blast.confidence.graph              0.90            confidence when the module graph answered
blast.confidence.heuristic          0.40            confidence when path heuristics answered
blast.confidence.narrow_census      0.70            multiplier when the caller lent no file inventory
blast.heuristic.same_directory      0.60            weight: a dependent in the changed file's own directory
blast.heuristic.test_pair           0.50            weight: the test file that covers a changed source file
blast.heuristic.sibling_directory   0.30            weight: the changed file's parent directory's own files
blast.heuristic.package_prefix      0.15            weight: everything under the changed file's top-level package
blast.heuristic.min_score           0.30            below this a heuristic candidate is not counted
blast.max_rows                      10              blast rows per ring before the remainder is named
blast.max_candidates                5000            candidates examined before the walk stops and says so
```
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace

# --- sections --------------------------------------------------------------
SECTION_WHAT = "what"
SECTION_WHERE = "where"
SECTION_DEPENDENCIES = "dependencies"
SECTION_SURFACE = "surface"
SECTION_TESTS = "tests"
SECTION_BLAST = "blast"
SECTION_ATTENTION = "attention"

STATE_MEASURED = "measured"
STATE_PARTIAL = "partial"
STATE_UNAVAILABLE = "unavailable"
STATES: tuple[str, ...] = (STATE_MEASURED, STATE_PARTIAL, STATE_UNAVAILABLE)

#: Severities, best first. These are the sub-analyses' own spellings — `e2.8` and `e2.3` both
#: use `high`/`notable`/`note`, and the card does not invent a fourth.
SEVERITY_HIGH = "high"
SEVERITY_NOTABLE = "notable"
SEVERITY_NOTE = "note"
SEVERITY_ORDER: tuple[str, ...] = (SEVERITY_HIGH, SEVERITY_NOTABLE, SEVERITY_NOTE)


@dataclass(frozen=True)
class SectionSpec:
    """One row of the section table: the question, who answers it, and the switch that hides it."""

    id: str
    title: str
    question: str
    owner: str                      # the ticket whose analysis answers it
    config_key: str
    default: bool = True
    doc: str = ""

    @property
    def short(self) -> str:
        return self.title


SECTIONS: tuple[SectionSpec, ...] = (
    SectionSpec(SECTION_WHAT, "what changed", "What changed, once the noise is folded away?",
                "e2.2 (noise suppression) + e2.1 (path classes)", "card.sections.what", True,
                "Semantic counts after `e2.2` folds reformat, generated and lockfile churn: "
                "files, changed lines, semantic lines, the noise share, and the path-class mix. "
                "Without this the card opens with a raw line count, which is the number the "
                "noise ticket exists to stop the product printing."),
    SectionSpec(SECTION_WHERE, "where it landed", "Where did the code land, and did the tree move?",
                "e2.3 (folder structure delta)", "card.sections.where", True,
                "Directories touched, lines and files that moved, the structural story if one "
                "fired, and the structure-churn score with its band — plus the census basis, "
                "because '14 files in this folder' means two different things."),
    SectionSpec(SECTION_DEPENDENCIES, "what it depends on",
                "What does the tree now depend on that it did not?",
                "e2.6 (dependency delta)", "card.sections.dependencies", True,
                "The dependency-delta headline: direct adds and removals, upgrades and "
                "downgrades, transitive-only movement, source and licence switches. Supplied by "
                "the caller, because the delta is computed in `fathom-analyze::deps`."),
    SectionSpec(SECTION_SURFACE, "what surface it touches",
                "Which non-code surfaces moved, and who has to do something?",
                "e2.8 (non-code surfaces)", "card.sections.surface", True,
                "env, flags, migrations, CI, IaC and container findings, and the operator-action "
                "count — the part of the review that happens outside the diff."),
    SectionSpec(SECTION_TESTS, "what is tested", "What is tested after this change?",
                "e2.9 (test surface delta)", "card.sections.tests", True,
                "Test versus production lines with the ratio's absolutes and its denominator "
                "named, cases added / removed / skipped / weakened, and the coverage findings. "
                "Never a coverage percentage."),
    SectionSpec(SECTION_BLAST, "what it reaches", "How far does this change reach?",
                "e2.10 (this package: `blast.py`)", "card.sections.blast", True,
                "Direct dependents and a one-hop estimate, from the module graph when the "
                "caller lends one and from named path heuristics when it does not, always with "
                "the basis and the confidence printed beside the count."),
    SectionSpec(SECTION_ATTENTION, "why it is here", "Why is this episode in front of a human?",
                "e2.10, from the findings the other sections raised", "card.sections.attention",
                True,
                "The findings that carry weight, in severity order, each with the evidence row "
                "that put it there. A supplied risk score is used when the caller has one; "
                "without it the card says the ranking is `e4.4`'s instead of pretending."),
)

SECTION_INDEX: dict[str, SectionSpec] = {spec.id: spec for spec in SECTIONS}
SECTION_IDS: tuple[str, ...] = tuple(spec.id for spec in SECTIONS)


# --- blast bases -----------------------------------------------------------
BASIS_GRAPH = "module graph"
BASIS_HEURISTIC = "path heuristics"
BASIS_NONE = "no basis available"

#: Why a blast answer is narrower than it looks. Each of these is a printed row, not an
#: internal state: a partial that is not printed is a wrong answer with extra steps.
PARTIAL_UNMAPPED = "changed paths the graph has no node for"
PARTIAL_HOPS = "dependents beyond the configured hop limit"
PARTIAL_CENSUS = "no repository inventory supplied, only the episode's own paths"
PARTIAL_CANDIDATES = "candidates the walk stopped at, over the cost guard"


@dataclass(frozen=True)
class Knob:
    """One number or switch that decides an answer."""

    name: str
    key: str
    kind: str                       # integer | number | boolean | string
    default: object
    doc: str
    units: str = ""
    analyzer: str = "card.compose"

    @property
    def short_doc(self) -> str:
        return self.doc.split(". ")[0] + "."


KNOBS: tuple[Knob, ...] = (
    # --- the card itself ---------------------------------------------------
    Knob("max_figures", "card.max_figures", "integer", 6,
         "Figure rows printed per section. The remainder is counted and named on its own line, "
         "because a card that silently drops a question is worse than a long one.",
         units="rows"),
    Knob("max_findings", "card.max_findings", "integer", 8,
         "Attention rows before the remainder is counted rather than cut. Ordering is severity "
         "then source section then name, so the same eight rows come out twice.", units="rows"),
    Knob("max_evidence", "card.max_evidence", "integer", 3,
         "Evidence lines printed per figure. Everything behind a link is in `--json` and in the "
         "view the link names, whether or not the card had room for it.", units="lines"),
    Knob("attention_severity", "card.attention.severity", "string", "notable",
         "Lowest severity that earns an attention row: `high`, `notable` or `note`. Lowering it "
         "to `note` is how a reviewer sees everything the analyses raised; raising it to `high` "
         "is how a triage card stays short. Findings below it are still counted.",
         units="severity"),
    Knob("risk_tau", "risk.tau", "integer", 40,
         "The queue's surfacing threshold, owned by `e4.4`. The card does not score risk and does "
         "not rank the queue; it reads this one number so a relayed risk score can say whether "
         "the episode would be surfaced at all.",
         units="score", analyzer="card.compose"),
    Knob("state_digits", "card.state.digits", "integer", 3,
         "Decimal places on every ratio, share and confidence printed by the card. Three, "
         "because two hides the difference between 0.615 and 0.618 that a reviewer may want to "
         "argue about, and four is noise.", units="digits"),
    # --- blast radius ------------------------------------------------------
    Knob("hops", "blast.hops", "integer", 1,
         "Rings counted *beyond* the direct one. The direct ring is always counted — that is "
         "what `direct` means — so the default of 1 answers the ticket's 'direct + one-hop'. "
         "Modules further out than this are a stated partial, never a silent stop.",
         units="hops", analyzer="blast_radius"),
    Knob("confidence_graph", "blast.confidence.graph", "number", 0.9,
         "Confidence attached to a blast answer that came from a module graph. Not 1.0: a graph "
         "is built by an extractor, and `s1` measured declarations a program wrote that no "
         "extractor can enumerate.", units="0-1", analyzer="blast_radius"),
    Knob("confidence_heuristic", "blast.confidence.heuristic", "number", 0.4,
         "Confidence attached to a blast answer built from path heuristics. Lower-bounded by "
         "the ticket's own framing: a path guess is a guess, and the plan asks for the reduced "
         "confidence to be stated, not for it to be flattering.", units="0-1",
         analyzer="blast_radius"),
    Knob("confidence_narrow_census", "blast.confidence.narrow_census", "number", 0.7,
         "Multiplier applied when the caller lent no file inventory, so the only paths the "
         "heuristics could see were the episode's own. The reach cannot be wider than the world "
         "it was allowed to look at, and the card says so.", units="multiplier",
         analyzer="blast_radius"),
    Knob("w_same_directory", "blast.heuristic.same_directory", "number", 0.6,
         "Heuristic weight: a file in the changed file's own directory. The strongest path "
         "signal there is — same folder, so same imports, in every layout that has folders.",
         units="score", analyzer="blast_radius"),
    Knob("w_test_pair", "blast.heuristic.test_pair", "number", 0.5,
         "Heuristic weight: the test file that covers a changed source file (and, the other "
         "way round, the source a changed test covers). Named before the sibling rule so a "
         "test pair is reported as a test pair.", units="score", analyzer="blast_radius"),
    Knob("w_sibling_directory", "blast.heuristic.sibling_directory", "number", 0.3,
         "Heuristic weight: the files in the changed file's parent directory. Weaker than "
         "same-directory on purpose — a parent's own files are neighbours, not dependents.",
         units="score", analyzer="blast_radius"),
    Knob("w_package_prefix", "blast.heuristic.package_prefix", "number", 0.15,
         "Heuristic weight: everything under the changed file's top-level package. Cheap and "
         "wide, which is why it sits under `blast.heuristic.min_score` and only counts when "
         "nothing stronger fired.", units="score", analyzer="blast_radius"),
    Knob("min_score", "blast.heuristic.min_score", "number", 0.3,
         "Heuristic score a candidate must reach to be counted as reached. The floor is what "
         "stops 'shares a repository' being a blast radius.", units="score",
         analyzer="blast_radius"),
    Knob("max_rows", "blast.max_rows", "integer", 10,
         "Blast rows printed per ring before the remainder is counted and named.", units="rows",
         analyzer="blast_radius"),
    Knob("max_candidates", "blast.max_candidates", "integer", 5000,
         "Candidate paths examined before the heuristic walk stops. Stopping is a stated "
         "partial naming how many it did not look at, so the cost guard is visible in the "
         "answer instead of being a silent truncation.", units="candidates",
         analyzer="blast_radius"),
)

KNOB_INDEX: dict[str, Knob] = {knob.name: knob for knob in KNOBS}
BLAST_KNOBS: tuple[Knob, ...] = tuple(knob for knob in KNOBS if knob.key.startswith("blast."))
CARD_KNOBS: tuple[Knob, ...] = tuple(knob for knob in KNOBS if knob.key.startswith("card."))

#: Every key this package reads. The cache digest is taken over exactly this list, so a card
#: cannot be cached behind a knob it obeyed (ADR-004 §5.5).
CONFIG_KEYS: tuple[str, ...] = (tuple(knob.key for knob in KNOBS)
                                + tuple(spec.config_key for spec in SECTIONS))

#: The card's own failure modes, each raised rather than softened.
class UnlinkedFigure(ValueError):
    """A figure with no evidence link: refused, not printed. 'Every number is a button'."""


class UnanchoredSection(ValueError):
    """A section with no state, no lead, or no owner: the card would be a layout, not an answer."""


class UnresolvableLink(ValueError):
    """A link whose evidence view is empty: a button that goes nowhere is worse than none."""


class UnnamedOmission(ValueError):
    """A dropped section that the render did not name."""


def severity_rank(severity: str) -> int:
    """Lower is heavier. An unknown severity sorts after `note`, never before it."""
    try:
        return SEVERITY_ORDER.index(severity)
    except ValueError:
        return len(SEVERITY_ORDER)


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
        return cls(enabled={spec.id: spec.default for spec in SECTIONS},
                   knobs={knob.name: _coerce(knob, knob.default) for knob in KNOBS})

    @classmethod
    def from_config(cls, config=None) -> "Policy":
        policy = cls(enabled={}, source={}, knobs={}, knob_source={})
        for spec in SECTIONS:
            value, source = _lookup(config, spec.config_key, spec.default)
            policy.enabled[spec.id] = spec.default if value is None else bool(value)
            policy.source[spec.id] = source
        for knob in KNOBS:
            value, source = _lookup(config, knob.key, knob.default)
            policy.knobs[knob.name] = knob.default if value is None else _coerce(knob, value)
            policy.knob_source[knob.name] = source
        if config is not None:
            # ADR-004 §5.5: the digest names everything the payload read, including `blast.hops`
            # — a card cached over a hop-limit change is a card about a different episode.
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

    def with_section(self, section_id: str, enabled: bool,
                     source: str = "caller override") -> "Policy":
        if section_id not in SECTION_INDEX:
            raise KeyError("no card section %r" % section_id)
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source))
        clone.enabled[section_id] = bool(enabled)
        clone.source[section_id] = source
        return clone

    def all_enabled(self) -> "Policy":
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source))
        clone.enabled = {spec.id: True for spec in SECTIONS}
        return clone

    # -- reads --------------------------------------------------------------
    def effective(self, section_id: str) -> tuple[bool, str]:
        return self.enabled.get(section_id,
                                SECTION_INDEX[section_id].default), \
            self.source.get(section_id, "built-in default")

    def enabled_for(self, section_id: str) -> bool:
        return self.effective(section_id)[0]

    def hidden_sections(self) -> list[tuple[str, str]]:
        """Sections the policy switched off, with the layer that switched it off."""
        return [(spec.id, self.effective(spec.id)[1]) for spec in SECTIONS
                if not self.enabled_for(spec.id)]

    def knob(self, name: str) -> object:
        return self.knobs.get(name, KNOB_INDEX[name].default)

    def number(self, name: str) -> float:
        value = self.knob(name)
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            return KNOB_INDEX[name].default
        return float(value)

    def integer(self, name: str) -> int:
        return int(self.number(name))

    def text(self, name: str) -> str:
        return str(self.knob(name))

    def attn_floor(self) -> int:
        """The severity rank an attention row must beat."""
        return severity_rank(self.text("attention_severity"))

    def state(self, value: float) -> str:
        """One number, printed the same way everywhere on this card."""
        return "%.*f" % (max(0, self.integer("state_digits")), value)

    # -- the text -----------------------------------------------------------
    def describe(self) -> list[str]:
        out: list[str] = ["card policy (highest precedence first)"]
        out.append("  sections")
        for spec in SECTIONS:
            enabled, source = self.effective(spec.id)
            out.append("  %-13s %-5s %-30s %-9s %s"
                       % (spec.id, "on" if enabled else "off", spec.config_key, spec.owner,
                          source))
        out.append("  definition")
        for knob in KNOBS:
            out.append("  %-26s %-10s %-38s %s"
                       % (knob.name, _clip(_format(self.knob(knob.name)), 10), knob.key,
                          self.knob_source.get(knob.name, "built-in default")))
        return out

    def definition_lines(self) -> list[str]:
        return ["%s = %s  (%s)" % (knob.key, _format(self.knob(knob.name)), knob.doc)
                for knob in KNOBS]

    def as_dict(self) -> dict:
        return {"sections": {spec.id: {"enabled": self.enabled_for(spec.id),
                                       "config_key": spec.config_key,
                                       "owner": spec.owner,
                                       "source": self.source.get(spec.id, "built-in default")}
                             for spec in SECTIONS},
                "knobs": {knob.name: {"value": _jsonable(self.knob(knob.name)), "key": knob.key,
                                      "default": _jsonable(knob.default), "units": knob.units,
                                      "analyzer": knob.analyzer,
                                      "source": self.knob_source.get(knob.name,
                                                                     "built-in default")}
                          for knob in KNOBS},
                "config_digest": self.config_digest}


def describe_sections() -> list[str]:
    return ["%-13s %-30s %s" % (spec.id, spec.owner, spec.question) for spec in SECTIONS]


def section_docs() -> list[str]:
    return ["%-13s %s" % (spec.id, spec.doc) for spec in SECTIONS]


def blast_definition(policy: Policy | None = None) -> list[str]:
    """The blast-radius procedure with the numbers currently in force."""
    policy = policy or Policy.built_in()
    return [
        "%-22s the changed files' own dependents; always counted, graph or heuristic"
        % "direct",
        "%-22s dependents of the direct ring, up to blast.hops = %s rings out; further than "
        "that is a stated partial" % ("one hop", policy.integer("hops")),
        "%-22s edges from `e3.2`'s graph, confidence %s"
        % (BASIS_GRAPH, policy.state(policy.number("confidence_graph"))),
        "%-22s rules, each named on the row it won: same-directory %s, test pair %s, "
        "sibling-directory %s, package-prefix %s; a candidate below %s is not counted"
        % (BASIS_HEURISTIC, policy.state(policy.number("w_same_directory")),
           policy.state(policy.number("w_test_pair")),
           policy.state(policy.number("w_sibling_directory")),
           policy.state(policy.number("w_package_prefix")),
           policy.state(policy.number("min_score"))),
        "%-22s heuristics answer at %s, and that is multiplied by %s when the caller lent no "
        "file inventory" % ("confidence", policy.state(policy.number("confidence_heuristic")),
                            policy.state(policy.number("confidence_narrow_census"))),
    ]


def _jsonable(value: object) -> object:
    return list(value) if isinstance(value, (list, tuple)) else value


def _clip(text: str, width: int) -> str:
    return text if len(text) <= width else text[: max(1, width - 1)] + "…"


def _format(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        return ("%.3f" % value).rstrip("0").rstrip(".")
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
