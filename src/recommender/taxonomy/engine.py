"""The engine: resolve a repository path to a class and a noise level, with its receipts.

One object, :class:`Taxonomy`, holds the effective table (built-ins plus everything
`fathom.toml` adds) and answers two questions per path:

    taxonomy.classify("src/api/client.ts")      # -> Verdict(class="source", noise="informative", …)
    taxonomy.classify("dist/app.min.js").noise  # -> "generated"

A :class:`Verdict` is not a string. It carries the rule that won, the layer it came from,
the pattern that actually matched, and every rule it beat with the reason each one lost —
because "the classifier said generated" is not a sentence a reviewer can act on, and the
ticket's acceptance criterion is that the decision is *explainable*, not that it is right
(rightness is the decision-table test's job).

Cost, and why it is bounded
---------------------------
`classify` is O(depth + extensions + segments + |broad|) dict probes, then a glob match
against the surviving candidates — typically three to eight, against regexes compiled once
at build time. Nothing in the hot path allocates a regex, and a second memo layer means a
10 000-file commit touching the same paths twice pays once. `tests/test_taxonomy_contract.py`
measures it, and `fathom taxonomy bench` prints the number so a regression is a number
somebody can see rather than a feeling.

Two axes, resolved independently
--------------------------------
The class axis and the noise axis each get their own winner, because they can legitimately
disagree: a `fathom.toml` may say `.plan/plan.json` is noise without claiming to know what
class of thing it is, and the class axis then still answers `config`; a `tests/x.snap` is
test surface *and* machine-written, and two axes let the engine say both instead of
averaging them into one label. The two axes are therefore **not** ranked the same way:

* **class** — layer, then specificity, then declaration order. Which surface a path belongs
  to is a question about the tree, so the more specific claim about the tree wins.
* **noise** — layer, then "does this rule call the path derived at all", then specificity,
  then whether the rule can *name* its category. "`.snap` is machine-written" is a stronger
  claim about reviewability than "this path lives in `tests/`", so it is consulted before
  specificity; the naming term at the end is what keeps a seed entry in the config's own
  defaults list from out-shouting the table entry that knows why the path is noise.

Both keys are built by :func:`_rank` and both term lists are declared in :data:`_RANK_TERMS`,
so the reasons `explain` prints cannot drift from the comparison that was actually made.

A tie broken by declaration order — two patterns of identical specificity in one layer —
falls to the later rule, the gitignore convention; `Rule.seq` carries it and `explain`
names it when it is what decided.
"""

from __future__ import annotations

import threading
from dataclasses import dataclass, field, replace

from recommender.config import globs
from recommender.config.errors import Warning
from recommender.config.schema import Layer

from . import builtins as B
from . import classes as C
from . import noise as N
from . import rules as R

#: How many distinct paths one `Taxonomy` memoises. Per-path memo, not per-blob: the same
#: path is classified again on every commit that touches it, and a review session touches
#: the same few hundred paths over and over. Bounded so a long-lived daemon's RSS does not
#: track repository size (ADR-004 §11.5's "never scale memory by what we can stream").
MEMO_LIMIT = 8192

#: The reason strings `explain` prints. Kept as constants so a test can assert on the
#: *reason a rule lost* rather than on a paragraph of prose.
REASON_AGREES = "agrees"
REASON_LAYER = "lost: layer"
REASON_DERIVED = ("lost: a rule that names it as derived outranks one that only "
                  "names where it lives")
REASON_SPECIFICITY = "lost: specificity"
REASON_NAMED = "lost: a named noise category beats an unnamed one"
REASON_ORDER = "lost: declaration order"
REASON_EXCLUDED = "excluded: a `!` exception named this path"
REASON_WINNER = "winner"


_RANK_TERMS = {
    # The precedence terms per axis, in the order they are consulted. `explain` names the
    # first one that differs, so this table and `_reason` must stay in step.
    "class": ("layer", "specificity", "declaration order"),
    "noise": ("layer", "derived-first", "specificity", "named-category",
              "declaration order"),
}


FALLBACK = R.Rule(
    id="fallback.unclassified", patterns=(), cls=C.PathClass.OTHER,
    noise=N.Noise.INFORMATIVE, origin="builtin",
    note="no rule matched; an unknown path is `other`, not `source` (e2.1's rule)",
    source="built-in taxonomy table v%d" % B.BUILTINS_VERSION)


