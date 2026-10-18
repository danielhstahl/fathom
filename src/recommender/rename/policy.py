"""Every number that decides what this package is willing to assert, and where the value came from.

`e2.4`'s description asks for "explicit confidence" and for low-confidence pairs to be guesses. Both
are knobs, and a knob in a source file is a guess with an author — so each one is declared in `e0.8`'s
schema, resolves through the same five layers as every other key, and prints the layer that won. Two
of these numbers are **measured** rather than chosen, and say so in their `doc`:

```
rename.fact_bar    0.75   git's similarity index needed to call a pairing a fact
rename.guess_floor 0.15   the lowest-scoring guess the card will still show
```

`s4`'s measurement is why 0.75 and not the 0.50/0.60 git ships with: at the shipped default git
asserts the corpus's named decoy and still misses every rewrite below R050, and at 0.75 it ships
nothing false (precision 1.000 on 34 pairings, 0 wrong). `guess_floor` is not a precision decision at
all — no threshold separates the decoy from the true rewrites (`G46`-`G48`) — it is a *display*
budget, and it is graded on how many true pairings a bar buys per lie it lets in.

Everything else is a cost or a representation knob and is graded on cost:

| key | default | what it decides |
|---|---|---|
| `rename.fact_bar` | 0.75 | fact versus guess, measured |
| `rename.fact.allow_content` | false | whether our own similarity score may assert a fact at all |
| `rename.guess_floor` | 0.15 | the display floor for guesses |
| `rename.candidates.top_k` | 3 | destinations a source may claim, and sources a destination may claim |
| `rename.candidates.budget` | 4000 | scored pairs before MinHash blocking starts dropping candidates |
| `rename.candidates.copies_harder` | false | widen sources to every untouched path: O(repo), refused by default |
| `rename.copies` | true | allow a *modified* file to be a copy source |
| `rename.folds` | true | allow a *modified* file to be a merge destination |
| `rename.content.shingle_lines` | 3 | lines per shingle |
| `rename.content.normalise` | lines | `lines` (whitespace-insensitive) or `tokens` (formatter-insensitive) |
| `rename.content.min_shared_shingles` | 4 | shared shingles before a similarity is a pairing |
| `rename.card.rows` | 12 | rows before the card says how many more there are |

There is deliberately **no name-similarity weight**. `s4` ablated it: `hybrid` and `hybrid-no-name`
differ by a 0.15 path weight and emit 36 and 1 wrong pairings at the same bar, four of them on a
window where nothing moved at all (`G39`). Path similarity is still computed — it orders rows and it
is printed, because "same basename, different directory" is a fact a reviewer wants — it is just never
part of a score, so there is nothing to tune into the footgun.

Precedence is `e0.8`'s, unchanged: flags > env > repo `fathom.toml` > user config > built-in default.
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace

#: The one key this package does not own: `e1.6`'s rename threshold for the *diff pane*. The pairing
#: engine reads git below it (at `rename.guess_floor`) so it can label a guess with git's own number;
#: the two are different questions and the card says which bar a row was read under.
GIT_CANDIDATE_KEY = "rename.threshold"

#: Keys the pairing engine reads. Goes into the analyzer manifest (ADR-004 §5.1) so a change to any
#: of them invalidates cached pairings and nothing else.
CONFIG_KEYS: tuple[str, ...] = (
    GIT_CANDIDATE_KEY,
    "rename.fact_bar",
    "rename.fact.allow_content",
    "rename.guess_floor",
    "rename.candidates.top_k",
    "rename.candidates.budget",
    "rename.candidates.copies_harder",
    "rename.copies",
    "rename.folds",
    "rename.content.shingle_lines",
    "rename.content.normalise",
    "rename.content.min_shared_shingles",
    "rename.card.rows",
)


@dataclass(frozen=True)
class Knob:
    """One tunable number, with the built-in default and the reason it is that value."""

    name: str
    key: str
    default: object
    kind: str
    units: str = ""
    doc: str = ""
    measured: bool = False          # the value comes from a spike, not from taste


KNOBS: tuple[Knob, ...] = (
    Knob("fact_bar", "rename.fact_bar", 0.75, "number", "similarity",
         "git's similarity index needed before a pairing is a FACT. Measured on s4's corpus: at "
         "git's shipped 0.50 the engine asserts a named decoy and misses every rewrite below R050; "
         "at 0.75 it ships nothing false. A fact is the only tier a reviewer is asked to act on.",
         measured=True),
    Knob("allow_content", "rename.fact.allow_content", False, "boolean", "",
         "Allow our own content similarity to assert a FACT at or above the fact bar. Off by "
         "default: a fact should be re-checkable with `git log --follow`, and s4 measured git's "
         "percentage as badly calibrated (ECE 0.182) once the engine is allowed to pair anything, "
         "which is worse for a number a reviewer cannot reproduce at all."),
    Knob("guess_floor", "rename.guess_floor", 0.15, "number", "similarity",
         "The lowest-scoring guess the card prints. This is not a correctness bar — no threshold "
         "separates the corpus's decoy from its true rewrites — it is how much noise a reviewer is "
         "asked to skim. The shipped value is the measured knee: at 0.15 every claimable pair the "
         "engine can find is found, and going down to 0.10 buys two more (both above the claimable "
         "line) for three more rows and no fewer lies.", measured=True),
    Knob("top_k", "rename.candidates.top_k", 3, "integer", "pairs",
         "A source may claim its k best destinations and a destination its k best sources; a pair "
         "survives if it is in either list. This is what lets a 1-to-3 split be emitted at all: a "
         "1:1 assignment cannot name three destinations at any threshold."),
    Knob("budget", "rename.candidates.budget", 4000, "integer", "pairs",
         "Scored pairs per window before MinHash blocking starts dropping candidate pairs to hold "
         "the quadratic down. When it fires, the card says how many candidates were dropped — a "
         "stated partial, never a silent one."),
    Knob("copies_harder", "rename.candidates.copies_harder", False, "boolean", "",
         "Widen copy sources to every path in the base tree, which is the only way to see a copy of "
         "a file nobody touched in this commit. It is O(repo files), which `e2.4`'s own cost "
         "criterion rules out, and s4 measured what it buys: both real copies and two fabricated "
         "pairings on a window where nothing moved. Off, and when on the card prints the cost."),
    Knob("copies", "rename.copies", True, "boolean", "",
         "Let a file this window *modified* be a copy source. This keeps the cost at O(changed "
         "files) while still making a copy visible whenever the copy and the touched original are "
         "in the same window."),
    Knob("folds", "rename.folds", True, "boolean", "",
         "Let a file this window *modified* be a merge destination. Without this a fold into an "
         "existing module is invisible: the destination never appears among the additions, which is "
         "why git reports zero folds at any threshold (G9, G10)."),
    Knob("shingle_lines", "rename.content.shingle_lines", 3, "integer", "lines",
         "Lines per shingle. One line is a bag of lines, which scores every file sharing an import "
         "header as similar — that is the decoy failure; three needs a run of context. Wider and a "
         "moved-and-edited file stops matching at all."),
    Knob("normalise", "rename.content.normalise", "lines", "string", "mode",
         "`lines` folds whitespace only, so a formatter run still looks like the file it is and a "
         "reformat of a moved file is found; `tokens` runs e2.2's token normaliser first, which "
         "makes a pure reformat invisible between the two blobs and also erases the difference "
         "between two files that differ only in formatting."),
    Knob("min_shared_shingles", "rename.content.min_shared_shingles", 4, "integer", "shingles",
         "How many shingles two files must actually share before a similarity is a pairing. A "
         "four-line file sharing one shingle with another has a Jaccard of 1.00 and shares nothing "
         "worth calling provenance; without this floor, every four-line config file in a tree is a "
         "candidate copy of every other."),
    Knob("card_rows", "rename.card.rows", 12, "integer", "rows",
         "Rows the card prints before it names the remainder and the verb that lists it. A cap is "
         "not a truncation as long as the count survives."),
)

KNOB_INDEX: dict[str, Knob] = {knob.name: knob for knob in KNOBS}
KEY_TO_KNOB: dict[str, Knob] = {knob.key: knob for knob in KNOBS}

#: The keys whose value came off a measurement rather than off a judgement, printed as such.
MEASURED_KEYS: tuple[str, ...] = tuple(k.key for k in KNOBS if k.measured)


@dataclass
class Policy:
    """The effective pairing policy for one repository."""

    knobs: dict[str, object] = field(default_factory=dict)
    source: dict[str, str] = field(default_factory=dict)
    git_candidate_percent: int = 60
    git_candidate_source: str = "built-in default"
    config_digest: str = ""

    @classmethod
    def built_in(cls) -> "Policy":
        return cls(knobs={k.name: k.default for k in KNOBS}, git_candidate_percent=60)

    @classmethod
    def from_config(cls, config=None) -> "Policy":
        policy = cls()
        for knob in KNOBS:
            value, source = _lookup(config, knob.key, knob.default)
            policy.knobs[knob.name] = knob.default if value is None else _coerce(knob, value)
            policy.source[knob.name] = source
        value, source = _lookup(config, GIT_CANDIDATE_KEY, 60)
        policy.git_candidate_percent = int(value) if value is not None else 60
        policy.git_candidate_source = source
        try:
            policy.config_digest = config.digest_for("diff.rename_detect") if config else ""
        except Exception:                            # noqa: BLE001 — no manifest, no digest
            policy.config_digest = ""
        return policy

    # -- the questions ---------------------------------------------------------
    def knob(self, name: str) -> object:
        return self.knobs.get(name, KNOB_INDEX[name].default)

    def number(self, name: str) -> float:
        value = self.knob(name)
        return float(value) if isinstance(value, (int, float)) else float(KNOB_INDEX[name].default)

    def integer(self, name: str) -> int:
        return int(self.number(name))

    def flag(self, name: str) -> bool:
        return bool(self.knob(name))

    def text(self, name: str) -> str:
        return str(self.knob(name) or KNOB_INDEX[name].default)

    def source_of(self, name: str) -> str:
        return self.source.get(name, "built-in default")

    def with_knobs(self, source: str = "caller override", **names) -> "Policy":
        """A copy with these knobs overridden, each labelled with where the override came from."""
        clone = replace(self, knobs=dict(self.knobs), source=dict(self.source))
        for name, value in names.items():
            clone.knobs[name] = _coerce(KNOB_INDEX[name], value)
            clone.source[name] = "%s (%s=%s)" % (source, name, _format(value))
        return clone

    @property
    def fact_bar(self) -> float:
        return self.number("fact_bar")

    @property
    def guess_floor(self) -> float:
        return self.number("guess_floor")

    def git_candidate_bar(self) -> int:
        """What percentage to ask git for.

        Git is asked at the *lower* of the fact bar and the guess floor, because a guess is worth
        more with git's own number on it, and asking above the fact bar would make the fact tier
        the only thing the engine can ever see.
        """
        return max(1, min(100, int(round(min(self.fact_bar, self.guess_floor) * 100))))

    def describe(self) -> list[str]:
        out = ["rename pairing policy (highest precedence first)"]
        for knob in KNOBS:
            out.append("  %-22s %-10s %-40s %s" % (knob.name, _format(self.knob(knob.name)),
                                                   knob.key, self.source_of(knob.name)))
        out.append("  %-22s %-10s %-40s %s" % ("git_candidate_percent",
                                               self.git_candidate_percent, GIT_CANDIDATE_KEY,
                                               self.git_candidate_source))
        return out

    def as_dict(self) -> dict:
        return {"knobs": {k.name: {"value": self.knob(k.name), "key": k.key,
                                   "default": k.default, "units": k.units,
                                   "measured": k.measured,
                                   "source": self.source_of(k.name)} for k in KNOBS},
                "git_candidate_percent": self.git_candidate_percent,
                "git_candidate_source": self.git_candidate_source,
                "git_candidate_bar": self.git_candidate_bar(),
                "config_digest": self.config_digest}

    def limits(self) -> list[str]:
        """The standing cost and blindness statements — printed with any result, not hidden."""
        out = ["candidates are window-local: O(changed files), never the whole tree"]
        if not self.flag("copies_harder"):
            out.append("a copy of a file this window did not touch is not reportable: seeing it "
                       "costs O(repo files) (`rename.candidates.copies_harder`)")
        else:
            out.append("copies_harder is ON: candidate sources are every path in the base tree, "
                       "which is O(repo files) and slower by the size of the tree")
        if not self.flag("folds"):
            out.append("folds are OFF: a merge into a file that already exists cannot be reported")
        if not self.flag("copies"):
            out.append("copies are OFF: only a source deleted in this window can be a copy source")
        return out


def _format(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        return ("%.2f" % value).rstrip("0").rstrip(".")
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
    """`(value, source)` for one key, from a resolved `Config` or a bare mapping."""
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
