"""Rules: what one taxonomy rule claims, how specific it is, and how it is found cheaply.

The acceptance criterion this file exists for is the unfashionable one: *"Lookup is
O(log n)-ish and cheap enough to call per changed file (no regex-per-file blowup)"*. A
naive engine holds ~150 globs and runs a compiled regex per glob per changed file, so a
10 000-file commit costs 1.5 M regex matches and the classifier becomes the thing the
warm-index budget in ADR-004 §11 is spent on. Here a lookup instead consults four dict
buckets keyed by facts already known about the path — its exact string, its directory
ancestors, any of its path segments, its dotted suffixes — plus one deliberately small
`broad` list, and only the handful of candidates that survive run a glob match. The
candidate count is asserted in `tests/test_taxonomy_contract.py`, not hoped for.

The precedence model, in one paragraph
-------------------------------------

Two rules can both match a path, and exactly one of them decides it. The comparison is a
three-term key, evaluated in order:

1. **layer** — a rule from a higher config layer beats one from a lower layer, always.
   This is what makes the ticket's "user globs can both promote and demote a path
   relative to built-ins" true without caveats: `repo` beats `defaults` whatever the
   patterns look like, so `dist/**` in a `fathom.toml` outranks a built-in no matter how
   general it is.
2. **specificity** — within one layer, the rule whose pattern says more wins
   (:func:`specificity`). `*.pb.go` beats `*.go`; `src/api/**` beats `src/**`.
3. **declaration order** — last one wins, gitignore-style, so a later line in one's own
   `fathom.toml` edits an earlier one.

Rules carry at most two claims: a **class** (`cls`) and a **noise level** (`noise`). A
class rule with no explicit `noise` asserts its class's default level
(:func:`recommender.taxonomy.classes.default_noise`), so `vendored` says "vendored *and*
vendored-noise" in one line. A noise-only rule (an entry in `noise.globs`) asserts no
class at all: saying "this is noise" is not an opinion about what the file *is*. That is
also why the engine resolves the two axes independently and reports both winners.

A `!`-negated noise glob is a rule too — one that asserts `informative`. Modelling the
negation as a rule rather than as a subtraction is what makes `fathom explain` able to
name it: "the rule it beat" is frequently a negation, and a set difference has no name to
print.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, field

from recommender.config import globs, schema as S
from recommender.config.errors import Warning
from recommender.config.schema import Layer

from . import classes as C
from . import noise as N

_MAGIC_RE = re.compile(r"[*?\[\]{}]")
#: `**/dist/**` — a directory name at any depth. Its own bucket, because a bucket keyed by
#: the segment itself turns an "any depth" test into one dict probe per path segment.
_DIRSEG_RE = re.compile(r"\A\*\*/([^/*?{}\[\]]+)/\*\*\Z")
#: A pattern may not name a class we do not have, and may not claim content-only noise.
_NEGATION = "!"


@dataclass(frozen=True)
class Rule:
    """One named rule: an id, one or more patterns, and up to two claims.

    `id` is stable text (`gen.protobuf`, `cfg.docker`, `cfg.repo.test.globs#3`), because it is what
    `fathom explain` prints and what a bug report quotes. A rule with several patterns is
    one convention (`gen.protobuf` for `.pb.go` and `_pb.go`), which is why `explain`
    prints the id *and* the pattern that actually matched: the id is the convention, the
    pattern is the evidence.
    """

    id: str
    patterns: tuple[str, ...]
    cls: str | None = None
    noise: str | None = None
    note: str = ""
    layer: str = Layer.DEFAULTS
    source: str = "built-in taxonomy table"
    origin: str = "builtin"            # builtin | config
    seq: int = 0                       # global declaration order; tie-break for term 3
    key: str = ""                      # the config key a config rule came from
    negated: bool = False

    @property
    def claims_class(self) -> bool:
        return self.cls is not None

    @property
    def noise_level(self) -> str | None:
        """The noise this rule asserts, resolving a class rule's implied level."""
        if self.noise is not None:
            return self.noise
        if self.cls is not None:
            return C.default_noise(self.cls)
        return None

    def label(self) -> str:
        return "%s (%s)" % (self.id, ", ".join(self.patterns))

    def digest_token(self) -> str:
        """Stable text for the table digest: two builds agree iff their tokens agree."""
        return "|".join((self.id, ",".join(self.patterns), self.cls or "-",
                         self.noise_level or "-", self.layer))