# ---------------------------------------------------------------------------
# verdicts
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Match:
    """One candidate rule, the pattern of its that matched, and how it fared."""

    rule: R.Rule
    pattern: str
    specificity: tuple
    outcome: str = ""

    @property
    def rule_id(self) -> str:
        return self.rule.id

    @property
    def layer(self) -> str:
        return self.rule.layer

    @property
    def source(self) -> str:
        return self.rule.source

    @property
    def negated(self) -> bool:
        return self.rule.negated

    def verdict_text(self) -> str:
        return self.rule.noise_level or "—"

    def as_dict(self) -> dict:
        return {"rule": self.rule.id, "pattern": self.pattern, "layer": self.rule.layer,
                "layer_label": Layer.label(self.rule.layer), "source": self.rule.source,
                "class": self.rule.cls, "noise": self.rule.noise_level,
                "specificity": list(self.specificity), "outcome": self.outcome,
                "key": self.rule.key, "note": self.rule.note,
                "negated": self.rule.negated}


@dataclass
class Verdict:
    """The answer for one path: class, noise, and the receipts for both."""

    path: str
    cls: str
    noise: str
    class_match: Match | None = None
    noise_match: Match | None = None
    class_matches: list[Match] = field(default_factory=list)
    noise_matches: list[Match] = field(default_factory=list)
    taxonomy_version: int = B.BUILTINS_VERSION
    table_digest: str = ""

    @property
    def noise_score(self) -> float:
        return N.score_of(self.noise)

    @property
    def is_noise(self) -> bool:
        return N.is_noise(self.noise)

    @property
    def matched(self) -> bool:
        """Whether a real rule spoke for this path (as opposed to the fallback)."""
        return bool(self.class_match and self.class_match.rule.id != FALLBACK.id)

    @property
    def diffable(self) -> bool:
        return C.is_diffable(self.cls)

    @property
    def reviewable(self) -> bool:
        return C.is_reviewable(self.cls)

    @property
    def rule_id(self) -> str:
        return self.class_match.rule_id if self.class_match else FALLBACK.id

    @property
    def layer(self) -> str:
        return self.class_match.layer if self.class_match else Layer.DEFAULTS

    def runners_up(self, axis: str = "class") -> list[Match]:
        matches = self.class_matches if axis == "class" else self.noise_matches
        return [m for m in matches if m.outcome != REASON_WINNER]

    def as_dict(self) -> dict:
        return {"path": self.path, "class": self.cls, "noise": self.noise,
                "noise_score": round(self.noise_score, 3), "is_noise": self.is_noise,
                "diffable": self.diffable, "reviewable": self.reviewable,
                "matched": self.matched,
                "taxonomy_version": self.taxonomy_version, "table_digest": self.table_digest,
                "class_rule": self.class_match.as_dict() if self.class_match else None,
                "noise_rule": self.noise_match.as_dict() if self.noise_match else None,
                "class_matches": [m.as_dict() for m in self.class_matches],
                "noise_matches": [m.as_dict() for m in self.noise_matches]}


