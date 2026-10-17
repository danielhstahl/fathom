"""The findings this ticket can raise, and every number that decides whether one fires.

Two tables live here, and the distinction is the point.

**The finding table.** Seven kinds of finding, each with a config key, a default and a
severity. `Deleting or skipping an existing test is a first-class finding` is AC 2, and
"first-class" has to mean something stronger than "in the same list as the rest": each
finding carries the test or symbol **name** and a **location**, and
:class:`Finding` refuses to exist without them (:class:`UnanchoredFinding`). A card row that
said "a test was removed somewhere" is not a finding, it is a rumour.

| id | key | severity | fires when |
| --- | --- | --- | --- |
| `test-removed` | `test.findings.test_removed` | high | a named test case present in the old revision is gone from the new one, file deleted or case deleted |
| `test-file-removed` | `test.findings.test_file_removed` | high | a whole test file was deleted (the file-level row beside its case-level rows) |
| `test-skipped` | `test.findings.test_skipped` | high | a case that used to run is now `#[ignore]`/`@pytest.mark.skip`/`t.Skip`/`it.skip`/`@Disabled`, or the whole file's suite was disabled |
| `assertion-weakened` | `test.findings.assertion_weakened` | high | an `exact` assertion about a subject was replaced by a `presence` one about the same subject |
| `assertion-removed` | `test.findings.assertion_removed` | notable | a case kept running but lost exact assertions with nothing added in their place |
| `untested-public` | `test.findings.untested_public` | notable | new public logic with no test surface covering it — the tunable definition, §5 of the document |
| `focused-test` | `test.findings.focused_test` | notable | an `it.only`/`fit`/`fdescribe` shipped: every other test in that run is silently skipped |

**The knob table.** The definition AC 3 asks to be "documented and tunable rather than a
guess" is a set of numbers, and a number in a source file is a guess with an author. Each
one is declared in `e0.8`'s schema, resolves through the same five layers as every other
key, and prints the layer that won — so a repository that thinks four body lines is the
wrong floor says so in `fathom.toml` and the card quotes that file back.

```
test.untested.min_body_lines    4     ≥ this many non-blank body lines before a symbol is "logic"
test.untested.require_logic     true  a public type/const/re-export is not "logic" on its own
test.untested.visibility        public  which Rust visibilities count as public (`public`, `crate`)
test.untested.scope             episode  must the covering test be *in this episode*, or may an
                                      existing repository test reference count (needs the index)
test.untested.name_overlap      1     shared name tokens between symbol and new test case to count
test.untested.path_overlap      1     shared path tokens between the source file and a changed test file
test.untested.mention_in_test   true  a new symbol named inside added test lines counts as covered
test.untested.exclude           []    paths this finding never fires for (examples, benches, …)
test.proxy.confidence.parsed    0.90  case names parsed for every test surface in the episode
test.proxy.confidence.partial   0.60  some test surface had no parser or refused
test.proxy.confidence.lines_only 0.30 no test framework present at all: line counts only
test.card.max_findings          12    rows before the card says "… and N more, run `fathom testsurface findings <ep>`"
```

Precedence is `e0.8`'s, unchanged: flags > env > repo `fathom.toml` > user config > built-in
default. Booleans merge `replace`, so a repository can actually say no.
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace

from recommender.config.globs import matching as _glob_matching

# --- finding ids -------------------------------------------------------------
TEST_REMOVED = "test-removed"
TEST_FILE_REMOVED = "test-file-removed"
TEST_SKIPPED = "test-skipped"
ASSERTION_WEAKENED = "assertion-weakened"
ASSERTION_REMOVED = "assertion-removed"
UNTESTED_PUBLIC = "untested-public"
FOCUSED_TEST = "focused-test"

SEVERITY_HIGH = "high"
SEVERITY_NOTABLE = "notable"
SEVERITY_NOTE = "note"
SEVERITY_ORDER: tuple[str, ...] = (SEVERITY_HIGH, SEVERITY_NOTABLE, SEVERITY_NOTE)


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
    unit: str = "test"                  # "test" | "symbol" | "assertion" | "file"

    @property
    def phrase(self) -> str:
        return self.title.lower()


RULES: tuple[Rule, ...] = (
    Rule(TEST_REMOVED, "Test removed", "test.findings.test_removed", True, SEVERITY_HIGH,
         "A named test case that ran in the old revision does not exist in the new one. "
         "Reported per case, with the case name and its declaring line — including the cases "
         "lost because the whole file went.", detector="diff.case_delta", unit="test"),
    Rule(TEST_FILE_REMOVED, "Test file removed", "test.findings.test_file_removed", True,
         SEVERITY_HIGH,
         "A test file was deleted. The file row is printed even when every case inside it is "
         "also listed, because 'the suite is gone' is the fact a reviewer acts on.",
         detector="delta.files", unit="file"),
    Rule(TEST_SKIPPED, "Test skipped or expected-to-fail", "test.findings.test_skipped", True,
         SEVERITY_HIGH,
         "A case that used to run is now `#[ignore]`, `@pytest.mark.skip`/`skipif`/`xfail`, "
         "`@unittest.skip`, `t.Skip`, `it.skip` or `@Disabled`. Same channel as a deletion: "
         "the test still exists and still proves nothing.",
         detector="diff.case_delta", unit="test"),
    Rule(ASSERTION_WEAKENED, "Assertion weakened", "test.findings.assertion_weakened", True,
         SEVERITY_HIGH,
         "An assertion that compared a value against an expected value was replaced by one "
         "that only checks the value exists, is truthy, or was called — matched subject to "
         "subject, so the claim is about the same thing both times.",
         detector="diff.case_delta", unit="assertion"),
    Rule(ASSERTION_REMOVED, "Assertions removed", "test.findings.assertion_removed", True,
         SEVERITY_NOTABLE,
         "A test case that survived lost exact assertions and gained none. Weaker than the "
         "weakened finding and reported separately, because sometimes the deleted assertion "
         "was the redundant one and the reviewer should just be told.",
         detector="diff.case_delta", unit="assertion"),
    Rule(UNTESTED_PUBLIC, "New public logic, no new test", "test.findings.untested_public",
         True, SEVERITY_NOTABLE,
         "The tunable definition: a public symbol added by this episode, with at least "
         "`test.untested.min_body_lines` body lines and at least one line of logic, that no "
         "test surface in the episode covers by path, by name overlap, or by mention.",
         detector="delta.coverage", unit="symbol"),
    Rule(FOCUSED_TEST, "Focused test shipped", "test.findings.focused_test", True,
         SEVERITY_NOTABLE,
         "`it.only`, `fit`, `fdescribe`, `test.only` in committed code: the file's other "
         "tests do not run in CI, and nothing about the run says so.",
         detector="diff.case_delta", unit="test"),
)

RULE_INDEX: dict[str, Rule] = {rule.id: rule for rule in RULES}

#: `test.findings.<tail>` and the finding id differ only by `-` versus `_`; both spellings are
#: accepted wherever a caller names a finding kind.
_BY_KEY_TAIL: dict[str, Rule] = {rule.config_key.rsplit(".", 1)[-1]: rule for rule in RULES}

#: Every finding id, in table order — the print order and the `policy` verb's order.
FINDING_IDS: tuple[str, ...] = tuple(rule.id for rule in RULES)


@dataclass(frozen=True)
class Knob:
    """One tunable number or list in the definition, with its built-in default."""

    name: str
    key: str
    default: object
    kind: str
    units: str = ""
    doc: str = ""


KNOBS: tuple[Knob, ...] = (
    Knob("min_body_lines", "test.untested.min_body_lines", 4, "integer", "lines",
         "A public symbol needs at least this many non-blank body lines before the episode "
         "is said to have added 'logic'. Low values fire on one-line re-export helpers; high "
         "values miss small but real functions."),
    Knob("require_logic", "test.untested.require_logic", True, "boolean", "",
         "Only count a symbol as logic if its body calls, assigns, branches or returns. Off "
         "means a new `pub struct` or a new constant can raise the finding, which is what a "
         "schema-review repository may want."),
    Knob("visibility", "test.untested.visibility", "public", "string", "visibility",
         "Which Rust visibilities count as public: `public` (plain `pub` only) or `crate` "
         "(also `pub(crate)`). Other languages have no equivalent knob and are unaffected."),
    Knob("scope", "test.untested.scope", "episode", "string", "scope",
         "`episode`: a covering test must be in this episode (default, and honest without a "
         "repository index). `repo`: a symbol the repository's existing tests already mention "
         "is not flagged — needs the caller to supply `repo_test_mentions`, and the card says "
         "when it was asked for and not given."),
    Knob("name_overlap", "test.untested.name_overlap", 1, "integer", "tokens",
         "How many name tokens a new test case must share with a new symbol for it to count "
         "as covering it (`load_snapshot` ↔ `TestSnapshotLoading` share `snapshot`)."),
    Knob("path_overlap", "test.untested.path_overlap", 1, "integer", "tokens",
         "How many path-name tokens a changed test file must share with the source file "
         "(`src/store/load.rs` ↔ `tests/store_load_test.rs` share `store`, `load`), for "
         "repositories without a path convention worth encoding in `test.globs`."),
    Knob("mention_in_test", "test.untested.mention_in_test", True, "boolean", "",
         "If a new symbol's name appears in lines the episode added to a test file, treat it "
         "as covered. The loosest of the three coverage tests, and the only one that catches "
         "a new function asserted inside a pre-existing test."),
    Knob("exclude", "test.untested.exclude", [], "glob_list", "paths",
         "Paths this finding never fires for — `**/examples/**`, `**/benches/**` and friends. "
         "`extend`, so a repository adds to the list rather than replacing it."),
    Knob("proxy_confidence_parsed", "test.proxy.confidence.parsed", 0.9, "number", "confidence",
         "Stated confidence when every test surface in the episode was parsed into named cases."),
    Knob("proxy_confidence_partial", "test.proxy.confidence.partial", 0.6, "number",
         "confidence",
         "Stated confidence when some test surface had no parser, or a parser refused it: the "
         "names are partly known."),
    Knob("proxy_confidence_lines_only", "test.proxy.confidence.lines_only", 0.3, "number",
         "confidence",
         "Stated confidence when no test framework was found at all and the only honest number "
         "is a line count. Deliberately low: a line count correlates with test effort, it does "
         "not measure it."),
    Knob("max_findings", "test.card.max_findings", 12, "integer", "rows",
         "How many finding rows the card prints before it says how many more there are and "
         "which verb lists them. A cap, never a truncation without a trace."),
)

KNOB_INDEX: dict[str, Knob] = {knob.name: knob for knob in KNOBS}
KEY_TO_KNOB: dict[str, Knob] = {knob.key: knob for knob in KNOBS}

#: Config keys this package reads, for the analyzer manifest (ADR-004 §5.1).
CONFIG_KEYS: tuple[str, ...] = (
    "test.globs",
) + tuple(rule.config_key for rule in RULES) + tuple(knob.key for knob in KNOBS)

#: Test-side paths this package never asks about as production surface, whatever the layers say.
BUILTIN_EXCLUDES: tuple[str, ...] = ("**/examples/**", "**/benches/**", "**/fixtures/**")


class UnanchoredFinding(Exception):
    """A finding that cannot name its test and its location. Raised instead of rendered."""


@dataclass
class Finding:
    """One finding, with the name and location AC 2 requires and the evidence to check it."""

    kind: str
    name: str
    path: str
    line: int = 0
    detail: str = ""
    evidence: list[str] = field(default_factory=list)
    language: str = ""
    old_line: int = 0
    unit: str = "test"

    def __post_init__(self) -> None:
        if self.kind not in RULE_INDEX:
            raise UnanchoredFinding("unknown finding kind %r" % self.kind)
        if not self.name or not self.name.strip():
            raise UnanchoredFinding("%s: a finding must name the test or symbol" % self.kind)
        if not self.path:
            raise UnanchoredFinding("%s %s: a finding must name a path" % (self.kind, self.name))
        if self.unit in ("test", "assertion") and not self.line:
            raise UnanchoredFinding("%s %s: a finding must carry a line number"
                                    % (self.kind, self.name))
        if not self.evidence:
            self.evidence = [self.detail] if self.detail else ["no further evidence recorded"]

    @property
    def rule(self) -> Rule:
        return RULE_INDEX[self.kind]

    @property
    def severity(self) -> str:
        return self.rule.severity

    @property
    def location(self) -> str:
        return "%s:%d" % (self.path, self.line) if self.line else self.path

    @property
    def old_location(self) -> str:
        return "%s:%d" % (self.path, self.old_line) if self.old_line else ""

    def render(self) -> str:
        """The card row. Name and location come first: they are the clickable part."""
        parts = ["%s: %s" % (self.rule.title, self.name)]
        if self.detail:
            parts.append(self.detail)
        parts.append("at %s" % self.location)
        if self.old_location and self.old_location != self.location:
            parts.append("(was at %s)" % self.old_location)
        if self.language:
            parts.append("[%s]" % self.language)
        return " · ".join(parts)

    def block(self, indent: int = 2) -> list[str]:
        pad = " " * indent
        lines = [pad + self.render()]
        lines.extend(pad + "  · %s" % line for line in self.evidence if line)
        return lines

    def as_dict(self) -> dict:
        return {"kind": self.kind, "title": self.rule.title, "severity": self.severity,
                "name": self.name, "path": self.path, "line": self.line,
                "location": self.location, "old_line": self.old_line, "unit": self.unit,
                "language": self.language, "detail": self.detail,
                "evidence": list(self.evidence), "config_key": self.rule.config_key}


@dataclass
class Policy:
    """The effective finding policy and definition for one repository."""

    enabled: dict[str, bool] = field(default_factory=dict)
    source: dict[str, str] = field(default_factory=dict)
    knobs: dict[str, object] = field(default_factory=dict)
    knob_source: dict[str, str] = field(default_factory=dict)
    config_digest: str = ""

    # -- construction -------------------------------------------------------
    @classmethod
    def built_in(cls) -> "Policy":
        return cls(enabled={rule.id: rule.default for rule in RULES},
                   knobs={knob.name: knob.default for knob in KNOBS})

    @classmethod
    def from_config(cls, config=None) -> "Policy":
        policy = cls()
        for rule in RULES:
            value, source = _lookup(config, rule.config_key, rule.default)
            policy.enabled[rule.id] = bool(value) if value is not None else rule.default
            policy.source[rule.id] = source
        for knob in KNOBS:
            value, source = _lookup(config, knob.key, knob.default)
            policy.knobs[knob.name] = knob.default if value is None else _coerce(knob, value)
            policy.knob_source[knob.name] = source
        try:
            policy.config_digest = config.digest_for("test") if config else ""
        except Exception:                            # noqa: BLE001 — no manifest, no digest
            policy.config_digest = ""
        return policy

    # -- the questions ------------------------------------------------------
    def enabled_for(self, rule_id: str) -> bool:
        return bool(self.enabled.get(rule_id, RULE_INDEX[rule_id].default))

    # -- caller overrides ---------------------------------------------------
    def with_knobs(self, source: str = "caller override", **names) -> "Policy":
        """A copy with these definition knobs overridden, each labelled with where it came from.

        The labelled source is not decoration: `describe()` prints it, so a card or a corpus
        row built under an override says which number it was measured under. The corpus uses
        this to run one piece of code under two definitions and prove the knobs are real.
        """
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source))
        for name, value in names.items():
            knob = KNOB_INDEX[name]
            clone.knobs[name] = _coerce(knob, value)
            clone.knob_source[name] = "%s (%s=%s)" % (source, name, _format(value))
        return clone

    def with_flag(self, rule_id: str, enabled: bool,
                  source: str = "caller override") -> "Policy":
        """A copy with one finding kind switched on or off, accepting the id or the key tail."""
        rule = RULE_INDEX.get(rule_id) or _BY_KEY_TAIL.get(rule_id)
        if rule is None:
            raise KeyError("no finding kind %r" % rule_id)
        clone = replace(self, enabled=dict(self.enabled), source=dict(self.source),
                        knobs=dict(self.knobs), knob_source=dict(self.knob_source))
        clone.enabled[rule.id] = bool(enabled)
        clone.source[rule.id] = source
        return clone

    def effective(self, rule_id: str) -> tuple[bool, str]:
        return self.enabled_for(rule_id), self.source.get(rule_id, "built-in default")

    def knob(self, name: str) -> object:
        return self.knobs.get(name, KNOB_INDEX[name].default)

    def number(self, name: str) -> int | float:
        value = self.knob(name)
        return value if isinstance(value, (int, float)) else KNOB_INDEX[name].default

    def flag(self, name: str) -> bool:
        return bool(self.knob(name))

    def text(self, name: str) -> str:
        return str(self.knob(name) or "")

    def excluded(self, path: str) -> str:
        """The exclusion glob that claims this path, `""` if none does."""
        globs = list(self.knob("exclude") or [])
        hits = _glob_matching(globs, path) or []
        return hits[0] if hits else ""

    def counts_as_public(self, visibility: str) -> bool:
        if visibility in ("", "public"):
            return True
        if visibility == "crate":
            return self.text("visibility") == "crate"
        return False

    def describe(self) -> list[str]:
        """`fathom testsurface policy`: every switch, every number, every winning layer."""
        out: list[str] = ["test surface policy (highest precedence first)"]
        out.append("  findings")
        for rule in RULES:
            enabled, source = self.effective(rule.id)
            out.append("  %-20s %-5s %-38s %s"
                       % (rule.id, "on" if enabled else "off", rule.config_key, source))
        out.append("  definition")
        for knob in KNOBS:
            out.append("  %-26s %-10s %-40s %s"
                       % (knob.name, _format(self.knob(knob.name)), knob.key,
                          self.knob_source.get(knob.name, "built-in default")))
        return out

    def definition_lines(self) -> list[str]:
        """The tunable definition, spelled out — printed by `explain --definition`."""
        return ["%s = %s  (%s)" % (knob.name, _format(self.knob(knob.name)), knob.doc)
                for knob in KNOBS]

    def as_dict(self) -> dict:
        return {"findings": {rule.id: {"enabled": self.enabled_for(rule.id),
                                       "source": self.source.get(rule.id, "built-in default"),
                                       "config_key": rule.config_key, "severity": rule.severity}
                             for rule in RULES},
                "knobs": {knob.name: {"value": self.knob(knob.name), "key": knob.key,
                                      "default": knob.default, "units": knob.units,
                                      "source": self.knob_source.get(knob.name,
                                                                     "built-in default")}
                          for knob in KNOBS},
                "config_digest": self.config_digest}


def describe_rules() -> list[str]:
    """The finding table on its own, independent of any repository's configuration."""
    out: list[str] = []
    for rule in RULES:
        out.append("%-20s %-8s %-40s %s" % (rule.id, rule.severity, rule.config_key, rule.doc))
    return out


def sort_key(finding: Finding) -> tuple:
    return (SEVERITY_ORDER.index(finding.severity), finding.kind, finding.path,
            finding.line, finding.name)


def _format(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        return ("%.2f" % value).rstrip("0").rstrip(".")
    if isinstance(value, list):
        return ", ".join(value) or "—"
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
    if knob.kind == "glob_list":
        return list(value or [])
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