@dataclass(frozen=True)
class Entry:
    """One (rule, pattern) pair, with the precomputed facts the resolver needs.

    `dirish` and `dir_prefix` carry the one piece of syntax a compiled regex cannot
    express: `dist/**` and `dist/` are written as "the directory and its contents", so
    they must also match a changed path that *is* the directory. `globs.matches` handles
    that by re-testing; doing it here keeps a per-file call to two regex probes and no
    string surgery, and keeps the taxonomy's answers identical to the config layer's.
    """

    rule: Rule
    pattern: str
    specificity: tuple
    compiled: object = field(repr=False, default=None)
    dirish: bool = False
    dir_prefix: str = ""

    def matches(self, path: str) -> bool:
        try:
            if self.compiled.match(path):
                return True
            if self.dirish and self.compiled.match(path + "/"):
                return True
        except Exception:                                  # noqa: BLE001 - see _compile
            return False
        return bool(self.dir_prefix) and path.startswith(self.dir_prefix)


def has_magic(pattern: str) -> bool:
    return bool(_MAGIC_RE.search(pattern))


def bare(pattern: str) -> str:
    """The pattern without its `!`, normalised the way every matcher in the repo does."""
    return globs.normalise(pattern[1:] if pattern.startswith(_NEGATION) else pattern)


def is_negated(pattern: str) -> bool:
    return pattern.startswith(_NEGATION)


def _tails(segment: str) -> tuple[int, int]:
    """(literal characters after the last magic char, literal characters before the first).

    `test_*.py` and `*.py` tie on "zero literal segments, tail `.py`", and the head run is
    what makes `test_*.py` the more specific claim — which is exactly the kind of tie a
    specificity function gets wrong silently, so it is a term rather than an accident.
    """
    if not has_magic(segment):
        return len(segment), len(segment)
    positions = [i for i, ch in enumerate(segment) if _MAGIC_RE.match(ch)]
    return len(segment) - max(positions) - 1, min(positions)


def _literal_chars(segs: list[str]) -> int:
    """How much literal text a pattern carries, ignoring `**` and every magic character.

    The term that settles the ties `literal_segments` cannot: `*_test.go` and `*.go` both
    have zero literal segments and both end in `.go`, and only the eight literal characters
    of the first make it the more specific claim.
    """
    total = 0
    for seg in segs:
        if seg == "**":
            continue
        total += len(_MAGIC_RE.sub("", seg))
    return total


def specificity(pattern: str) -> tuple:
    """How much a pattern says. Bigger tuple wins; documented in the module docstring.

    Ordered deliberately: literal segments first (a named directory outranks a wildcard
    directory), then total literal text, then the dotted tail (`.pb.go` outranks `.go`),
    then the head run (`test_*.py` outranks `*.py`), then fewer magic segments, then more
    segments.
    """
    segs = [s for s in bare(pattern).split("/") if s]
    if not segs:
        return (0, 0, 0, 0, 0, 0)
    literal = sum(1 for s in segs if not has_magic(s) and s != "**")
    magic = sum(1 for s in segs if has_magic(s))
    tail, head = _tails(segs[-1])
    return (literal, _literal_chars(segs), tail, head, -magic, -len(segs))


# ---------------------------------------------------------------------------
# the index
# ---------------------------------------------------------------------------
def bucket_for(pattern: str) -> tuple[str, str]:
    """Which bucket an entry lives in, and its key there. See :class:`RuleIndex`.

    Four buckets, each keyed by something a caller already has, so lookup is dict probes:
    `exact` (the whole path), `prefix` (a literal directory prefix: `src/api/**`),
    `dirseg` (a name at any depth: `**/dist/**`), `suffix` (a dotted tail: `.pb.go`).
    Anything that cannot be keyed — `**/Dockerfile*` — goes to `broad`, which is a
    linear scan and is therefore asserted small by a test.
    """
    p = bare(pattern)
    if not has_magic(p):
        if p.endswith("/"):
            return ("prefix", p.rstrip("/"))
        return ("exact", p)
    m = _DIRSEG_RE.match(p)
    if m:
        return ("dirseg", m.group(1))
    literal: list[str] = []
    for seg in p.split("/"):
        if has_magic(seg):
            break
        literal.append(seg)
    if literal:
        return ("prefix", "/".join(literal))
    tail, _ = _tails(p.rsplit("/", 1)[-1])
    if tail:
        rest = p.rsplit("/", 1)[-1]
        cut = len(rest) - tail
        return ("suffix", rest[cut:])
    return ("broad", "")