# ---------------------------------------------------------------------------
# the taxonomy
# ---------------------------------------------------------------------------
class Taxonomy:
    """The effective table, indexed, for one repository's resolved configuration.

    Built once per index run and called per changed file. Immutable after construction,
    which is what lets a whole process share one instance (and what makes the memo safe):
    a rule added after the fact would change answers without changing the digest, and a
    cache key that lies is worse than no cache.
    """

    def __init__(self, rules: list[R.Rule], *, version: int = B.BUILTINS_VERSION,
                 warnings: list[Warning] | None = None, config=None):
        self.version = version
        self.warnings = list(warnings or [])
        self.config = config
        self.rules: list[R.Rule] = []
        self.index = R.RuleIndex()
        self._memo: dict[str, Verdict] = {}
        self._lock = threading.Lock()
        for seq, rule in enumerate(rules):
            rule = _with_seq(rule, seq)
            self.rules.append(rule)
            for pattern in rule.patterns:
                self.index.add(rule, pattern)
        self.digest = R.table_digest(self.rules, version)

    # -- construction ------------------------------------------------------
    @classmethod
    def builtins_only(cls) -> "Taxonomy":
        return cls(B.builtin_rules())

    @classmethod
    def from_config(cls, config) -> "Taxonomy":
        """Built-ins plus everything a resolved `Config` adds, layer by layer.

        Accepts a :class:`recommender.config.layer.Config` (the normal path, where each
        contributing layer keeps its own provenance) or a plain nested dict, which is what
        the storage fixture has on hand — see :meth:`from_dict`.
        """
        if config is None or isinstance(config, dict):
            return cls.from_dict(config or {})
        warnings: list[Warning] = []
        user = R.rules_from_config(config, warnings)
        version = _config_int(config, "noise.builtins_version", B.BUILTINS_VERSION)
        return cls(B.builtin_rules() + user, version=version, warnings=warnings,
                   config=config)

    @classmethod
    def from_dict(cls, values: dict, *, layer: str = Layer.REPO,
                  source: str = "resolved config (dict)") -> "Taxonomy":
        """Read the taxonomy keys out of a plain dict — the shape `store.git` carries.

        A dict has no layers, so its rules are given `layer`, defaulting to `repo`: the
        config a project committed is the common case, and it has to outrank the built-in
        table or a repository's own `[test] globs` would be advisory. The alternative —
        `defaults` — would put a caller's own configuration *below* the table it is meant
        to adjust.
        """
        warnings: list[Warning] = []
        out: list[R.Rule] = []

        def add(patterns, cls, key):
            for index, raw in enumerate(patterns or []):
                pattern = str(raw)
                negated = R.is_negated(pattern)
                if negated and cls is not None:
                    warnings.append(Warning("taxonomy_negation_ignored",
                                            "%r in %s is negated; a class list has nothing "
                                            "to re-promote it to" % (pattern, key),
                                            source=source, key=key))
                    continue
                out.append(R.Rule(id="cfg.%s.%s#%d" % (layer, key, index),
                                  patterns=(pattern,),
                                  cls=None if negated else cls,
                                  noise=(N.Noise.INFORMATIVE if negated else
                                         (N.Noise.UNKNOWN if cls is None else None)),
                                  layer=layer, source=source, origin="config", key=key,
                                  negated=negated))

        noise_values = (values.get("noise") or {}).get("globs") or []
        add(noise_values, None, "noise.globs")
        test_values = (values.get("test") or {}).get("globs") or []
        add(test_values, C.PathClass.TEST, "test.globs")
        for name, patterns in sorted(((values.get("taxonomy") or {}).get("classes") or {})
                                     .items()):
            key = "taxonomy.classes.%s" % name
            if not C.assertable(name):
                warnings.append(Warning("taxonomy_class_unknown",
                                        "%s names an unknown taxonomy class %r" % (key, name),
                                        source=source, key=key,
                                        suggestion="one of: %s" % ", ".join(C.ASSERTABLE_CLASSES)))
                continue
            add(patterns, name, key)
        version = ((values.get("noise") or {}).get("builtins_version")
                   or B.BUILTINS_VERSION)
        return cls(B.builtin_rules() + out, version=int(version), warnings=warnings)

    # -- the questions -----------------------------------------------------
    def classify(self, path: str) -> Verdict:
        """The decision for one repository-relative path. Cheap; see the module docstring."""
        text = globs.normalise(str(path))
        if not text:
            return self._fallback(text)
        cached = self._memo.get(text)
        if cached is not None:
            return cached
        matched = [entry for entry in self.index.candidates(text) if entry.matches(text)]
        verdict = self._resolve(text, matched)
        with self._lock:
            if len(self._memo) >= MEMO_LIMIT:
                self._memo.clear()             # bounded; a daemon's RSS must not track history
            self._memo[text] = verdict
        return verdict

    def classify_many(self, paths) -> list[Verdict]:
        return [self.classify(p) for p in paths]

    def candidates(self, path: str) -> list[R.Entry]:
        """Every entry that *actually matches* `path` — the diagnostic behind the cost claim."""
        text = globs.normalise(str(path))
        return [entry for entry in self.index.candidates(text) if entry.matches(text)]

    def probed(self, path: str) -> int:
        """How many entries the index handed to the matcher for `path`."""
        text = globs.normalise(str(path))
        return len(self.index.candidates(text))

    def noise_share(self, paths, weights=None) -> float:
        """Noise share of a change set — the glossary's *noise share*, line-weighted."""
        return N.noise_share([self.classify(p) for p in paths], weights)

    def summary(self, paths) -> dict[str, int]:
        """Per-class counts for a change set: the where-did-it-land bar `e2.2`/`e2.10` show."""
        out: dict[str, int] = {}
        for path in paths:
            cls = self.classify(path).cls
            out[cls] = out.get(cls, 0) + 1
        return dict(sorted(out.items(), key=lambda kv: (-kv[1], kv[0])))

    def noise_summary(self, paths) -> dict[str, int]:
        out: dict[str, int] = {}
        for path in paths:
            level = self.classify(path).noise
            out[level] = out.get(level, 0) + 1
        return dict(sorted(out.items(), key=lambda kv: (-kv[1], kv[0])))

    def clear_memo(self) -> None:
        """Drop the memo. Public so `bench` and the tests can measure the index, not the cache."""
        with self._lock:
            self._memo.clear()

    def stats(self) -> dict:
        return {"rules": len(self.rules), "entries": self.index.size,
                "buckets": self.index.counts(), "version": self.version,
                "digest": self.digest, "warnings": len(self.warnings),
                "memo": len(self._memo)}

    # -- resolution --------------------------------------------------------
    def _resolve(self, path: str, matched: list[R.Entry]) -> Verdict:
        class_cands = [e for e in matched if e.rule.claims_class]
        noise_cands = [e for e in matched if e.rule.noise_level is not None]

        # A `!` entry is not a noise rule; it is a veto. A path named by a negation at layer
        # L has its noise claims from L and below withdrawn, which is what makes
        # `globs = ["docs/reports/**", "!docs/reports/curated.md"]` mean "and not this":
        # the negation outranks the list it is in, and a repo-layer exception cannot be undone
        # by anything at user or defaults, because layer is still the first term everywhere.
        veto = max((Layer.rank(e.rule.layer) for e in noise_cands if e.rule.negated),
                   default=None)
        active = [e for e in noise_cands
                  if not e.rule.negated
                  and (veto is None or Layer.rank(e.rule.layer) < veto)]

        class_win, class_matches = self._winners(class_cands, "class")
        noise_win, noise_matches = self._winners(active, "noise")
        # The vetoed rules stay in the receipts: "these three globs called it noise and a `!`
        # you wrote cancelled them" is the part of the answer a reader needs.
        noise_matches += [Match(rule=e.rule, pattern=e.pattern, specificity=e.specificity,
                                outcome=REASON_EXCLUDED)
                          for e in noise_cands if e.rule.negated]

        if class_win is None:
            # Nothing claimed a class. The noise axis may still have a real winner — a bare
            # `noise.globs` entry says "hide this" without claiming to know what it is, and
            # the class answer is `other` beside it. Throwing the noise receipts away here
            # would make `fathom explain` blind to the only rule that spoke.
            fallback = Match(rule=FALLBACK, pattern="—", specificity=(0, 0, 0, 0, 0, 0),
                             outcome=REASON_WINNER)
            return Verdict(path=path, cls=FALLBACK.cls, noise=(noise_win.rule.noise_level
                                                              if noise_win else
                                                              FALLBACK.noise_level),
                           class_match=fallback, noise_match=noise_win or fallback,
                           class_matches=[fallback],
                           noise_matches=noise_matches or [fallback],
                           taxonomy_version=self.version, table_digest=self.digest)

        return Verdict(
            path=path, cls=class_win.rule.cls,
            noise=(noise_win.rule.noise_level if noise_win is not None
                   else class_win.rule.noise_level),
            class_match=class_win, noise_match=noise_win or class_win,
            class_matches=class_matches, noise_matches=noise_matches or [class_win],
            taxonomy_version=self.version, table_digest=self.digest)

    def _fallback(self, path: str) -> Verdict:
        fallback = Match(rule=FALLBACK, pattern="—", specificity=(0, 0, 0, 0, 0, 0),
                         outcome=REASON_WINNER)
        return Verdict(path=path, cls=FALLBACK.cls, noise=FALLBACK.noise_level,
                       class_match=fallback, noise_match=fallback,
                       class_matches=[fallback], noise_matches=[fallback],
                       taxonomy_version=self.version, table_digest=self.digest)

    def _winners(self, candidates: list[R.Entry], axis: str
                 ) -> tuple[Match | None, list[Match]]:
        if not candidates:
            return None, []
        ranked = sorted(candidates, key=lambda e: _rank(e, axis), reverse=True)
        top = ranked[0]
        matches: list[Match] = []
        for entry in ranked:
            outcome = REASON_WINNER if entry is top else _reason(top, entry, axis)
            matches.append(Match(rule=entry.rule, pattern=entry.pattern,
                                 specificity=entry.specificity, outcome=outcome))
        return matches[0], matches


def _with_seq(rule: R.Rule, seq: int) -> R.Rule:
    return replace(rule, seq=seq)


def _rank(entry: R.Entry, axis: str) -> tuple:
    """The precedence key for one axis, biggest wins. Documented in the module docstring.

    The two axes are *not* symmetric, and the asymmetry is the interesting part:

    * **class** — layer, then specificity, then declaration order. Which surface a path
      belongs to is a question about the tree, and the more specific claim is the better one.
    * **noise** — layer, then "does this rule call the path derived at all", then
      specificity, then "can it name the category". A rule that says `.snap`, `.min.js` or
      `Cargo.lock` is machine-written makes a stronger claim about *reviewability* than a
      rule that says the path sits in a hand-written directory, so on this axis it goes
      first: `tests/x.snap` is test surface (class) *and* noise (level), and it is the two
      axes that let the engine say both instead of averaging them into a single label.

    `named-category` is what keeps a seed entry in the config's own defaults list from
    out-shouting the table entry that knows why the path is noise. A user glob at a higher
    layer still wins outright, because layer is first.
    """
    layer = -Layer.rank(entry.rule.layer)
    if axis != "noise":
        return (layer, entry.specificity, entry.rule.seq)
    level = entry.rule.noise_level or N.Noise.INFORMATIVE
    return (layer, 1 if N.is_noise(level) else 0, entry.specificity,
            0 if level == N.Noise.UNKNOWN else 1, entry.rule.seq)