class RuleIndex:
    """Buckets of :class:`Entry`, built once per taxonomy, consulted per lookup."""

    def __init__(self) -> None:
        self.exact: dict[str, list[Entry]] = {}
        self.prefix: dict[str, list[Entry]] = {}
        self.dirseg: dict[str, list[Entry]] = {}
        self.suffix: dict[str, list[Entry]] = {}
        self.broad: list[Entry] = []
        self.size = 0

    def add(self, rule: Rule, pattern: str) -> None:
        clean = bare(pattern)
        entry = Entry(rule=rule, pattern=pattern, specificity=specificity(pattern),
                      compiled=_compile(pattern), dirish=clean.endswith("/**"),
                      dir_prefix=clean if clean.endswith("/") else "")
        kind, key = bucket_for(pattern)
        if kind == "broad":
            self.broad.append(entry)
        else:
            getattr(self, kind).setdefault(key, []).append(entry)
        self.size += 1

    def candidates(self, path: str) -> list[Entry]:
        """Every entry that could speak for `path`, in bucket-probe order, de-duplicated."""
        segs = path.split("/")
        out: list[Entry] = []
        seen: set[tuple[str, str]] = set()

        def take(entries):
            for entry in entries:
                token = (entry.rule.id, entry.pattern)
                if token not in seen:
                    seen.add(token)
                    out.append(entry)

        take(self.exact.get(path, ()))
        for i in range(len(segs)):                          # every ancestor dir, + ""
            take(self.prefix.get("/".join(segs[:i]), ()))
        base = segs[-1]
        # Every suffix of the basename, not just the dotted ones: a pattern's bucket key is
        # whatever literal text follows its last magic character, and for `*_test.go` that
        # is `_test.go`, which no dot-aligned probe would ever try. Suffixes are a handful
        # of dict probes per path segment — cheaper than one regex, which is the whole
        # point of the index.
        for i in range(len(base)):
            take(self.suffix.get(base[i:], ()))
        for seg in segs:
            take(self.dirseg.get(seg, ()))
        take(self.broad)
        return out

    def counts(self) -> dict[str, int]:
        return {"exact": len(self.exact), "prefix": len(self.prefix),
                "dirseg": len(self.dirseg), "suffix": len(self.suffix),
                "broad": len(self.broad), "entries": self.size}


def _compile(pattern: str):
    """Compile once, at build time. A bad pattern is a dead rule, never a raised error.

    `config lint` already refuses an unbalanced `{` at the config layer
    (`layer._check_globs`), so reaching here with a bad pattern means somebody built a
    `Rule` in code. Returning a never-matching pattern keeps one bad entry in a table from
    turning every classification into a traceback; the rule still shows up in `rules` and
    in the digest, so it is not invisible.
    """
    try:
        return globs.compile_glob(bare(pattern))
    except globs.BadPattern:
        return re.compile(r"(?!)")


# ---------------------------------------------------------------------------
# building rules from a resolved config
# ---------------------------------------------------------------------------
#: Which config keys feed the taxonomy, and what each one asserts. A key not in this table
#: is not consulted: `non_code.patterns` (`e2.8`) deliberately stays out, because "a
#: surface an operator must be told about" is a different claim from "what this file is" —
#: a CI workflow is `config` in this taxonomy and *also* non-code in `e2.8`'s sense.
CLASS_KEYS: dict[str, str | None] = {
    "noise.globs": None,                        # noise-only, supports `!`
    "test.globs": C.PathClass.TEST,             # e2.6's "what is a test" == class test
}

#: The open map a user asserts classes in: `taxonomy.classes.<class-name>`.
CLASS_MAP_KEY = "taxonomy.classes.<class-name>"


def rules_from_config(config, warnings: list[Warning]) -> list[Rule]:
    """Turn a resolved `Config` into rules, one per contributing layer.

    Every layer gets its own rules rather than the merged list, because precedence is a
    property of *layers* (module docstring, term 1) and the merged list has thrown that
    information away. The steps of a `Decision` keep each layer's own value, which is why
    this can say "`**/vendor/**` came from your user config at line 2" instead of "some
    layer said so".
    """
    rules: list[Rule] = []
    if config is None:
        return rules
    for key, cls in CLASS_KEYS.items():
        decision = config.decision(key)
        if decision is None:
            continue
        rules.extend(_rules_for(decision, cls, key, warnings, config))
    for concrete in config.keys():
        if S.abstract(concrete) != CLASS_MAP_KEY:
            continue
        name = concrete.rsplit(".", 1)[-1]
        decision = config.decision(concrete)
        if name == C.PathClass.OTHER:
            warnings.append(Warning(
                "taxonomy_class_unassertable",
                "%s asserts class `other`, which is what an unmatched path gets anyway"
                % concrete, source=decision.source if decision else "", key=concrete,
                suggestion="delete the entry, or name the class you actually mean"))
            continue
        if not C.assertable(name):
            warnings.append(Warning(
                "taxonomy_class_unknown", "%s names an unknown taxonomy class %r"
                                          % (concrete, name),
                source=decision.source if decision else "", key=concrete,
                suggestion="one of: %s" % ", ".join(C.ASSERTABLE_CLASSES)))
            continue
        rules.extend(_rules_for(decision, name, concrete, warnings, config))
    return rules


def _rules_for(decision, cls: str | None, key: str, warnings: list[Warning],
               config) -> list[Rule]:
    out: list[Rule] = []
    if decision is None:
        return out
    for step in decision.chain():
        if step.action == "suppressed":
            continue                                   # a repo file may not set this
        patterns = step.value if isinstance(step.value, list) else \
            ([step.value] if step.value else [])
        for index, raw in enumerate(patterns):
            pattern = str(raw)
            negated = is_negated(pattern)
            if negated and cls is not None:
                warnings.append(Warning(
                    "taxonomy_negation_ignored",
                    "%r in %s is negated, but a class list has nothing to re-promote it "
                    "to — use `!` in `noise.globs` to undo a noise glob" % (pattern, key),
                    source=step.source, key=key))
                continue
            try:
                globs.compile_glob(bare(pattern))
            except globs.BadPattern as exc:
                warnings.append(Warning("taxonomy_bad_glob", "%s contains an unusable glob: %s"
                                                  % (key, exc), source=step.source, key=key))
                continue
            rule = Rule(
                id="cfg.%s.%s#%d" % (step.layer, key, index),
                patterns=(pattern,),
                cls=None if negated else cls,
                # A `!` re-promotes; an entry in `noise.globs` demotes to "noise, reason not
                # one of the named categories" (`unknown`); an entry in a class list says
                # nothing about noise and inherits its class's default level.
                noise=(N.Noise.INFORMATIVE if negated else
                       (N.Noise.UNKNOWN if cls is None else None)),
                note=("re-promoted by a negation" if negated else ""),
                layer=step.layer, source=step.source or Layer.label(step.layer),
                origin="config", key=key, negated=negated)
            out.append(rule)
    return out


def table_digest(rules: list[Rule], builtins_version: int) -> str:
    """A digest of the *effective* table: built-ins plus every user rule.

    ADR-004 §5.5's rule is that a cache key may only contain what the payload actually
    reads, and a taxonomy payload reads the whole table. This is the value a caller can
    put in a key alongside `noise.builtins_version` — which is the declared config key
    that moves `taxonomy.path`'s digest today — so a user glob that changes a decision
    also changes the digest, and a re-ordered list that changes nothing does not.
    """
    h = hashlib.sha256()
    h.update(("v%d;" % builtins_version).encode())
    for rule in sorted(rules, key=lambda r: r.digest_token()):
        h.update((rule.digest_token() + "\n").encode())
    return h.hexdigest()[:16]