def _reason(winner: R.Entry, loser: R.Entry, axis: str) -> str:
    """Why one matched rule did not decide the answer, in one short phrase."""
    if axis == "class" and winner.rule.cls == loser.rule.cls:
        return REASON_AGREES
    if axis == "noise" and winner.rule.noise_level == loser.rule.noise_level:
        return REASON_AGREES
    w, l = _rank(winner, axis), _rank(loser, axis)
    for index, term in enumerate(_RANK_TERMS[axis]):
        if w[index] == l[index]:
            continue
        if term == "layer":
            return "%s: %s beats %s" % (REASON_LAYER, Layer.label(winner.rule.layer),
                                         Layer.label(loser.rule.layer))
        if term == "specificity":
            return "%s: `%s` says more than `%s`" % (REASON_SPECIFICITY, winner.pattern,
                                                     loser.pattern)
        if term == "derived-first":
            return REASON_DERIVED
        if term == "named-category":
            return REASON_NAMED
        return "%s: `%s` came later" % (REASON_ORDER, winner.pattern)
    return REASON_AGREES


def _config_int(config, path: str, default: int) -> int:
    try:
        value = config.get(path, default)
        return int(value) if value is not None else default
    except Exception:                                      # noqa: BLE001 - degrade
        return default


# ---------------------------------------------------------------------------
# the shared built-ins-only instance
# ---------------------------------------------------------------------------
_DEFAULT: Taxonomy | None = None
_DEFAULT_LOCK = threading.Lock()


def default_taxonomy() -> Taxonomy:
    """A process-wide built-ins-only taxonomy, for callers with no configuration to pass.

    Cached because building the index is the one genuinely superlinear step (compiling ~200
    regexes), and callers like the store fixture have exactly one configuration: none.
    Anyone with a `fathom.toml` must build their own via :meth:`Taxonomy.from_config`; the
    whole point of the layering is that a repository's own file outranks this.
    """
    global _DEFAULT
    if _DEFAULT is None:
        with _DEFAULT_LOCK:
            if _DEFAULT is None:
                _DEFAULT = Taxonomy.builtins_only()
    return _DEFAULT


def reset_default_taxonomy() -> None:
    """Drop the shared instance. Tests only; a daemon never calls this."""
    global _DEFAULT
    _DEFAULT = None
