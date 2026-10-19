"""The typed root config: every key, its type, its default, its layer scope and its
merge strategy — **types only, no IO** (ADR-002 §4.5).

This module is the Python form of `fathom-core`'s `config` module. ADR-002 §4.5 splits
the config system along the IO line and then makes the split a rule with a lint
attached (`W1`):

    No crate below `fathom-cli` reads configuration, environment variables or files to
    find out how it should behave. The resolved root config is passed in as a value.

So this file imports nothing from `sources.py`/`layer.py`, never calls `open()`, never
reads `os.environ`, and never looks at the clock. `tests/test_config_contract.py`
enforces that literally by grepping the module's source. The one input that would
plausibly have needed a file read here — the per-size-class budget table of ADR-004 §11
— is *injected* into :func:`defaults` by the IO half, with a checked-in fallback per
key (see :data:`Key.fallback`).

Everything downstream of here is a function of data, which is what makes the config
usable in a cache key (ADR-004 §5.5) and what makes `fathom config explain` able to
answer "why is this the value" without re-reading anything.

## The five kinds of information carried per key

| field | why it exists |
|---|---|
| `kind` | what a value must be; feeds the JSON Schema and the `--set`/env coercer |
| `default` | the `defaults` layer's value (the answer when nothing else speaks) |
| `merge` | how a layer combines with the ones below it: `replace`, `extend`, `deep` |
| `scope` | which layers may set it at all: `any`, or `user` (never a cloned file) |
| `budget` | the `benchmarks/budgets.json` family whose value replaces `default` |

`scope` and `merge` are the two fields that carry real decisions, and both exist to
refuse a specific failure: `scope="user"` because a cloned repository must never be able
to turn on network egress or hand a token to a listener (ADR-001 §4.6, "repo-provided
config is hostile by default"), and `merge="extend"` because the alternative — repo
config *replacing* a user's noise-glob list — silently deletes a person's personal
configuration the first time they clone a project that adds one glob.
"""

from __future__ import annotations

import difflib
import re
from dataclasses import dataclass

from .errors import ConfigInvalid

#: Version of *this key layout*. A file may declare it (`meta.config_version`); a newer
#: declaration is refused rather than partially understood (see errors.ConfigVersionRejected).
CONFIG_VERSION = 1

#: Name of the per-repo config file. One name, one location rule (ADR-002 §4.5).
REPO_CONFIG_FILENAME = "fathom.toml"

#: Env prefix for the `env` layer.
ENV_PREFIX = "FATHOM_"

#: Separator between path segments inside an env var name: `FATHOM_CACHE__BUDGET__MIB`.
#: Two underscores, so a key containing a single `_` (`test_ratio`) stays unambiguous.
ENV_SEP = "__"


# ---------------------------------------------------------------------------
# layers
# ---------------------------------------------------------------------------
class Layer:
    """The five layers, numbered by *precedence*: lower number wins.

    `defaults` is "system defaults" in the ticket's phrasing: compiled-in values here,
    optionally replaced per-key by the budget table of ADR-004 §11 for the resolved size
    class. There is deliberately **no** `/etc/fathom/config.toml`: see
    [docs/concepts/config.md](../../../docs/concepts/config.md) §3.1 for why a
    root-owned, unversioned sixth layer was refused.
    """

    FLAG = "flag"
    ENV = "env"
    REPO = "repo"
    USER = "user"
    DEFAULTS = "defaults"

    #: highest precedence first — the order `explain` prints a chain in
    ORDER = (FLAG, ENV, REPO, USER, DEFAULTS)

    @classmethod
    def rank(cls, name: str) -> int:
        return cls.ORDER.index(name)

    @classmethod
    def beats(cls, winner: str, loser: str) -> bool:
        return cls.rank(winner) < cls.rank(loser)

    @classmethod
    def label(cls, name: str) -> str:
        return {
            cls.FLAG: "command-line flag",
            cls.ENV: "environment variable",
            cls.REPO: "repo fathom.toml",
            cls.USER: "user config",
            cls.DEFAULTS: "built-in default",
        }[name]


# ---------------------------------------------------------------------------
# key specification
# ---------------------------------------------------------------------------
#: Kinds, and the JSON Schema shape each produces. Kept small on purpose: a kind that
#: needs its own validation code path is a kind that will be validated differently in
#: the CLI and in the schema. Open maps are *not* separate kinds: a key path carrying a
#: `<placeholder>` segment says the value is whatever `kind` says, nested under a
#: pattern-named table, which keeps exactly one shape-generating code path.
KINDS = {
    "string": {"type": "string"},
    "path": {"type": "string"},
    "glob": {"type": "string"},
    "integer": {"type": "integer"},
    "number": {"type": "number"},
    "boolean": {"type": "boolean"},
    "string_list": {"type": "array", "items": {"type": "string"}, "uniqueItems": True},
    "glob_list": {"type": "array", "items": {"type": "string"}, "uniqueItems": True},
}

#: Segments that may appear as `<placeholder>` in a key path, and what they match.
#: These become `patternProperties` in the generated JSON Schema, which is what lets a
#: per-repo layer map (`[layers] ui = [...]`) be both open-ended and *checked*.
PLACEHOLDERS: dict[str, dict] = {
    "<layer-name>": {"pattern": r"^[a-z][a-z0-9_]*$",
                     "doc": "a layer name: lowercase, starts with a letter"},
    "<weight>": {"pattern": r"^[a-z][a-z0-9_]*$",
                 "doc": "a risk-weight name (`size`, `test_ratio`, `blast`, …)"},
    "<mode>": {"pattern": "^(?:global|queue|timeline|diff|structure|deps|ambient|review)$",
               "doc": "a keymap mode (the canonical table is ADR-006's, `e0.6`)"},
    "<binding>": {"pattern": r"^[A-Za-z0-9<>^+_,.\- ]{1,12}$",
                  "doc": "a key or key-sequence, e.g. `j`, `gg`, `<C-PageDown>`"},
    "<class-name>": {"pattern": r"^(?:source|test|docs|config|schema|migration|generated|"
                                 "vendored|lockfile|asset|binary|other)$",
                     "doc": "a path-taxonomy class (`reco-5vz`); `other` is refused because "
                            "asserting it is indistinguishable from matching nothing"},
    "<agent-id>": {"pattern": r"^[a-z][a-z0-9_-]*$",
                   "doc": "a known-agent id from the attribution registry (`e4.1`): the "
                          "name the label prints as the agent"},
}


@dataclass(frozen=True)
class Key:
    path: str
    kind: str
    doc: str
    default: object = None
    merge: str = "replace"          # replace | extend | deep
    scope: str = "any"              # any | user  (`user` may never come from a repo file)
    units: str = ""
    enum: tuple = ()
    pattern: str = ""
    item_pattern: str = ""
    minimum: float | None = None
    maximum: float | None = None
    secret: bool = False
    budget: str = ""                # budgets.json family, e.g. "cache.budget.mib"
    fallback: object = None         # default used when no budget table is injected
    ticket: str = ""                # the plan key that needs this key to exist
    analyzer: str = ""              # analyzer that declares it (ADR-004 §5.1), if any
    deprecated: str = ""

    def json_shape(self) -> dict:
        shape = dict(KINDS[self.kind])
        if self.enum:
            if self.kind.endswith("_list"):
                shape = {"type": "array", "items": {"enum": list(self.enum)},
                         "uniqueItems": True}
            else:
                shape = {"type": "string", "enum": list(self.enum)}
        if self.pattern:
            shape["pattern"] = self.pattern
        if self.item_pattern:
            shape = dict(shape)
            shape["items"] = {**shape.get("items", {}), "pattern": self.item_pattern}
        if self.minimum is not None:
            shape["minimum"] = self.minimum
        if self.maximum is not None:
            shape["maximum"] = self.maximum
        if self.secret:
            shape["x-fathom-secret"] = True
        return shape

    def open_segments(self) -> list[str]:
        return [s for s in self.segments() if s in PLACEHOLDERS]

    def segments(self) -> list[str]:
        return self.path.split(".")

    def is_open(self) -> bool:
        return any(seg in PLACEHOLDERS for seg in self.segments())

    def is_budget(self) -> bool:
        return bool(self.budget)

    def effective_default(self, budgets: dict | None = None) -> object:
        """The `defaults` layer's value for this key.

        Budget-linked keys take their value from the injected per-size-class table when
        one is supplied (the CLI half reads `benchmarks/budgets.json`; this module does
        not), and from :attr:`fallback` otherwise. `fallback` is checked against the
        budget file by a test rather than trusted.
        """
        if self.budget and budgets and self.budget in budgets:
            return budgets[self.budget]
        return self.fallback if self.is_budget() else self.default


#: Sections, in the order they are documented, with who owns the vocabulary inside them.
SECTIONS: dict[str, dict] = {
    "meta": {"title": "File metadata", "owner": "e0.8",
             "doc": "What the file says about itself, so a newer file is refused rather "
                    "than half-understood."},
    "paths": {"title": "Where things live", "owner": "e0.8 / ADR-004 §12",
              "doc": "The four paths Fathom writes to. Every relative path is resolved "
                     "against the repository root and must stay inside it."},
    "noise": {"title": "Noise policy", "owner": "e0.9 (`reco-5vz`)",
              "doc": "Globs for change that carries no reviewable information. "
                     "`e0.9` owns the built-in table; this is the user half of it."},
    "taxonomy": {"title": "Path taxonomy overrides", "owner": "e0.9 (`reco-5vz`)",
                 "doc": "Assert what a set of paths IS, relative to the built-in table: "
                        "one open map keyed by class. The engine and the built-ins are "
                        "`reco-5vz`'s; only the overrides live here."},
    "test": {"title": "Test surface", "owner": "e2.9 (`reco-6pl`)",
             "doc": "What counts as a test, which test-surface findings fire, and the "
                    "numbers that define 'new public logic, no new test'. The finding "
                    "policy is here rather than in `[detectors]` because these findings "
                    "are one definition with knobs, not a detector registry."},
    "non_code": {"title": "Non-code surface", "owner": "e2.8 (`reco-bjd`)",
                 "doc": "Config, env, flags, migrations and CI — the surfaces a reviewer "
                        "must be told about even though they are not code."},
    "structure": {"title": "Folder structure delta", "owner": "e2.3 (`reco-5fn`)",
                  "doc": "Where an episode's code landed. The thresholds that decide when a "
                         "set of pairings is a moved directory, a split or a flattening, the "
                         "weights behind the structure-churn score, and one switch per "
                         "finding kind. `fathom structure definition` prints these numbers as "
                         "sentences."},
    "graph": {"title": "Graph inputs", "owner": "e3.2 (`reco-8s4`)",
              "doc": "Directories excluded from the dependency and structure graph."},
    "layers": {"title": "Layering rules", "owner": "ADR-004 §5.1 / e3.1",
               "doc": "The declared intended architecture: named layers, each a list of "
                      "path prefixes. An open map, keyed by layer name."},
    "deps": {"title": "Dependencies", "owner": "e2.5 (`reco-j83`) / e2.7 (`reco-yxi`)",
             "doc": "Which ecosystems' manifests and lockfiles are parsed, and what the "
                    "dependency-risk answer does with them: the advisory cache and its "
                    "freshness bars, the licence matrix, and how hard provenance looks for "
                    "the episode that asked for a package. Consent to reach the network is "
                    "not here - it is `privacy.advisories`, because a cloned repository must "
                    "not be able to grant it."},
    "risk": {"title": "Risk weights and threshold", "owner": "e2.9 / e2.10",
             "doc": "The ranking knobs. Changing these changes `risk.score` and "
                    "`delta.compose` and nothing else (ADR-004 §6)."},
    "detectors": {"title": "Detectors", "owner": "e2.7",
                  "doc": "Which finding detectors run."},
    "blast": {"title": "Blast radius", "owner": "e2.10",
              "doc": "How far the blast-radius walk goes before it declares a partial, the "
                     "confidence attached to each basis, and the path-heuristic weights that "
                     "decide what counts as reached."},
    "card": {"title": "Episode summary card", "owner": "e2.10 (`reco-4ey`)",
             "doc": "The one-screen answer per episode. Row caps, how loud a finding has to be "
                    "to earn an attention row, and one switch per section - because a "
                    "repository narrowing its own card is a different thing from a card "
                    "quietly losing a question, and the card prints which happened."},
    "rename": {"title": "Rename detection and pairing", "owner": "e1.6 / e2.4 (`reco-a0v`)",
               "doc": "Two questions with two bars. `rename.threshold` is what the diff pane "
                      "prints as a rename; the `fact_bar`/`guess_floor` pair decides what the "
                      "pairing engine asserts, what it labels a guess, and what it costs."},
    "diff": {"title": "Diff rendering", "owner": "e1.6",
             "doc": "Per-file diff knobs."},
    "episode": {"title": "Episode segmentation", "owner": "ADR-003 / e1.3",
                "doc": "Boundary sources in priority order, and the idle gap that ends an "
                       "episode when nothing better is available."},
    "attribution": {"title": "Authorship attribution", "owner": "ADR-003 §8 / e4.1 (`reco-w7q`)",
                    "doc": "Who wrote a commit: the registry of known agents, the marker "
                           "table, and the confidence bar below which Fathom prints `?` "
                           "instead of guessing. Every value here is handed to the "
                           "classifier as data — no crate below the CLI opens this file."},
    "intent": {"title": "Intent capture", "owner": "ADR-003 §8 / e4.2 (`reco-c54`)",
               "doc": "What an episode was *for*: the ladder that answers that question "
                      "(the harness prompt, the `Agent-Intent:` trailer, a `Task:` line, a "
                      "prompt digest) and how much of the answer is kept. The ladder's ORDER "
                      "is not configurable, only its membership — an intent a user reordered "
                      "is an intent nobody can explain, which is the same refusal `[episode] "
                      "boundaries` and `[attribution]` make."},
    "ingest": {"title": "Ingest limits", "owner": "e1.2",
               "doc": "The size cap: a bigger blob is recorded as oversized, never slurped."},
    "cache": {"title": "Cache caps", "owner": "ADR-004 §11",
              "doc": "The class-R budget. Defaults come from the size class, not from a "
                     "magic number."},
    "store": {"title": "Store caps", "owner": "ADR-004 §7, §11",
              "doc": "WAL ceiling and the soft store-size ceiling `doctor` warns at."},
    "rss": {"title": "Memory caps", "owner": "ADR-004 §11.5",
            "doc": "The RSS budgets. Per ADR-004 §11.8 these are *never* scaled by a "
                   "machine factor, so the only knob is the size class."},
    "write": {"title": "Write batching", "owner": "ADR-004 §7.3",
              "doc": "Batching knobs for the single writer."},
    "prune": {"title": "Pruning", "owner": "ADR-004 §8.1", "doc": "Prune batch size."},
    "rebuild": {"title": "Rebuild safety", "owner": "ADR-004 §8.6",
                "doc": "Free-disk-ratio required before a rebuild starts."},
    "retention": {"title": "Retention windows", "owner": "ADR-004 §8.2, ADR-005 §6.3",
                  "doc": "Windows on the live epoch. ADR-004 §8.2 states these numbers "
                         "as `fathom.toml`'s `[retention]` defaults; they are restated "
                         "here from that table and a test refuses drift."},
    "harness": {"title": "Harness ingest", "owner": "ADR-005 §6.3",
                "doc": "The per-session ingest budget that quarantines a runaway "
                       "hook stream."},
    "keys": {"title": "Keymap overrides", "owner": "ADR-006 / e5.4",
             "doc": "Per-mode user overrides **only**. The canonical binding table is "
                    "ADR-006's; conflict detection and the cheatsheet are `e5.4`'s."},
    "mcp": {"title": "MCP server", "owner": "ADR-005 §4, ADR-002 §6.3",
            "doc": "Transport and bind address. The token lives here and is user-scoped "
                   "and redacted: a listener credential from a cloned repo is a gift."},
    "privacy": {"title": "Network consent", "owner": "ADR-001 §7.5, ADR-002 §6.3",
                "doc": "The runtime half of the no-network-by-default claim. Every key "
                       "here is user-scoped: a clone may not consent on your behalf."},
    "query": {"title": "Query interrupts", "owner": "ADR-004 §7.6, §11",
              "doc": "The interrupt budget for one store call on the keystroke path. "
                     "Exceeding it is a stated partial, not a hang."},
    "watch": {"title": "Watch mode", "owner": "e1.9 (`reco-949`)",
              "doc": "What share of one core `fathom watch` may spend while work is flowing. "
                     "Past it the poll stretches toward its ceiling and says so, because a "
                     "watcher that overruns the machine it is watching is the failure this "
                     "ticket exists to avoid."},
    "ratchet": {"title": "Violation ratchet", "owner": "ADR-004 §5.1 / e3.10 (`reco-5ht`)",
                "doc": "The baselined pre-existing violations graph.cycles reads. ADR-004 §5.1 "
                       "declares `ratchet.` as an analyzer input, so the subtree exists here; "
                       "`e3.10` may extend it, and the digest test refuses a silent absence."},
    "repo": {"title": "Repository facts", "owner": "ADR-004 §11.1",
             "doc": "The size class the budget defaults are chosen from."},
}

#: A glob item may be negated with a leading `!` (ADR-009/e0.9 demotes with it).
GLOB_ITEM = r"^!?[^\x00]+$"


def _k(**kw) -> Key:
    return Key(**kw)


KEY_LIST: list[Key] = [
    # --- meta ---------------------------------------------------------------
    _k(path="meta.config_version", kind="integer", default=CONFIG_VERSION,
       minimum=1, maximum=CONFIG_VERSION, units="version",
       doc="Config format this file was written for. A value greater than the running "
           "binary's is an error, never a silent partial read.",
       ticket="e0.8"),
    # --- paths --------------------------------------------------------------
    _k(path="paths.store", kind="path", default=".fathom/store.sqlite3",
       doc="SQLite index + analysis cache. A relative path is resolved against the "
           "repository root; a value that escapes the root from a *repo* file is refused.",
       pattern=r"^[^\x00]+$", ticket="e1.1", analyzer=""),
    _k(path="paths.objects", kind="path", default=".fathom/objects",
       doc="Content-addressed object directory for payloads too large to inline.",
       pattern=r"^[^\x00]+$", ticket="e1.2"),
    _k(path="paths.events", kind="path", default=".fathom/events.jsonl",
       doc="Append-only harness event stream the hook adapters write (ADR-005 §2).",
       pattern=r"^[^\x00]+$", ticket="e6.1"),
    _k(path="paths.export", kind="path", default=".fathom/export",
       doc="Where `fathom export` writes review artefacts.", pattern=r"^[^\x00]+$",
       ticket="e5.7"),
    # --- noise / taxonomy inputs (e0.9 consumes these, does not own them) ----
    _k(path="noise.globs", kind="glob_list", merge="extend",
       default=["**/dist/**", "**/*.snap"], item_pattern=GLOB_ITEM,
       doc="Path globs treated as noise. `extend`: a repo adds to your list instead of "
           "replacing it. A leading `!` re-promotes a path a broader glob demoted.",
       analyzer="taxonomy.path", ticket="e0.9"),
    _k(path="taxonomy.classes.<class-name>", kind="glob_list", merge="extend", default={},
       item_pattern=GLOB_ITEM,
       doc="One entry per taxonomy class: globs that assert paths ARE that class, "
           "outranking the built-in table because a user layer beats the defaults layer. "
           "`generated = [\"src/gen/**\"]` demotes hand-written-looking code; "
           "`source = [\"vendor/our-fork/**\"]` promotes vendored-looking code. "
           "`recommender/taxonomy/builtins.py` owns the built-ins; this key only overrides "
           "them, which is what `fathom explain <path>` prints as the winning layer.",
       analyzer="taxonomy.path", ticket="e0.9"),
    _k(path="noise.builtins_version", kind="integer", default=1, minimum=1,
       units="version",
       doc="Version of the *built-in* noise table e0.9 ships. Declared so a taxonomy "
           "table bump invalidates the analyzers that read noise (ADR-004 §6).",
       analyzer="taxonomy.path", ticket="e0.9"),
    # --- noise suppression (e2.2 owns these; `reco-zqf`) ---------------------
    # One boolean per suppression rule, because the thesis makes folding a *policy* (§7:
    # "never hide silently") and a policy has to be answerable to the person it hides from.
    # `last` merge, not `extend`: turning a rule off is the whole point of the key, and a
    # merge that could only ever add `true` would let a user layer undo a repository's "no".
    _k(path="noise.suppress.reformat", kind="boolean", default=True,
       doc="Fold a file pair whose token streams are identical — a rustfmt/prettier/gofmt "
           "re-layout, not a change. The rule only ever applies to a pair the token model "
           "can read; an unreadable pair is never folded. The fold line names the formatter.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="noise.suppress.generated_churn", kind="boolean", default=True,
       doc="Fold a generated file whose diff is a stamp line, a re-order of generated "
           "entries, or a whole-file regeneration. Deliberately narrower than 'the path is "
           "generated': a small edit inside a generated file stays visible, because that is "
           "the case where a person edited the generator's output by hand and wants it seen.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="noise.suppress.lockfile", kind="boolean", default=True,
       doc="Fold a lockfile to one counted line ('3 dependencies upgraded'). The rule stands "
           "aside when the declaring manifest moved too — then the lockfile diff is the "
           "dependency decision and is shown.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="noise.suppress.vendored", kind="boolean", default=True,
       doc="Fold vendored code (the `vendored` taxonomy class, `e0.9`). A person did not "
           "write it and cannot review it here; `fathom config explain --path` says which "
           "assertion made a path vendored.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="noise.suppress.user_glob", kind="boolean", default=True,
       doc="Fold a path the layers put in `noise.globs`. A repository adding a glob is "
           "asking for its own noise to be folded, not for it to be hidden, so the fold "
           "quotes the glob that caught the path.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="noise.collapse.noise_only", kind="boolean", default=True,
       doc="When *every* file in an episode folded, collapse the episode to one line. This "
           "is the 'a reformat-only commit is one line' behaviour, and it is a separate "
           "switch so a reviewer can keep per-file fold lines while refusing whole-episode "
           "collapses.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="noise.keep", kind="glob_list", merge="extend", default=[],
       item_pattern=GLOB_ITEM,
       doc="Paths suppression must never fold, whatever the rules say — the reviewer's "
           "'always show me this file' list. Beats every rule, including a repository's own "
           "assertion, because the only thing that outranks 'show it' is 'show it'.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="noise.expand", kind="string", default="b",
       pattern=r"^[A-Za-z0-9<>^+_,.\- ]{1,12}$",
       doc="The key that shows a folded change again (AC 4). Configurable because every "
           "other key here is configurable, and because the fold lines are *rendered* from "
           "this value — a fold that said `[press b]` after you rebind it would be a lie.",
       analyzer="noise.suppress", ticket="e2.2"),
    _k(path="test.globs", kind="glob_list", merge="extend",
       default=["tests/**", "**/test_*.py"], item_pattern=GLOB_ITEM,
       doc="Paths counted as test surface.", analyzer="test.surface", ticket="e2.6"),
    # --- test surface (e2.9 owns these; `reco-6pl`) -------------------------
    # One switch per finding, then the numbers that define "new public logic, no new
    # test". The ticket's acceptance criterion is that the definition is *documented and
    # tunable rather than a guess*, and a number in a source file is a guess with an
    # author: these are declared here so a repository can disagree with one in
    # `fathom.toml`, have it layer, and have the card quote the layer that won.
    _k(path="test.findings.test_removed", kind="boolean", default=True,
       doc="Report a named test case that existed in the old revision and is gone. A "
           "deletion is a first-class finding with the case name and its line, including "
           "the cases lost because the whole file went.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.findings.test_file_removed", kind="boolean", default=True,
       doc="Also report the file-level row when a test file is deleted, beside the "
           "case-level rows for the tests inside it.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.findings.test_skipped", kind="boolean", default=True,
       doc="Report a case that used to run and is now `#[ignore]`, `@pytest.mark.skip`, "
           "`xfail`, `t.Skip`, `it.skip` or `@Disabled`. A skipped test still exists and "
           "proves nothing, so it shares the channel a deletion gets.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.findings.assertion_weakened", kind="boolean", default=True,
       doc="Report an exact assertion replaced by a presence check on the same subject "
           "(`assert_eq!(x, 42)` → `assert!(x.is_ok())`, `expect(x).toBe(3)` → "
           "`.toBeDefined()`). The comparison is subject-to-subject, so it is about the "
           "same value both times.", analyzer="test.surface", ticket="e2.9"),
    _k(path="test.findings.assertion_removed", kind="boolean", default=True,
       doc="Report a test case that survived but lost exact assertions and gained none.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.findings.untested_public", kind="boolean", default=True,
       doc="Report new public logic that no test surface in the episode covers. What "
           "'public', 'logic' and 'covers' mean is the `test.untested.*` block below, not "
           "a hunch.", analyzer="test.surface", ticket="e2.9"),
    _k(path="test.findings.focused_test", kind="boolean", default=True,
       doc="Report a committed `it.only`/`fit`/`fdescribe`: every other test in that file "
           "is silently not running.", analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.min_body_lines", kind="integer", default=4, minimum=0,
       units="lines",
       doc="How many non-blank body lines a new public symbol needs before the episode is "
           "said to have added logic. Below it a one-line re-export helper raises the "
           "finding; above it a small real function does not.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.require_logic", kind="boolean", default=True,
       doc="Only count a symbol as logic if its body calls, assigns, branches or returns. "
           "Off means a new `pub struct` or constant can raise the finding, which is what "
           "a repository that reviews schemas as API surface may want.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.visibility", kind="string", default="public",
       enum=("public", "crate"), units="visibility",
       doc="Which Rust visibilities count as public: `public` is plain `pub` only, `crate` "
           "also counts `pub(crate)`. Other languages have no equivalent, and the default "
           "refuses to guess that a crate-internal item owed the reviewer a test.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.scope", kind="string", default="episode", units="scope",
       enum=("episode", "repo"),
       doc="`episode`: a covering test must be in this episode — the only thing measurable "
           "from a diff. `repo`: a symbol the repository's existing tests already mention "
           "is not flagged; needs the index to supply those mentions, and the card says so "
           "when it could not.", analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.name_overlap", kind="integer", default=1, minimum=0,
       units="tokens",
       doc="Name tokens a new test case must share with a new symbol to count as covering "
           "it (`load_snapshot` ↔ `TestSnapshotLoading` share `snapshot`).",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.path_overlap", kind="integer", default=1, minimum=0,
       units="tokens",
       doc="Path-name tokens a changed test file must share with the source file "
           "(`src/store/load.rs` ↔ `tests/store_load_test.rs` share `store`, `load`). For "
           "repositories whose test layout is not worth encoding in `test.globs`.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.mention_in_test", kind="boolean", default=True,
       doc="If a new symbol's name appears in lines the episode added to a test file, "
           "treat it as covered. The loosest of the three coverage tests, and the only one "
           "that catches a new function asserted inside a pre-existing test.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.untested.exclude", kind="glob_list", merge="extend", default=[],
       item_pattern=GLOB_ITEM,
       doc="Paths the untested-public finding never fires for: example code, benches, "
           "generated helpers. `extend`, so a repository adds to the list.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.proxy.confidence.parsed", kind="number", default=0.9, minimum=0.0,
       maximum=1.0, units="confidence",
       doc="Stated confidence when every test surface in the episode was parsed into named "
           "cases. Printed on the card, never left implicit.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.proxy.confidence.partial", kind="number", default=0.6, minimum=0.0,
       maximum=1.0, units="confidence",
       doc="Stated confidence when some test surface had no parser or a parser refused it: "
           "the case names are partly known, so the counts are a lower bound.",
       analyzer="test.surface", ticket="e2.9"),
    _k(path="test.proxy.confidence.lines_only", kind="number", default=0.3, minimum=0.0,
       maximum=1.0, units="confidence",
       doc="Stated confidence when no test framework was found and the only honest number "
           "is a line count. Deliberately low: a line count correlates with test effort and "
           "does not measure it.", analyzer="test.surface", ticket="e2.9"),
    _k(path="test.card.max_findings", kind="integer", default=12, minimum=1, units="rows",
       doc="How many finding rows the card prints before it says how many more there are "
           "and which verb lists them. A cap that names the remainder, never a truncation "
           "without a trace.", analyzer="test.surface", ticket="e2.9"),
    _k(path="non_code.patterns", kind="glob_list", merge="extend",
       default=["**/migrations/**", ".github/workflows/**"], item_pattern=GLOB_ITEM,
       doc="Non-code surfaces a reviewer must be told about: config, env, flags, "
           "migrations and CI.", analyzer="non_code.surface", ticket="e2.8"),
    # --- non-code surfaces (e2.8 owns these; `reco-bjd`) --------------------
    # One switch per finding, then the numbers that decide what counts as a new
    # secret, a destructive statement, an escalation or a floating base image. The
    # ticket standard is that the definition is documented and tunable rather than a
    # guess, and `fathom noncode definition` prints these numbers as sentences so a
    # card and a config file cannot drift from each other.
    _k(path="non_code.findings.env_added", kind="boolean", default=True,
       doc="A variable this episode introduces, either declared in an env file or read by new "
           "code. The process environment is not in the repository, so nothing about the diff "
           "makes it exist in production.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.env_removed", kind="boolean", default=True,
       doc="A variable that is no longer declared or read. Retiring it is the operator's job: a "
           "value left in a secret store with nothing reading it is a stale credential with an "
           "expiry nobody watches.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.env_declared_unread", kind="boolean", default=True,
       doc="Declared in `.env.example` (or a ConfigMap, compose block or workflow `env:`) with no "
           "read found for it. Either the code that reads it has not landed, or the declaration is "
           "theatre and the operator will set a value that changes nothing.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.env_stale_reference", kind="boolean", default=True,
       doc="The declaration is gone and a reference is not. This is the rename that was only half "
           "done: the process starts, reads nothing, and fails at the first request that needed it "
           "rather than at boot.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.env_renamed", kind="boolean", default=True,
       doc="One variable removed and a similar one added in the same episode, with the old name "
           "still referenced. Renames are the failure mode of a diff-shaped tool: both halves look "
           "harmless alone.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.env_value_committed", kind="boolean", default=True,
       doc="A non-placeholder value for a secret-shaped name landed in a committed file. The value "
           "is now in every clone and every object the history holds.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.flag_added", kind="boolean", default=True,
       doc="A new flag with the default value the code will see if the provider never answers. The "
           "default is the rollout plan whether the author meant it as one or not.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.flag_removed", kind="boolean", default=True,
       doc="A flag the code no longer reads, or a declaration that went while reads remained. Flag "
           "rows in a provider outlive the code that read them and confuse whoever inherits the "
           "panel.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.flag_default_changed", kind="boolean", default=True,
       doc="The default flipped. When the default is what ships, this diff *is* the rollout, and "
           "it happens to 100 % of traffic at the moment the deploy finishes.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.flag_read_no_default", kind="boolean", default=True,
       doc="The call site supplies no fallback, so the provider is on the request path: a missing "
           "flag or a provider outage becomes the caller's problem.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.migration_added", kind="boolean", default=True,
       doc="An ordered, execute-once change to durable state. It runs against a database that is "
           "still serving traffic, in an order the deploy has to get right.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.migration_forward_only", kind="boolean", default=True,
       doc="A forward path with no down path: no `-- +goose Down`, no `def down`, no `.down.sql`, "
           "no undo script. A rollback of the release is now a manual repair with a different "
           "author.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.migration_destructive", kind="boolean", default=True,
       doc="A `DROP`, `TRUNCATE`, `SET NOT NULL`, type change or rename in the forward path. These "
           "take locks, and some of them cannot be undone by anything.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.migration_with_model", kind="boolean", default=True,
       doc="The schema file and the code that reads it move together, which is correct and also an "
           "ordering problem: one of the two will be true before the other.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.deploy_coordination", kind="boolean", default=True,
       doc="The episode changes only durable-state surface: nothing here is safe to ship on the "
           "normal path, because there is no code for the migration to be compatible with.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.ci_secret_new", kind="boolean", default=True,
       doc="A workflow now reads `${{ secrets.X }}`. The build fails until somebody types that "
           "name into the repository, organisation or environment secrets — a fact nobody learns "
           "until a red tick.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.ci_permission_escalation", kind="boolean", default=True,
       doc="A `permissions:` block gained a write scope, or `write-all`. The token the job holds "
           "is now worth more than it was, and it is handed to every step in that job.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.ci_oidc_changed", kind="boolean", default=True,
       doc="`id-token: write`, a role ARN, an `environment:` binding or a vault login changed: the "
           "workflow can now mint an identity a cloud provider trusts. This is an access review, "
           "not a code review.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.ci_runner_changed", kind="boolean", default=True,
       doc="A job moved onto a runner this project owns, or onto a new label. Everything the job "
           "can see — network, cache, credentials on the host — changed with it.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.ci_trigger_main_push", kind="boolean", default=True,
       doc="A `push` trigger that includes the default branch appeared, or a job gained one. "
           "Whatever that workflow does — publish, deploy, sign — now happens on merge.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.iac_resource_added", kind="boolean", default=True,
       doc="A managed resource appeared in a plan. It costs money, has a name that must be "
           "globally unique, and will outlive the branch.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.iac_resource_removed", kind="boolean", default=True,
       doc="A managed resource disappeared from the configuration, which in every IaC tool means "
           "*destroy it*. For a bucket, a table or a database that is the data, not the "
           "infrastructure.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.iac_policy_widened", kind="boolean", default=True,
       doc="A new line widens an IAM policy, opens an ingress to the internet, or makes an "
           "instance public. The blast radius of this diff is not the repository.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.container_base_changed", kind="boolean", default=True,
       doc="The `FROM` moved: a different vendor, a different major, or a different digest. Every "
           "binary in the image was built against the old one.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.container_base_unpinned", kind="boolean", default=True,
       doc="No digest, or a tag that floats (`latest`, or no tag at all). The image you build next "
           "week is not the image you reviewed today.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.container_supply_chain", kind="boolean", default=True,
       doc="A `curl … | sh` in a Dockerfile: the build's reproducibility and the reviewer's threat "
           "model both now include whatever that server returns at build time.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.findings.container_privilege_changed", kind="boolean", default=True,
       doc="`USER root` added, the last non-root `USER` removed, `--privileged` or a capability "
           "granted. A container escape is now a root escape.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.env.declaration_globs", kind="glob_list", merge="extend", default=['.env.example', '.env.sample', '.env.template', '.env.default', '.env.dist', '**/.env.example', '**/.env.sample', '**/.env.template', '**/*.env.example', '**/*.env.sample', 'config/env.example', 'docs/env.example'],
       doc="Files whose whole job is to declare environment variables (`.env.example` and "
           "friends). A key added here is a key an operator must set; a key added only in code is "
           "the same finding with a different location.",
       item_pattern=GLOB_ITEM, analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.env.committed_globs", kind="glob_list", merge="extend", default=['.env', '.env.local', '.env.production', '**/.env', '**/.env.production', '**/.env.staging', 'config/env.ini', '**/secrets.yaml', '**/secrets.yml'],
       doc="Files where a *value* is a real value rather than documentation. A `.env` in a "
           "repository is usually an accident; a secret-shaped name in one is a rotation.",
       item_pattern=GLOB_ITEM, analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.env.placeholder_values", kind="string_list", merge="extend", default=['', 'changeme', 'change-me', 'change_me', 'your-value-here', 'your_value_here', 'your-secret-here', 'xxx', 'xxxx', 'xxxxx', '<value>', '<secret>', 'placeholder', 'sample', 'example', 'examples', 'test', 'dummy', 'todo', 'none', 'null', 'true', 'false', '0', '1', 'secret', 'password', 'admin', 'foo', 'bar', 'undefined', 'REPLACE_ME', 'insert-here', 'redacted'],
       doc="Values that read as documentation (`changeme`, `<secret>`, empty). Compared case- "
           "insensitively: this list is what keeps the committed-value finding from firing on the "
           "`.env` in every README.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.env.secret_name_pattern", kind="string", default='(?i)(^|_)(secret|token|password|passwd|pwd|api[_-]?key|access[_-]?key|private[_-]?key|credential|auth[_-]?key|dsn|_key|_cert|_certificate)($|_)|(^|_)(aws|gcp|azure|oidc)(_|-)?(key|secret|token|credential)',
       doc="Which variable names mean 'belongs in a secret store'. Deliberately a pattern, not a "
           "list: the useful part is the shape (`_KEY`, `TOKEN`, `DSN`), and every company has "
           "names the shape catches that a list does not.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.env.reference_scope", kind="string", default='episode', enum=("episode", "repo"),
       doc="`episode`: 'nothing reads this' is claimed about the files in this episode only, and "
           "the finding says so at a lower confidence. `repo`: the caller supplied the "
           "repository's full reference set, which is the only basis for the confident version of "
           "the claim — and the card says out loud when `repo` was asked for and not given.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.env.rename_similarity", kind="number", default=0.6, minimum=0.0, maximum=1.0,
       doc="Name-token similarity at which a removed variable and an added one are called one "
           "rename (`DATABASE_URL` → `PG_DATABASE_URL` shares 2 of 3 tokens). Low values merge "
           "unrelated removals into a rename; high values leave a half-done rename as two "
           "harmless-looking rows.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.flags.globs", kind="glob_list", merge="extend", default=['flags.yaml', 'flags.yml', 'flags.json', 'feature-flags.yaml', 'feature-flags.json', '**/flags.yaml', '**/flags.yml', '**/flags.json', '**/features.yaml', '**/features.json', '**/featureflags.json', '**/flip-flop.json', '**/unleash-toggles.json', 'config/flags/**', '**/config/flags/**', '**/feature_flags.py', '**/flags.php'],
       doc="Files that declare flags with their defaults. Code that *reads* a flag is scanned "
           "wherever it lives, so this list only decides where defaults come from.",
       item_pattern=GLOB_ITEM, analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.flags.require_default", kind="boolean", default=True,
       doc="Report a flag read that supplies no fallback. Off for a repository that runs its "
           "provider on the same node and treats a missing flag as a bug in the deploy.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.migration.globs", kind="glob_list", merge="extend", default=['**/migrations/**', '**/migration/**', 'db/migrate/**', 'db/migrations/**', '**/alembic/versions/**', '**/migrations/versions/**', 'prisma/migrations/**', '**/flyway/sql/**', 'src/main/resources/db/migration/**', '**/migrations/**.sql', '**/migrations/**.up.sql', 'resource/migrations/**'],
       doc="Migration layouts. The `e0.9`/`e2.1` taxonomy class `migration` is consulted first and "
           "these only add to it, so a repository with an unusual directory says it once.",
       item_pattern=GLOB_ITEM, analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.migration.model_globs", kind="glob_list", merge="extend", default=['**/models.py', '**/models/**.py', '**/model.py', 'db/schema.rb', 'db/schema.sql', 'app/models/**', '**/entities/**.java', '**/Entity/**.php', '**/*.prisma', '**/schema.sql', '**/models/**.go', 'internal/model/**.go', '**/Models/**.cs', '**/migrations/**/schema.rs'],
       doc="The model half of 'the migration and the model moved together': ORM definitions, "
           "`db/schema.rb`, `*.prisma`. Narrow on purpose — a Java package called `model` that "
           "moves without a migration is not a schema change.",
       item_pattern=GLOB_ITEM, analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.migration.destructive_ops", kind="string_list", merge="extend", default=['drop table', 'drop column', 'drop index', 'drop constraint', 'drop database', 'truncate table', 'truncate ', 'set not null', 'alter column', 'modify column', 'rename to', 'delete from', 'update ', 'alter type', 'drop materialized view', 'drop_table', 'remove_column', 'remove_index', 'change_column_null', 'delete_all', 'destroy_all', 'execute '],
       doc="Statements treated as destroying or rewriting data in a forward path. Matched against "
           "statement text, case-insensitively, and extendable per repository.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.ci.default_branches", kind="string_list", merge="extend", default=['main', 'master'],
       doc="Which branch names count as 'the default branch' when deciding whether a trigger means "
           "every merge runs this job.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.ci.escalating_permissions", kind="string_list", merge="extend", default=['contents:write', 'actions:write', 'security-events:write', 'id-token:write', 'pull-requests:write', 'deployments:write', 'packages:write', 'statuses:write', 'checks:write', 'issues:write', 'discussions:write', 'pages:write', 'organization-secrets:write', 'organization-administration:write', 'write-all'],
       doc="`permissions:` moves that are escalations, in `scope:access` spelling. `write-all` is "
           "in the list because it is the escalation that does not name what it escalates.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.iac.globs", kind="glob_list", merge="extend", default=['**/*.tf', '**/*.tfvars', '**/*.hcl', 'cloudformation*.y*ml', '**/cloudformation*.y*ml', '**/cfn-*.y*ml', 'k8s/**.y*ml', '**/k8s/**.y*ml', '**/kubernetes/**.y*ml', '**/manifests/**.y*ml', '**/helm/**/values.y*ml', '**/Chart.yaml', 'ansible/**.y*ml', '**/*.stack.yaml', 'Pulumi.*.yaml', '**/*.cue'],
       doc="Infrastructure-as-code files: Terraform, CloudFormation, Kubernetes manifests, Helm "
           "values, Pulumi, Ansible. A manifest with `kind:` is also detected by content, because "
           "nothing about a path says a YAML file is a Deployment.",
       item_pattern=GLOB_ITEM, analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.iac.widened_patterns", kind="string_list", merge="extend", default=['"*"', '"arn:aws:iam::aws:policy/AdministratorAccess"', '"0.0.0.0/0"', '::/0', '"Principal": "*"', 'associate_public_ip_address = true', 'cidr_blocks = ["0.0.0.0/0"]', 'Action = ["*"]', '--privileged'],
       doc="Text that widens an infrastructure exposure when it appears on an added line: a "
           "wildcard principal or action, a default route from anywhere, a public address.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.container.require_digest_pin", kind="boolean", default=True,
       doc="Insist that a new or changed base image is pinned by digest. Off in a repository that "
           "rebuilds nightly and considers a floating tag the honest answer.",
       analyzer="non_code.surface", ticket="e2.8"),
    _k(path="non_code.card.max_actions", kind="integer", default=12,
       doc="Action rows a card prints before it says how many more there are and which verb lists "
           "them. A cap that names its remainder, never a truncation without a trace.",
       analyzer="non_code.surface", ticket="e2.8"),
    # --- folder structure (e2.3) ------------------------------------------------
    # `fathom structure` answers `where did this land`: a per-directory table that sums
    # exactly to the episode totals, the structural stories the pairings add up to
    # (package renamed, subtree moved, module split, directory flattened), tree-shape
    # metrics, and a structure-churn score. The ticket standard is that a shape is
    # defined by numbers a repository can move rather than by a judgement call, and
    # `fathom structure definition` prints these values as sentences so a card and a
    # config file cannot drift from each other. Every default here is also
    # `recommender/structure/policy.py`'s, and a contract test fails if the two differ.
    _k(path="structure.code_classes", kind="string_list", default=['source', 'test'],
       merge="extend", units="path class",
       doc="Which taxonomy classes (`e0.9`, `e2.1`) count as code when the card measures "
           "where the code lives. `docs`, `config` and `generated` are out: a folder of "
           "markdown moving house is real, but it is not the same concentration claim as 4 "
           "000 lines of source changing home.",
       ticket="e2.3"),

    _k(path="structure.metrics.top_dirs", kind="integer", default=3,
       minimum=1, maximum=10, units="directories",
       doc="The N in `share of code in the top-N directories`. Three because that is the "
           "width the review question is asked with: top-1 measures one folder, top-10 "
           "measures the whole tree and separates nothing.",
       ticket="e2.3"),

    _k(path="structure.move.min_files", kind="integer", default=3,
       minimum=1, units="files",
       doc="Pairings a whole-directory story (`package-renamed`, `subtree-move`, `module- "
           "split`, `directory-flattened`) has to be built from. Below this, two files "
           "changing folder is a `files-moved` row, not a reorganisation of the tree - and "
           "a card that called every two-file move a reorganisation would be noisier than "
           "the file list it replaced.",
       ticket="e2.3"),

    _k(path="structure.move.min_share", kind="number", default=0.6,
       minimum=0.0, maximum=1.0, units="share",
       doc="Share of a directory's files that must have moved for the directory itself to "
           "be called moved. 1.0 demands a perfect sweep and loses every move with one "
           "straggler file; 0.0 calls any relocation a whole-directory move. 0.6 is `most "
           "of it`, which is what a reviewer means by `they moved the package`.",
       ticket="e2.3"),

    _k(path="structure.move.require_emptied", kind="boolean", default=True,
       doc="Additionally require that the source directory holds no files afterwards. On, a "
           "move is a move. Off, a directory that shipped most of its files and kept three "
           "still reads as moved, which is true of the files and misleading about the "
           "directory.",
       ticket="e2.3"),

    _k(path="structure.move.infer_content", kind="boolean", default=True,
       doc="When the caller supplies no pairings, infer them from bytes (same basename, "
           "similarity at or above `structure.move.min_similarity`). Turning this off makes "
           "the answer strictly narrower - no inferred moves, and the card says so - which "
           "is the right setting for a reviewer who will only act on what git asserted.",
       ticket="e2.3"),

    _k(path="structure.move.min_similarity", kind="number", default=0.5,
       minimum=0.0, maximum=1.0, units="0-1",
       doc="Line similarity required to accept an inferred pairing, computed exactly rather "
           "than sketched. Used only to accept or refuse a pairing, never printed as a "
           "similarity verdict of its own. Half the lines shared is where `the same file, "
           "edited` stops being a guess.",
       ticket="e2.3"),

    _k(path="structure.move.fact_bar", kind="number", default=0.75,
       minimum=0.0, maximum=1.0, units="0-1",
       doc="Confidence at or above which a caller-supplied pairing counts as a fact rather "
           "than a guess. Deliberately the same number as `rename.fact_bar`, which spike s4 "
           "measured on real histories: at git's shipped 0.50 the pairing engine asserts a "
           "named decoy, at 0.75 it ships nothing false. A structure card that trusted "
           "pairings git would not is a card that lies about where code landed.",
       ticket="e2.3"),

    _k(path="structure.split.min_destinations", kind="integer", default=2,
       minimum=2, units="dirs",
       doc="Destinations a module split needs. One destination is a move, and calling a "
           "move a split tells the reviewer responsibility was divided when nothing of the "
           "sort happened.",
       ticket="e2.3"),

    _k(path="structure.split.min_files_per_destination", kind="integer", default=2,
       minimum=1, units="files",
       doc="Files a destination needs to count as one arm of a split. One is the natural "
           "floor and the wrong one: a directory of 17 files with a single stray in a new "
           "folder is a move of 16 plus a `files-moved` row, not a two-arm split of the "
           "module.",
       ticket="e2.3"),

    _k(path="structure.flatten.min_levels", kind="integer", default=1,
       minimum=1, units="levels",
       doc="How many directory levels a move has to climb for the shape to be flattening "
           "rather than an ordinary move sideways. One is `went up at least one level, and "
           "the folder behind it is gone`, which is the shape.",
       ticket="e2.3"),

    _k(path="structure.churn.weight.moved_files", kind="number", default=0.25,
       minimum=0.0, maximum=1.0, units="weight",
       doc="Churn weight: share of the episode's files that changed directory. The five "
           "weights are normalised, so what matters is their ratio; this and `moved_lines` "
           "together make half the score, because `most of this episode was a move` is the "
           "claim the score exists to make.",
       ticket="e2.3"),

    _k(path="structure.churn.weight.moved_lines", kind="number", default=0.25,
       minimum=0.0, maximum=1.0, units="weight",
       doc="Churn weight: share of the episode's lines that travelled with a move. File "
           "counts alone let a one-line edit to a thousand files look like a "
           "reorganisation; line counts alone let one 5 000-line generated file moving look "
           "like one. Both are in, at equal weight.",
       ticket="e2.3"),

    _k(path="structure.churn.weight.directory_churn", kind="number", default=0.2,
       minimum=0.0, maximum=1.0, units="weight",
       doc="Churn weight: directories added and removed over directories the episode "
           "touched. Weighed below the mobility terms because a rename creates and deletes "
           "folders as a side effect of what already counted as a move.",
       ticket="e2.3"),

    _k(path="structure.churn.weight.stories", kind="number", default=0.2,
       minimum=0.0, maximum=1.0, units="weight",
       doc="Churn weight: the structural stories found, saturating at "
           "`structure.churn.story_ceiling`. A story is a counted claim with evidence "
           "attached, not a feeling, so it is worth a fifth of the score on its own.",
       ticket="e2.3"),

    _k(path="structure.churn.weight.shape_shift", kind="number", default=0.1,
       minimum=0.0, maximum=1.0, units="weight",
       doc="Churn weight: how far the tree's own shape metrics (mean and maximum depth, "
           "top-N concentration) moved. The smallest weight, because depth can change a lot "
           "in an episode that moved nothing; this is the tie-breaker, not the headline.",
       ticket="e2.3"),

    _k(path="structure.churn.story_ceiling", kind="number", default=2,
       minimum=1, units="stories",
       doc="Stories at which the story component saturates. A third story is not 50% worse "
           "than a second - it is the same story about a third folder - so a linear term "
           "would let a wide rename out-score a deep one.",
       ticket="e2.3"),

    _k(path="structure.churn.depth_scale", kind="number", default=4,
       minimum=1, units="levels",
       doc="Mean-depth change at which the shape component saturates. Four levels is where "
           "`the tree got deeper` has stopped being an incidental effect of where this "
           "episode's files landed and has become the change itself.",
       ticket="e2.3"),

    _k(path="structure.churn.band.local", kind="number", default=0.15,
       minimum=0.0, maximum=1.0, units="score",
       doc="Below this churn score the episode is `local`: it changed contents where they "
           "already were. Set from the 20-file local-change fixture, which scores 0.000; "
           "the band exists so the card can say `local` instead of making the reviewer read "
           "five components.",
       ticket="e2.3"),

    _k(path="structure.churn.band.structural", kind="number", default=0.35,
       minimum=0.0, maximum=1.0, units="score",
       doc="At or above this the episode is `structural`: the tree itself moved. The "
           "20-file cross-tree reshuffle fixture scores 0.900 and the 20-file local-change "
           "fixture 0.000, so the bar sits far from both rather than between two close "
           "calls. Between the two bands is `transitional`, and the card says that rather "
           "than rounding to a verdict.",
       ticket="e2.3"),

    _k(path="structure.depth.mean_delta", kind="number", default=0.5,
       minimum=0.0, units="levels",
       doc="Mean directory-depth growth that is worth a `nesting-deepened` row. Half a "
           "level over one episode is where a tree has stopped being shallow by accident.",
       ticket="e2.3"),

    _k(path="structure.depth.max_delta", kind="integer", default=2,
       minimum=1, units="levels",
       doc="Maximum-depth growth that earns the same row on its own, because one branch "
           "pushed two levels deeper is a review problem even when the mean barely moved.",
       ticket="e2.3"),

    _k(path="structure.concentration.delta", kind="number", default=0.15,
       minimum=0.0, maximum=1.0, units="share",
       doc="Growth in the top-N code share that is worth a `concentration-up` row. Smaller "
           "moves are what ordinary growth does to a tree; fifteen points is the tree "
           "getting one big folder.",
       ticket="e2.3"),

    _k(path="structure.card.max_dirs", kind="integer", default=12,
       minimum=1, units="rows",
       doc="Directory rows a card prints before it says how many more there are and which "
           "verb lists them. A cap that names its remainder, never a truncation without a "
           "trace.",
       ticket="e2.3"),

    _k(path="structure.card.max_stories", kind="integer", default=6,
       minimum=1, units="rows",
       doc="Structural stories a card prints before counting the rest. Six is more stories "
           "than an honest episode produces; a seventh usually means one directory was "
           "renamed in six places, which is one story, told six times.",
       ticket="e2.3"),

    _k(path="structure.card.max_evidence", kind="integer", default=4,
       minimum=1, units="lines",
       doc="Evidence lines printed per finding, with the remainder counted and named rather "
           "than dropped. Every line is always in `--json` and in `fathom structure "
           "findings`.",
       ticket="e2.3"),

    # --- structure findings (e2.3): one switch per kind ---------------------------
    _k(path="structure.findings.subtree_moved", kind="boolean", default=True,
       doc="A whole directory - its files and its inner folders - now lives under a "
           "different parent. Nothing inside it changed, so the reviewable content of the "
           "episode is the move itself. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.churn_high", kind="boolean", default=True,
       doc="The structure-churn score reached `structure.churn.band.structural`: the agent "
           "reorganised the tree. The card prints the score with all five components beside "
           "it, so this row is a number and not a vibe. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.package_renamed", kind="boolean", default=True,
       doc="A directory changed name in place with its contents intact, so every import of "
           "it moved and none of its logic did. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.module_split", kind="boolean", default=True,
       doc="One directory's files now live in two or more destinations. A split is where "
           "responsibility moved, which is the reviewable question, and the destinations "
           "are named. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.directory_flattened", kind="boolean", default=True,
       doc="A directory moved its files up into an ancestor and vanished, so the tree is "
           "one level shallower where it used to be. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.directory_removed", kind="boolean", default=True,
       doc="A directory that held files holds none. Reported even when its files were moved "
           "rather than deleted, because a removed directory is also a removed namespace "
           "and a stale import path. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.nesting_deepened", kind="boolean", default=True,
       doc="Average (or maximum) directory depth grew by the configured delta. Deep trees "
           "are how a review starts hiding things from itself. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.concentration_up", kind="boolean", default=True,
       doc="The share of code living in the top-N directories grew by the configured delta: "
           "the tree is getting one big folder. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.files_moved", kind="boolean", default=True,
       doc="Files changed directory without forming one of the four named shapes - a "
           "partial move, a move with a rename on the way, or a move out of a directory "
           "that still has files in it. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.directory_added", kind="boolean", default=True,
       doc="A new directory now holds files. Loud enough to notice, quiet enough not to "
           "compete with the move that explains it. `e2.3`.",
       ticket="e2.3"),

    _k(path="structure.findings.move_unconfirmed", kind="boolean", default=True,
       doc="Some pairing behind this answer was inferred from bytes, or handed over below "
           "`structure.move.fact_bar`, rather than asserted by git - so the directory rows "
           "built on it inherit that uncertainty and are listed instead of silently "
           "believed. `e2.3`.",
       ticket="e2.3"),
    _k(path="graph.ignored_dirs", kind="string_list", merge="extend",
       default=["node_modules"],
       doc="Directory names excluded from the structure/dependency graph entirely.",
       analyzer="graph.build", ticket="e3.2"),
    # --- layers -------------------------------------------------------------
    _k(path="layers.<layer-name>", kind="string_list", merge="deep",
       default={"ui": ["src/ui"], "core": ["src/core"], "store": ["src/store"]},
       doc="One named layer: the path prefixes that belong to it. `deep`, so a repo file "
           "adds a layer without erasing the layers you declared at home.",
       analyzer="structure.delta", ticket="e3.1"),
    # --- deps ---------------------------------------------------------------
    _k(path="deps.ecosystems", kind="string_list", merge="extend",
       default=["python", "node", "rust"],
       enum=("python", "node", "rust", "go", "java", "ruby", "dotnet", "php"),
       doc="Ecosystems whose manifests and lockfiles are parsed (the parser set is "
           "`e2.5`). Enabling one here never enables a network call.",
       analyzer="deps.resolve", ticket="e2.5"),

    # --- deps: dependency risk (e2.7 / reco-yxi) -----------------------------
    #
    # Every key below is a knob on an *answer*, not a switch on a program: none of them can
    # make the tool fetch. The one key that plausibly could have — "the OSV base URL" — was
    # deliberately not written: an endpoint read out of a cloned file is an endpoint an
    # attacker chose, and the no-network-by-default claim would then only have been true of
    # the default config. `fathom deprisk consent` prints the gates and their verdict.
    _k(path="deps.advisories.enabled", kind="boolean", default=True,
       doc="Whether the dependency-risk card answers advisories at all. Off, the card still "
           "renders and says advisories were switched off — an enrichment that vanishes "
           "silently is a worse answer than one with a line in it. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),
    _k(path="deps.advisories.mode", kind="string", default="cache",
       enum=("cache", "refresh", "offline"),
       doc="`cache` reads the local cache and never fetches; `refresh` may fetch, but only "
           "with `privacy.advisories` consent as well; `offline` does not read the cache "
           "either and says so on the card. Default `cache`, because 'zero network calls by "
           "default' is a default *mode*, not a default flag. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),
    _k(path="deps.advisories.cache", kind="path", default=".fathom/advisories.json",
       doc="Where cached OSV responses live, relative to the repository root. A missing file "
           "is an answer with a stated partial in it, never an error. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),
    _k(path="deps.advisories.max_age_hours", kind="integer", default=24, minimum=1,
       maximum=8760, units="hours",
       doc="Age past which a cached entry is `stale`: still used, and labelled as used. A "
           "clean answer with no age on it is a claim with no date. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),
    _k(path="deps.advisories.stale_limit_hours", kind="integer", default=168, minimum=1,
       maximum=8760, units="hours",
       doc="Age past which the answer stops claiming 'no advisories found' and says 'not "
           "checked recently' instead — the tense change is the whole point, because a "
           "seven-week-old clean result is not a clean result. Must be at or above "
           "`max_age_hours`; the schema cannot say that across two keys, so the analyzer "
           "refuses the pair. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),
    _k(path="deps.advisories.max_packages", kind="integer", default=200, minimum=1,
       maximum=10000, units="packages",
       doc="How many packages one episode's refresh will look up, with the remainder printed "
           "as 'not checked (cap reached)' rather than silently dropped. Bounds the refresh "
           "that a lockfile rewrite turned into four hundred requests. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),
    _k(path="deps.advisories.timeout_ms", kind="integer", default=1500, minimum=100,
       maximum=60000, units="milliseconds",
       doc="Per-request budget for a consented refresh — a bound on blocking, which exists "
           "because a lookup that can hang is a lookup that blocks episode rendering, and "
           "`e2.7` forbids that. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),
    _k(path="deps.advisories.ecosystems", kind="string_list", merge="extend",
       default=["crates.io", "PyPI", "npm", "Go", "RubyGems", "Maven", "NuGet", "Hex"],
       doc="Ecosystem names, in OSV's spelling, that advisories are asked about. `extend`, so "
           "a repository naming its vendored registry does not drop the eight everyone else "
           "gets. A package in an ecosystem not listed is reported as not checked, never as "
           "clean. `e2.7`.",
       analyzer="deps.advisories", ticket="e2.7"),

    # Two halves of one policy: the lists teach which SPDX ids are in which copyleft class
    # (a policy that hard-codes its own vocabulary cannot be argued with), and the matrix says
    # what a class may do in the library layer versus anywhere else. `fathom deprisk matrix`
    # prints the resolved matrix; `fathom deprisk policy` prints every key with its layer.
    _k(path="deps.license.deny", kind="string_list", merge="extend",
       default=["AGPL-3.0", "AGPL-3.0-only", "AGPL-3.0-later", "SSPL-1.0"],
       doc="Licence ids refused wherever they appear, matrix included — a named refusal "
           "outranks a class. `extend`, so adding SSPL cannot be done by rewriting the "
           "built-in list and losing AGPL on the way. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.permissive", kind="string_list", merge="extend",
       default=["MIT", "MIT-0", "Apache-2.0", "Apache-1.1", "BSD-2-Clause", "BSD-3-Clause",
                "BSD-3-Clause-Clear", "ISC", "Zlib", "0BSD", "Python-2.0", "PSF-2.0",
                "Unlicense", "BSL-1.0", "PostgreSQL", "SQLite", "curl", "WTFPL", "Beerware",
                "Unicode-3.0", "CC0-1.0", "MIT-open-group"],
       doc="Ids treated as permissive: allowed everywhere, and still counted when one is "
           "newly present in what the repository ships. Taught by table rather than baked in, "
           "so an unusual-but-fine id is a config edit and not a fork. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.weak_copyleft", kind="string_list", merge="extend",
       default=["MPL-2.0", "MPL-1.1", "LGPL-2.0-only", "LGPL-2.0-or-later", "LGPL-2.1-only",
                "LGPL-2.1-or-later", "LGPL-3.0-only", "LGPL-3.0-or-later", "CDDL-1.0",
                "CDDL-1.1", "EPL-1.0", "EPL-2.0",
                "GPL-2.0-only WITH Classpath-exception-2.0",
                "GPL-2.0-or-later WITH Classpath-exception-2.0",
                "GPL-2.0-with-classpath-exception", "Universal-FOSS-Exception-1.0"],
       doc="File-level copyleft: the obligation attaches to the file that carries it, so a "
           "library may ship these and an application usually only has to say so. That is a "
           "matrix cell, not a fact about MPL. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.strong_copyleft", kind="string_list", merge="extend",
       default=["GPL-1.0-only", "GPL-1.0-or-later", "GPL-2.0-only", "GPL-2.0-or-later",
                "GPL-3.0-only", "GPL-3.0-or-later", "GFDL-1.1-only", "GFDL-1.1-or-later",
                "GFDL-1.3-only", "GFDL-1.3-or-later", "Sleepycat", "Ruby", "QPL-1.0",
                "Sendmail"],
       doc="Whole-work copyleft. Whether that is fatal depends on whether the distributor is "
           "the library or an application built from it — which is why this is a class with a "
           "matrix row and not a deny list. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.network_copyleft", kind="string_list", merge="extend",
       default=["AGPL-1.0-only", "AGPL-1.0-or-later", "AGPL-3.0-only", "AGPL-3.0-or-later",
                "AGPL-3.0", "SSPL-1.0", "OSL-3.0", "EUPL-1.1", "EUPL-1.2", "CPAL-1.0",
                "CECILL-2.1"],
       doc="Copyleft whose trigger includes offering the code as a service, so 'we only run "
           "it' does not escape it. Denied in every layer by default; a repository that has "
           "taken advice can move a cell to `warn`. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.library_layers", kind="string_list", merge="extend",
       default=["library", "lib", "core", "sdk", "api"],
       doc="Which declared layers count as the library layer: the layer whose licences "
           "propagate to whoever links it, and therefore the layer the matrix is strict "
           "about. Read against `layers.*`. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.strong_copyleft_in_library", kind="string", default="deny",
       enum=("deny", "warn", "allow"),
       doc="What strong copyleft may do in the library layer. `deny` by default: linking it "
           "from a library hands the obligation to every downstream, which is not a decision "
           "a dependency bump gets to make silently. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.strong_copyleft_elsewhere", kind="string", default="warn",
       enum=("deny", "warn", "allow"),
       doc="What strong copyleft may do outside the library layer — `warn`, because a GPL CLI "
           "in the composition layer is usually a decision someone made on purpose, and the "
           "finding is there to have it confirmed. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.weak_copyleft_in_library", kind="string", default="warn",
       enum=("deny", "warn", "allow"),
       doc="What file-level copyleft may do in the library layer: allowed, and printed, "
           "because MPL-in-a-library is fine right up until someone ships a modification. "
           "`e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.weak_copyleft_elsewhere", kind="string", default="allow",
       enum=("deny", "warn", "allow"),
       doc="What file-level copyleft may do outside the library layer. `allow`: quiet enough "
           "not to compete with the finding that matters. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.network_copyleft_in_library", kind="string", default="deny",
       enum=("deny", "warn", "allow"),
       doc="What network copyleft may do in the library layer. Both network-copyleft cells "
           "default to `deny`, because the trigger is how software is *offered*, which no "
           "layering decision changes. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.network_copyleft_elsewhere", kind="string", default="deny",
       enum=("deny", "warn", "allow"),
       doc="What network copyleft may do outside the library layer. See the row above: both "
           "cells are `deny` because the class is the reason, not the layer. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.unknown_layer_policy", kind="string", default="strict",
       enum=("strict", "forgiving"),
       doc="How to treat a licence id nobody's table knows. `strict`: an unknown obligation is "
           "reported as a stated partial and never as a pass — 'no violation found' about a "
           "licence nobody read is the worst thing this tool can say. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.ignore_dev", kind="boolean", default=False,
       doc="Skip development-only dependencies when evaluating licence policy. Off by default: "
           "a copyleft test fixture that ships in the wheel is still shipped, and the finding "
           "nobody expected is still the finding. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),
    _k(path="deps.license.card.max_rows", kind="integer", default=12, minimum=1, maximum=1000,
       units="rows",
       doc="Licence rows printed on the card before the remainder is counted and the verb that "
           "lists them is named. A cap on rows is not a deletion: the render audit fails the "
           "card if a violation goes missing rather than going to the 'N more' line. `e2.7`.",
       analyzer="deps.license", ticket="e2.7"),

    # 'Who asked for this' is answered from what Fathom actually stores, which for intent is: a
    # commit trailer, a plan item, a review note, a manual note, or nothing. Hooks v1 stores
    # `prompt_sha256` and `prompt_chars` and not the prompt (ADR-005 §2.2), so a digest is
    # reported as a stated partial and never as an answer.
    _k(path="deps.provenance.require_intent", kind="boolean", default=True,
       doc="Report an unknown intent as the finding `intent-not-captured` when the introducing "
           "episode is known and its reason is not. `e2.7`.",
       analyzer="deps.provenance", ticket="e2.7"),
    _k(path="deps.provenance.intent_sources", kind="string_list", merge="extend",
       default=["agent-intent-trailer", "plan-item", "review-note", "manual-note"],
       doc="Which recorded intent sources may be printed as the answer. Evidence from a source "
           "listed here is quoted with its source and its confidence; evidence from one not "
           "listed is reported as existing-but-not-allowed, because unreviewed text silently "
           "used as an explanation is the failure this list exists to prevent. `e2.7`.",
       analyzer="deps.provenance", ticket="e2.7"),
    _k(path="deps.provenance.min_confidence", kind="number", default=0.5, minimum=0.0,
       maximum=1.0, units="0-1",
       doc="Minimum confidence for an intent to be printed as the answer rather than as a "
           "guess — and a guess is still printed, labelled, rather than dropped. Inferred "
           "evidence carries low confidence, so raising this mostly produces more guesses. "
           "`e2.7`.",
       analyzer="deps.provenance", ticket="e2.7"),
    _k(path="deps.provenance.agent_counts_as_intent", kind="boolean", default=False,
       doc="Let attribution ('agent codex') stand in for intent. Off, and it should stay off: "
           "who executed the change is not who asked for it, and turning this on makes "
           "`intent: not recorded` impossible to ever see. `e2.7`.",
       analyzer="deps.provenance", ticket="e2.7"),
    _k(path="deps.provenance.search_previous_episodes", kind="boolean", default=True,
       doc="Look through recorded introductions from earlier episodes when the episode under "
           "review did not add the package. The question is usually asked weeks after the "
           "answer was written. `e2.7`.",
       analyzer="deps.provenance", ticket="e2.7"),
    _k(path="deps.provenance.card.max_rows", kind="integer", default=12, minimum=1,
       maximum=1000, units="rows",
       doc="Provenance rows printed on the card before the remainder is counted and named. "
           "`e2.7`.",
       analyzer="deps.provenance", ticket="e2.7"),
    _k(path="deps.card.max_findings", kind="integer", default=20, minimum=1, maximum=1000,
       units="findings",
       doc="Findings printed on the card. Past the cap they are counted in the summary line "
           "rather than hidden: a card that quietly omits the eleventh violation is worse than "
           "a long one. `e2.7`.",
       analyzer="deps.card", ticket="e2.7"),
    _k(path="deps.card.max_evidence", kind="integer", default=4, minimum=0, maximum=20,
       units="lines",
       doc="Evidence lines printed per finding on the card. `0` prints kinds only, for a "
           "reviewer who wants the census first and the evidence on demand. `e2.7`.",
       analyzer="deps.card", ticket="e2.7"),

    # One switch per finding kind, because 'should I be worried' is a question about which of
    # these eighteen signals a reviewer has agreed to be told about. Every switch defaults on;
    # a finding that is switched off is counted in the card's suppressed line, never deleted.
    _k(path="deps.findings.new_direct_dependency", kind="boolean", default=True,
       doc="The repository now asks for a package by name that it did not ask for before: the "
           "row that carries the answer, and the reason this card exists. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.provenance_unknown", kind="boolean", default=True,
       doc="Nothing readable names the episode that introduced a new direct dependency. "
           "Printed as the answer — 'not recorded', plus what would fill it — because a blank "
           "line reads as 'nothing to see'. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.intent_not_captured", kind="boolean", default=True,
       doc="The introducing episode is known and its intent is not, or only a prompt digest "
           "is. A digest proves a request happened; it does not say what it was. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.vcs_source", kind="boolean", default=True,
       doc="Resolved from git/svn/hg instead of a registry: no immutable version, no registry "
           "signature, and a commit as the reviewable unit — a different question than the one "
           "a registry version answers. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.new_source", kind="boolean", default=True,
       doc="A registry, git URL or vendored tree this repository had never resolved from. The "
           "package may be innocent; the origin is what is new, and origins are where supply "
           "chains are actually attacked. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_critical", kind="boolean", default=True,
       doc="Cached OSV data reports a critical advisory for this package at this version. Off, "
           "this card could not raise a critical finding at all, which is why it is on and why "
           "the switch is stated rather than assumed. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_high", kind="boolean", default=True,
       doc="Cached OSV data reports a high-severity advisory for this package at this "
           "version. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_moderate", kind="boolean", default=True,
       doc="Cached OSV data reports a moderate advisory — including the ones upstream left "
           "unseverity, which are printed as unseverity and never as nothing. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_low", kind="boolean", default=True,
       doc="Cached OSV data reports a low-severity advisory. Loud enough to see, not loud "
           "enough to compete with the one that has a fix. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_fix_available", kind="boolean", default=True,
       doc="An advisory names a fixed version at or after the installed one, so the action is "
           "a bump rather than an argument, and the row names the version. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_unchecked", kind="boolean", default=True,
       doc="Packages this answer did not check, grouped by reason — no cached entry, ecosystem "
           "not asked about, cap reached. 'No advisories found' is only honest next to this "
           "line. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_cache_stale", kind="boolean", default=True,
       doc="The cache is older than `max_age_hours`, or past `stale_limit_hours`. A stale "
           "cache is still a usable cache; this finding says which one it is looking at. "
           "`e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.advisory_cache_missing", kind="boolean", default=True,
       doc="There was no cache to read. The card renders anyway: advisories are an enrichment, "
           "never a precondition of the answer. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.license_denied", kind="boolean", default=True,
       doc="The licence is one this repository's policy refuses for the layer that now asks "
           "for it. The finding names the edge path, because 'who asked' is the half a "
           "reviewer can act on. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.license_flagged", kind="boolean", default=True,
       doc="The licence is allowed but its matrix cell is `warn`: permitted, and still a "
           "decision nobody has made out loud. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.license_unknown", kind="boolean", default=True,
       doc="Nothing declared a licence, so policy cannot be evaluated. A stated partial, never "
           "a pass. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.license_drift", kind="boolean", default=True,
       doc="An existing package's declared licence moved between the two revisions — same "
           "name, different permission. Nobody reviewed a relicence unless this row says so. "
           "`e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    _k(path="deps.findings.license_added_to_set", kind="boolean", default=True,
       doc="A licence id is newly present in what this repository ships, violation or not: the "
           "id, and the package that brought it. `e2.7`.",
       analyzer="deps.delta", ticket="e2.7"),
    # --- risk / detectors ---------------------------------------------------
    _k(path="risk.weights.<weight>", kind="number", merge="deep",
       default={"size": 0.4, "test_ratio": 0.3, "blast": 0.3}, minimum=0.0, maximum=1.0,
       doc="Per-signal weights in the risk score. `deep`: adding a signal's weight in a "
           "repo file does not drop the signals you weighted at home.",
       analyzer="findings.detect", ticket="e2.9"),
    _k(path="risk.tau", kind="integer", default=70, minimum=0, maximum=100, units="score",
       doc="Risk threshold above which an episode is surfaced as needing attention.",
       analyzer="risk.score", ticket="e2.10"),
    _k(path="detectors.enabled", kind="string_list", merge="extend",
       default=["test_deletion", "untested_new_logic", "debug_leftover"],
       doc="Detectors that run. Unknown detector ids are a warning naming a valid id.",
       analyzer="findings.detect", ticket="e2.7"),
    _k(path="blast.hops", kind="integer", default=1, minimum=0, maximum=4, units="hops",
       doc="Rings counted *beyond* the direct one: the direct ring is always counted, so the "
           "default of 1 answers 'direct + one hop' and 0 answers 'direct only'. Anything "
           "further out is a stated partial, never a silent stop. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.confidence.graph", kind="number", default=0.9, minimum=0.0, maximum=1.0,
       units="0-1",
       doc="Confidence attached to a blast answer that came from a module graph. Not 1.0: a "
           "graph is built by an extractor, and spike `s1` measured declarations a program wrote "
           "(Rust macros, Python decorators) that no extractor can enumerate. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.confidence.heuristic", kind="number", default=0.4, minimum=0.0,
       maximum=1.0, units="0-1",
       doc="Confidence attached to a blast answer built from path heuristics instead of a "
           "graph. The plan asks for the reduced confidence to be *stated*; this is the number "
           "it states. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.confidence.narrow_census", kind="number", default=0.7, minimum=0.0,
       maximum=1.0, units="multiplier",
       doc="Multiplier applied to a heuristic blast answer when the caller lent no file "
           "inventory, so the only paths the heuristics could see were the episode's own. A "
           "reach computed from a world of twelve files may not claim the confidence of one "
           "computed from four thousand. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.heuristic.same_directory", kind="number", default=0.6, minimum=0.0,
       maximum=1.0, units="score",
       doc="Heuristic weight: a candidate in the changed file's own directory - the strongest "
           "path signal there is, because same folder means same imports in every layout that "
           "has folders. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.heuristic.test_pair", kind="number", default=0.5, minimum=0.0, maximum=1.0,
       units="score",
       doc="Heuristic weight: the test file that covers a changed source file, or the source a "
           "changed test covers, matched by naming convention only. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.heuristic.sibling_directory", kind="number", default=0.3, minimum=0.0,
       maximum=1.0, units="score",
       doc="Heuristic weight: the files in the changed file's parent directory. Weaker than "
           "same-directory on purpose - a parent's own files are neighbours, not dependents. "
           "`e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.heuristic.package_prefix", kind="number", default=0.15, minimum=0.0,
       maximum=1.0, units="score",
       doc="Heuristic weight: everything under the changed file's top-level package (two "
           "directory levels). Cheap and wide, which is why it normally sits under "
           "`blast.heuristic.min_score` and only counts when nothing stronger fired. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.heuristic.min_score", kind="number", default=0.3, minimum=0.0, maximum=1.0,
       units="score",
       doc="Heuristic score a candidate must reach to be counted as reached. This floor is what "
           "stops 'it is in the same repository' being a blast radius. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.max_rows", kind="integer", default=10, minimum=1, units="rows",
       doc="Blast rows printed per ring before the remainder is counted and named. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="blast.max_candidates", kind="integer", default=5000, minimum=1, units="candidates",
       doc="Candidate paths examined before the heuristic walk stops. Stopping is a stated "
           "partial naming how many paths were not looked at, so the cost guard shows up in the "
           "answer instead of being a silent truncation. `e2.10`.",
       analyzer="blast_radius", ticket="e2.10"),
    # --- episode summary card (e2.10) -----------------------------------------
    _k(path="card.max_figures", kind="integer", default=6, minimum=1, units="rows",
       doc="Figure rows printed per section before the remainder is counted and named. A cap "
           "that names its remainder, never a truncation without a trace: the audit fails a "
           "card that trimmed a figure in silence. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.max_findings", kind="integer", default=8, minimum=1, units="rows",
       doc="Attention rows before the rest are counted rather than cut. Ordering is severity, "
           "then source section, then name, so the same rows come out twice. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.max_evidence", kind="integer", default=3, minimum=1, units="lines",
       doc="Evidence lines printed under a figure. Everything behind the link is in `--json` "
           "and in the view the link names, whether or not the card had room for it. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.attention.severity", kind="string", default="notable",
       enum=("high", "notable", "note"),
       doc="Lightest severity that earns an attention row. Findings below it are still counted "
           "and the count is printed, because 'nothing here' and 'nothing loud' are different "
           "claims. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.state.digits", kind="integer", default=3, minimum=0, maximum=6, units="digits",
       doc="Decimal places on every ratio, share and confidence the card prints. One code path "
           "formats them, so `0.615` cannot be `0.62` in two sections of one card. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.sections.what", kind="boolean", default=True,
       doc="Print the what-changed section: semantic counts after `e2.2` folds reformat, "
           "generated and lockfile churn, plus the path-class mix. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.sections.where", kind="boolean", default=True,
       doc="Print the where-it-landed section: directories touched, lines and files that "
           "moved, the structural story and the churn band, with the census basis. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.sections.dependencies", kind="boolean", default=True,
       doc="Print the dependency section from `e2.6`'s delta. Switching it off is a stated "
           "omission on the card, not a section that vanishes. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.sections.surface", kind="boolean", default=True,
       doc="Print the non-code surface section: env, flags, migrations, CI, IaC and container "
           "findings with the operator actions they require. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.sections.tests", kind="boolean", default=True,
       doc="Print the test-surface section: test versus production lines with their absolutes, "
           "named cases, and the coverage findings. Never a coverage percentage. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.sections.blast", kind="boolean", default=True,
       doc="Print the blast-radius section: direct dependents plus a one-hop estimate, with the "
           "basis and the confidence beside the count. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="card.sections.attention", kind="boolean", default=True,
       doc="Print the attention roll-up: the findings the other sections raised, in severity "
           "order, each with the evidence that put it there. `e2.10`.",
       analyzer="card.compose", ticket="e2.10"),
    _k(path="rename.threshold", kind="integer", default=60, minimum=0, maximum=100,
       units="percent", doc="Similarity needed to call two paths the same file.",
       analyzer="diff.rename_detect", ticket="e1.6"),
    # --- rename pairing (e2.4). `rename.threshold` above is what the *diff pane* prints as a
    # rename; these keys decide what the pairing engine will assert, and spike s4 measured two of
    # them. The pairing engine asks git at the lower of the fact bar and the guess floor, so it can
    # put git's own number on a guess, and `rename.threshold` no longer caps what a pairing can say.
    _k(path="rename.fact_bar", kind="number", default=0.75, minimum=0.0, maximum=1.0,
       units="similarity",
       doc="git's similarity index needed before a pairing is a FACT a reviewer is asked to act "
           "on. Measured on s4's corpus: at git's shipped 0.50 the engine asserts a named decoy and "
           "still misses every rewrite below R050; at 0.75 it ships nothing false. Every pairing "
           "below this bar is a labelled guess, because no threshold separates the corpus's decoy "
           "from its true rewrites.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.fact.allow_content", kind="boolean", default=False,
       doc="Let the content matcher's own similarity assert a FACT at or above the fact bar. Off: "
           "a fact should be re-checkable with `git log --follow`, and s4 measured git's percentage "
           "as badly calibrated (ECE 0.182) once the engine pairs anything — worse for a number a "
           "reviewer cannot reproduce at all. On, and the row says the number is ours.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.guess_floor", kind="number", default=0.15, minimum=0.0, maximum=1.0,
       units="similarity",
       doc="The lowest-scoring guess the card prints. Not a correctness bar — no threshold removes "
           "the decoy — a display budget. 0.15 is the measured knee: every claimable pair the engine "
           "can find is found, and 0.10 buys two more (both above the claimable line) for three more "
           "rows and no fewer lies.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.candidates.top_k", kind="integer", default=3, minimum=1, maximum=10,
       units="pairs",
       doc="A source may claim its k best destinations and a destination its k best sources; a pair "
           "survives if either side wants it. This is what lets a 1-to-3 split be emitted at all — a "
           "1:1 assignment cannot name three destinations at any threshold.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.candidates.budget", kind="integer", default=4000, minimum=1,
       units="pairs",
       doc="Scored pairs per window before the candidate list is truncated to the best. Over budget "
           "is printed as a stated partial with the number dropped, never a silent short recall.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.candidates.copies_harder", kind="boolean", default=False,
       doc="Widen copy sources to every path in the base tree — the only way to see a copy of a "
           "file nobody touched in this window. It is O(repo files), which `e2.4`'s own cost "
           "criterion rules out, and s4 priced what it buys: both real copies and fabricated "
           "pairings on a window where nothing moved. When on, every card prints the cost.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.copies", kind="boolean", default=True,
       doc="Let a file this window modified be a copy source. Still O(changed files), and the only "
           "copy a window-local method can ever see: a copy's source is never deleted, so a "
           "deletions-only source set is copy-blind by construction.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.folds", kind="boolean", default=True,
       doc="Let a file this window modified be a merge destination. Without it a fold into an "
           "existing module is invisible — the destination never appears among the additions, which "
           "is why git reports zero folds at any threshold.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.content.shingle_lines", kind="integer", default=3, minimum=1, maximum=12,
       units="lines",
       doc="Lines per shingle. One line is a bag of lines, which scores every file sharing an import "
           "header as similar — that is the decoy failure; three needs a run of context. Wider, and "
           "a moved-and-edited file stops matching at all.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.content.normalise", kind="string", default="lines", enum=("lines", "tokens"),
       units="mode",
       doc="`lines` folds whitespace only, so a moved-and-reformatted file is still found. `tokens` "
           "runs e2.2's token normaliser first, which makes a pure reformat invisible between the "
           "two blobs — and also erases the difference between two files that differ only in "
           "formatting, which a reviewer may want reported.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.content.min_shared_shingles", kind="integer", default=4, minimum=0,
       units="shingles",
       doc="How many shingles two files must actually share before a similarity is a pairing. A "
           "four-line file sharing one shingle has a Jaccard of 1.00 and shares nothing worth "
           "calling provenance; without this floor every four-line config file is a candidate copy "
           "of every other.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="rename.card.rows", kind="integer", default=12, minimum=1, units="rows",
       doc="Rows per tier before the card names the remainder and the verb that lists it. A cap is "
           "not a truncation as long as the count survives — the auditor fails a card that loses "
           "rows silently.",
       analyzer="diff.rename_detect", ticket="e2.4"),
    _k(path="diff.context_lines", kind="integer", default=3, minimum=0, maximum=20,
       units="lines", doc="Context lines per diff hunk.", analyzer="diff.per_file",
       ticket="e1.6"),
    # --- episode ------------------------------------------------------------
    _k(path="episode.boundaries", kind="string_list", merge="replace",
       default=["hook", "trailer", "author"],
       enum=("hook", "trailer", "author", "branch", "time-gap", "message-shape"),
       doc="Boundary sources, spelled exactly as `episode.boundary_rule` stores them (ADR-003 "
           "App. B). `replace`, not `extend`: the *order* is the meaning, and a merged list has no "
           "coherent order. The order itself is fixed by ADR-003 §4.5 and is not configurable: a "
           "value other than the canonical prefix is named on the run that ignored it. "
           "`force_split`/`force_merge` are the supported way to move a boundary.",
       analyzer="episode.segment", ticket="e1.3"),
    _k(path="episode.time_gap_minutes", kind="integer", default=30, minimum=1,
       maximum=1440, units="minutes",
       doc="Idle gap that closes an episode when no stronger boundary source fires.",
       analyzer="episode.segment", ticket="e1.3"),
    _k(path="episode.force_split", kind="string_list", merge="extend", default=[],
       item_pattern=r"^(oid:[0-9a-f]{2,40}|subject:.+)$",
       doc="Commits that always open a new episode, each named `oid:<prefix>` or "
           "`subject:<substring>`. A boundary a human moved is recorded as such on the "
           "episode, so the card can say the split was an instruction and not a guess.",
       analyzer="episode.segment", ticket="e1.3"),
    _k(path="episode.force_merge", kind="string_list", merge="extend", default=[],
       item_pattern=r"^(oid:[0-9a-f]{2,40}|subject:.+)$",
       doc="Commits that never open an episode, in the same two spellings. Ordered "
           "before every other rule, including a turn event: when the human has said "
           "two commits are one change, the harness does not overrule them.",
       analyzer="episode.segment", ticket="e1.3"),
    # --- attribution (e4.1: who wrote this commit, and with what evidence) ----
    _k(path="attribution.agents.<agent-id>", kind="string_list", merge="extend", default=[],
       doc="The registry of known agents: one entry per agent id, holding lower-cased "
           "substrings of an author, committer or trailer identity. Entries are added to "
           "the built-in registry (codex, copilot, claude, devin, aider, \u2026), because a "
           "registry that silently dropped an entry would relabel that agent's commits as "
           "human work. An entry whose patterns match decides the agent the label names.",
       analyzer="episode.attribute", ticket="e4.1"),
    _k(path="attribution.markers", kind="string_list", merge="extend", default=[],
       doc="Substrings that make an identity look agent-shaped (`jane@copilot.example`), "
           "added to the built-in table. A marker that matches only inside an address's "
           "*domain* is read as ambiguity, not as an agent, because `jane@assistant.fr` is "
           "a person at a hosting name.",
       analyzer="episode.attribute", ticket="e4.1"),
    _k(path="attribution.trust_committer", kind="boolean", default=True,
       doc="Whether the committer counts as evidence when the author does not: a bot that "
           "applies a human's change moves the committer and leaves the author alone. A "
           "repository whose CI commits everything should turn this off rather than read "
           "every episode as `agent`.",
       analyzer="episode.attribute", ticket="e4.1"),
    _k(path="attribution.ambiguous_below", kind="number", default=0.5, minimum=0.0,
       maximum=1.0, units="confidence",
       doc="Below this confidence the verdict is `unknown` with the reason printed, rather "
           "than the weak label the evidence would have supported. The built-in 0.50 prices "
           "the weakest rung (`identity`) exactly at the bar, so nothing is demoted until a "
           "repository asks for it.",
       analyzer="episode.attribute", ticket="e4.1"),
    # --- intent (e4.2: what the episode was for, and how much of it is kept) --
    _k(path="intent.sources", kind="string_list", merge="replace",
       default=["hook-prompt", "hook-intent", "agent-intent-trailer", "plan-item",
                "task-trailer", "prompt-digest"],
       doc="Which rungs of the intent ladder may decide the text, in the published spellings "
           "(`hook-prompt`, `hook-intent`, `agent-intent-trailer`, `plan-item`, `task-trailer`, "
           "`prompt-digest`). This **replaces** the list rather than extending it: the only "
           "interesting use of the key is to turn something off — a repository whose prompts "
           "carry names it does not want quoted says `[\n\"agent-intent-trailer\"]` and means "
           "exactly that. An entry naming no rung this build reads is a notice, not an error: "
           "`review-note` and `manual-note` are spellings `[deps.provenance] intent_sources` "
           "already lists, and `e4.6`/`e6.5` are the tickets that would produce them.",
       analyzer="episode.intent", ticket="e4.2"),
    _k(path="intent.digest_counts", kind="boolean", default=True,
       doc="Whether a prompt digest with no text counts as an intent at all. On, the episode "
           "reads `digest-only` with the prompt's digest — \"a request happened and its text "
           "was not captured\" is a fact. Off, and such an episode reads `no recorded intent` "
           "like any other, which is the honest choice for a repository that would rather not "
           "print a hash where a sentence belongs.",
       analyzer="episode.intent", ticket="e4.2"),
    _k(path="intent.max_text_bytes", kind="integer", default=4096, minimum=64,
       units="bytes",
       doc="Cap on any one piece of intent text — a prompt, a plan item, a trailer value. A "
           "prompt is a paragraph, not a file, and ADR-005 §6.3 forbids file contents crossing "
           "this boundary: a harness that echoed a 2 MB prompt into a `UserPromptSubmit` "
           "payload would otherwise grow the store by the prompt's size forever. Text that "
           "exceeds the cap is kept truncated, and the document says `truncated: true` rather "
           "than ending mid-sentence in silence.",
       analyzer="episode.intent", ticket="e4.2"),
    _k(path="intent.plan_cap", kind="integer", default=64, minimum=1, units="items",
       doc="Plan items kept per episode. The document counts what it dropped instead of "
           "quietly losing the tail, because a plan list that silently ends is a plan that "
           "was not finished.",
       analyzer="episode.intent", ticket="e4.2"),
    _k(path="intent.scope_cap", kind="integer", default=64, minimum=1, units="globs",
       doc="Declared-scope globs kept per episode. Absolute paths, `..` escapes and anything "
           "holding a NUL are refused and named in `scope_rejected`, never normalised into "
           "something that reads as a repository-relative glob.",
       analyzer="episode.intent", ticket="e4.2"),
    # --- ingest / store / memory caps (defaults come from the budget table) --
    _k(path="ingest.size_cap_mib", kind="integer", default=8, fallback=8,
       budget="ingest.file_size_cap.mib", units="MiB", minimum=1,
       doc="Largest single blob that enters memory; bigger is recorded as `oversized` "
           "with counts, never slurped. The budget face of this knob is "
           "`ingest.file_size_cap.mib` — one value, two names, one alias table.",
       analyzer="ingest.normalize", ticket="e1.2"),
    _k(path="cache.budget.mib", kind="integer", default=512, fallback=512,
       budget="cache.budget.mib", units="MiB", minimum=16,
       doc="LRU ceiling on class-R cache rows and their blobs (ADR-004 §11).",
       ticket="e1.4"),
    _k(path="store.wal_cap.mib", kind="integer", default=64, fallback=64,
       budget="store.wal_cap.mib", units="MiB", minimum=8,
       doc="WAL size at which the writer warns and forces a passive checkpoint.",
       ticket="e1.1"),
    _k(path="store.size.mib", kind="integer", default=2048, fallback=2048,
       budget="store.size.mib", units="MiB", minimum=64,
       doc="Soft store-size ceiling: `fathom doctor` warns at this size. A warning, "
           "never a refusal — refusing to write would lose the review ledger's context.",
       ticket="e1.10"),
    _k(path="rss.analyzer_working_set.mib", kind="integer", default=128, fallback=128,
       budget="rss.analyzer_working_set.mib", units="MiB", minimum=8,
       doc="Per-analyzer working set; an analyzer needing more must stream or say it is "
           "running in the background lane.", ticket="e0.11"),
    _k(path="rss.cold_index_peak_mib", kind="integer", default=500, fallback=500,
       budget="rss.cold_index.peak_mib", units="MiB", minimum=64,
       doc="Cold-index peak RSS budget, graph lane included.", ticket="e0.11"),
    _k(path="rss.sqlite_page_cache.mib", kind="integer", default=8, fallback=8,
       budget="rss.sqlite_page_cache.mib", units="MiB", minimum=1,
       doc="`PRAGMA cache_size` per connection: fixed per connection, not per table.",
       ticket="e1.1"),
    _k(path="rss.daemon.mib", kind="integer", default=220, fallback=220,
       budget="rss.daemon.mib", units="MiB", minimum=32,
       doc="Daemon at idle plus one in-flight warm index.", ticket="e6.3"),
    _k(path="rss.tui.mib", kind="integer", default=180, fallback=180, budget="rss.tui.mib",
       units="MiB", minimum=32,
       doc="Steady-state TUI with a warm store. Never scaled by a machine factor "
           "(ADR-004 §11.8): memory budgets are not time budgets.", ticket="e5.1"),
    _k(path="write.batch_rows", kind="integer", default=5000, fallback=5000,
       budget="write.batch_rows", units="rows", minimum=1,
       doc="Rows per writer transaction (ADR-004 §7.3). Measured 12x versus one row per "
           "transaction, so this is not cosmetic.", ticket="e1.1"),
    _k(path="write.batch_ms", kind="integer", default=50, fallback=50,
       budget="write.batch.ms", units="ms", minimum=0,
       doc="Also commit at this many ms, whichever comes first, so a big ingest still "
           "shows progress.", ticket="e1.1"),
    _k(path="write.mandatory_checkpoint_per_commit", kind="integer", default=0, fallback=0,
       budget="write.mandatory_checkpoint_per_commit", units="checkpoints", minimum=0,
       doc="Mandatory `wal_checkpoint(TRUNCATE)` calls per commit. Zero: measured at "
           "0.8 % of a warm budget *per commit*.", ticket="e1.1"),
    _k(path="prune.batch_rows", kind="integer", default=5000, fallback=5000,
       budget="prune.batch_rows", units="rows", minimum=1,
       doc="Rows deleted per pruning transaction.", ticket="e6.1"),
    _k(path="rebuild.disk_ratio", kind="number", default=2.0, fallback=2.0,
       budget="rebuild.disk_ratio", units="ratio", minimum=1.0,
       doc="Free-disk-to-store-size ratio required before a rebuild may start, so a "
           "rebuild cannot be the thing that fills the disk.", ticket="e1.10"),
    _k(path="watch.cpu_share_pct", kind="number", default=5.0, fallback=5.0,
       budget="watch.budget.cpu_pct", units="percent of one core", minimum=0.5, maximum=100.0,
       doc="What share of one core `fathom watch` may spend while work is flowing. Past it the "
           "poll stretches itself toward its ceiling and says so in a notice, because a budget "
           "nobody can see is a budget nobody fails (ADR-004 §11). The idle watcher is nowhere "
           "near this: `watch.idle_cpu.pct` is the quiet-repository number.", ticket="e1.9"),
    # --- retention (ADR-004 §8.2 + ADR-005 §6.3) ----------------------------
    _k(path="retention.cache_epochs", kind="integer", default=2, minimum=1, units="epochs",
       doc="Live epoch plus the one before, so an interrupted rebuild can be abandoned "
           "without losing the index it replaced.", ticket="e6.1"),
    _k(path="retention.blob_epochs", kind="integer", default=3, minimum=1, units="epochs",
       doc="Blobs are small and deduplicated; the extra epoch is cheaper than the "
           "re-analysis it saves.", ticket="e6.1"),
    _k(path="retention.index_run_keep_last", kind="integer", default=20, minimum=1,
       units="runs", doc="Index-run history is the §11 evidence trail.", ticket="e6.1"),
    _k(path="retention.usage_snapshot_days", kind="integer", default=90, minimum=1,
       units="days", doc="Store-growth history window.", ticket="e6.1"),
    _k(path="retention.store_event_days", kind="integer", default=30, minimum=1,
       units="days",
       doc="Non-error store events age out; `error` rows are kept until read, because "
           "“when did it start happening” is the first question asked.", ticket="e6.1"),
    _k(path="retention.events_days", kind="integer", default=90, minimum=1, units="days",
       doc="Harness event retention (ADR-005 §6.3's `retention_days`).", ticket="e6.1"),
    # --- harness ------------------------------------------------------------
    _k(path="harness.max_bytes_per_session_mib", kind="integer", default=64, minimum=1,
       units="MiB",
       doc="Per-session ingest budget; over quota the session is quarantined rather "
           "than trusted (ADR-005 §6.3).", ticket="e6.1"),
    # --- keymap -------------------------------------------------------------
    _k(path="keys.overrides.<mode>.<binding>", kind="string", merge="deep", default={},
       doc="User key overrides per mode. Overrides only — `e5.4` detects conflicts and "
           "the cheatsheet is generated from the merged table, so neither can lie.",
       ticket="e5.4"),
    # --- mcp ----------------------------------------------------------------
    _k(path="mcp.transport", kind="string", default="stdio", enum=("stdio", "http"),
       doc="`stdio` opens no socket and is not a network feature (ADR-002 §6.3). `http` "
           "is a listener, so it needs `privacy.mcp_http_transport` as well as the "
           "compiled-in feature.", ticket="e6.4"),
    _k(path="mcp.bind", kind="string", default="127.0.0.1",
       pattern=r"^(?:[0-9]{1,3}(?:\.[0-9]{1,3}){3}|localhost|::1)$",
       doc="Listener address, loopback by default. A non-loopback bind is refused without "
           "consent: a listener on a shared host is a disclosure, not a convenience.",
       ticket="e6.4"),
    _k(path="mcp.auth_token", kind="string", default="", scope="user", secret=True,
       doc="Bearer token for the HTTP transport. User-scoped and redacted everywhere: "
           "a token a clone could write is a token that is already leaked.", ticket="e6.4"),
    # --- query interrupts (budget-linked) ----------------------------------
    _k(path="query.frame_store_budget_ms", kind="integer", default=8, fallback=8,
       budget="query.frame_store_budget.ms", units="ms", minimum=1,
       doc="One store call on the keystroke path may take this long before it is "
           "interrupted and rendered as a stated partial (ADR-004 §7.6).", ticket="e5.1"),
    # --- ratchet (ADR-004 §5.1 declares `ratchet.` as graph.cycles' input) --
    _k(path="ratchet.baseline", kind="path", default=".fathom/ratchet.toml",
       doc="The committed baseline of pre-existing violations (class M, ADR-004 §12). "
           "Committed on purpose: a baseline nobody can diff is a baseline nobody trusts.",
       pattern=r"^[^\x00]+$", analyzer="graph.cycles", ticket="e3.10"),
    _k(path="ratchet.mode", kind="string", default="warn", enum=("off", "warn", "deny"),
       doc="What a *new* violation above the baseline does: nothing, a finding, or a failing "
           "`fathom check` (exit 7). `warn` is the default because a ratchet that blocks a "
           "day-one review teaches nothing about the review.",
       analyzer="graph.cycles", ticket="e3.10"),
    # --- privacy / network consent -----------------------------------------
    _k(path="privacy.advisories", kind="boolean", default=False, scope="user",
       doc="Consent to fetch security advisories (OSV). Off by default; needs the "
           "`net-advisories` feature to be compiled in as well.", ticket="e2.7"),
    _k(path="privacy.llm", kind="boolean", default=False, scope="user",
       doc="Consent for optional narrative polish. Off by default; the template is the "
           "degraded answer and is shipped as the good one.", ticket="e4.7"),
    _k(path="privacy.mcp_http_transport", kind="boolean", default=False, scope="user",
       doc="Consent to bind a network listener for MCP. A listener is as much a privacy "
           "decision as an outbound call, so it wears the same gate.", ticket="e6.4"),
    # --- repo ---------------------------------------------------------------
    _k(path="repo.size_class", kind="string", default="auto",
       enum=("auto", "small", "medium", "large", "xlarge"),
       doc="Size class the budget defaults are read from. `auto` probes commit and file "
           "counts and takes the *stricter* of the two (ADR-004 §11.1); naming a class "
           "here is how you pay the xlarge budget on a machine that needs it.",
       ticket="e0.11"),
]

KEYS: dict[str, Key] = {k.path: k for k in KEY_LIST}

#: Two names, one knob. ADR-004 §5.1 declares the config key `ingest.size_cap_mib` and
#: §11 publishes the budget as `ingest.file_size_cap.mib`; the alias table is the only
#: place that says they are the same number, and a test refuses drift between them.
BUDGET_ALIASES: dict[str, str] = {
    k.path: k.budget for k in KEY_LIST if k.budget and k.budget != k.path
}

#: Keys whose *glob* values are matched against a repository path. `config explain
#: --path P` is built on this list: it is the honest answer to "which config decided
#: something about this file", and `e0.9`'s taxonomy consumes exactly these inputs.
PATH_KEYS: tuple[str, ...] = (
    "noise.globs", "test.globs", "non_code.patterns", "graph.ignored_dirs",
    "layers.<layer-name>", "taxonomy.classes.<class-name>",
    "paths.store", "paths.objects", "paths.events", "paths.export",
)

#: Keys whose value must never leave the machine redacted-free.
_SECRET_RE = re.compile(r"(token|secret|password|passwd|credential|api_?key)", re.I)

# ---------------------------------------------------------------------------
# lookup
# ---------------------------------------------------------------------------
def key_for(path: str) -> Key | None:
    """The :class:`Key` governing `path`, resolving `<placeholder>` segments."""
    if path in KEYS:
        return KEYS[path]
    segs = path.split(".")
    for cand in KEY_LIST:
        csegs = cand.segments()
        if len(csegs) != len(segs):
            continue
        for want, have in zip(csegs, segs):
            if want in PLACEHOLDERS:
                if not re.match(PLACEHOLDERS[want]["pattern"], have):
                    break
            elif want != have:
                break
        else:
            return cand
    return None


def is_known(path: str) -> bool:
    return key_for(path) is not None


def scope_of(path: str) -> str:
    """`any` or `user`; unknown paths are `any` (the unknown-key warning covers them)."""
    key = key_for(abstract(path))
    return key.scope if key else "any"


def concrete_paths() -> list[str]:
    """Every declared key path; open maps appear in their `<placeholder>` form."""
    return sorted(KEYS)


def numeric_range(path: str):
    key = key_for(path)
    if not key:
        return None, None
    return key.minimum, key.maximum


def nearest(path: str, *, limit: int = 3) -> list[str]:
    """Nearest-name suggestion for an unrecognised key (AC: "warn with a nearest-name
    suggestion instead of failing silently").

    The candidate set is *all* key paths with placeholders rendered as `<…>` and, for
    unknown keys inside a known table, the siblings of that table — so `[risk] weightz =
    1` suggests `risk.weights`, and `[nise]` suggests `noise`. Candidates are also
    offered for a *table* the file created but that has no keys in it, which is how
    `[layes] core = [...]` gets caught rather than ignored.
    """
    candidates = set(KEYS)
    head = path.rsplit(".", 1)[0] if "." in path else ""
    if head:
        candidates.update(p for p in KEYS if p.rsplit(".", 1)[0] == head)
    # a mistyped table: offer the tables themselves
    tables = {p.split(".")[0] for p in KEYS}
    if "." not in path:
        candidates.update(tables)
    scored = difflib.get_close_matches(path, sorted(candidates), n=limit, cutoff=0.4)
    if not scored and "." not in path:
        scored = difflib.get_close_matches(path, sorted(tables), n=limit, cutoff=0.2)
    return scored


def is_secret(path: str) -> bool:
    """Whether a value must be redacted in every rendered output.

    Both a declared `secret=True` and a name that looks like a credential count,
    because a user-scoped `mcp.auth_token` is not the only place a person will put a
    secret: an override map keyed `deploy_api_key` happens in real config files.
    """
    key = key_for(path)
    if key is not None and key.secret:
        return True
    return bool(_SECRET_RE.search(path.rsplit(".", 1)[-1]))


# ---------------------------------------------------------------------------
# defaults and coercion
# ---------------------------------------------------------------------------
def defaults(budgets: dict | None = None) -> dict:
    """Build the `defaults` layer as a nested dict. Pure: no files, no env (W1).

    `budgets` maps a budget *family* (`cache.budget.mib`) to the value for the resolved
    size class; the CLI half builds it from `benchmarks/budgets.json`. Without it the
    documented fallback is used, which is what makes this function usable — and
    testable — on its own.
    """
    out: dict = {}
    for key in KEY_LIST:
        segs = key.segments()
        if key.is_open():
            # an open map's default is the whole table, keyed by the placeholder's values
            segs = [s for s in segs if s not in PLACEHOLDERS]
        _insert(out, segs, key.effective_default(budgets))
    return out


def _insert(tree: dict, segments: list[str], value) -> None:
    cur = tree
    for seg in segments[:-1]:
        cur = cur.setdefault(seg, {})
    if value is not None:
        cur[segments[-1]] = value


def flatten(tree: dict, prefix: str = "") -> dict[str, object]:
    """`{"risk": {"tau": 70}}` -> `{"risk.tau": 70}`.

    Open maps stay concrete (`layers.ui`, `risk.weights.size`): the flattened form is
    what `explain` renders one line per, and what a precedence test asserts on.
    """
    out: dict[str, object] = {}
    for k, v in tree.items():
        path = f"{prefix}.{k}" if prefix else k
        if isinstance(v, dict):
            out.update(flatten(v, path))
        else:
            out[path] = v
    return out


def abstract(path: str) -> str:
    """Concrete `layers.ui` -> declared `layers.<layer-name>` where an open map owns it."""
    segs = path.split(".")
    for cand in KEY_LIST:
        csegs = cand.segments()
        if len(csegs) != len(segs):
            continue
        ok = True
        for want, have in zip(csegs, segs):
            if want in PLACEHOLDERS:
                if not re.match(PLACEHOLDERS[want]["pattern"], have):
                    ok = False
                    break
            elif want != have:
                ok = False
                break
        if ok:
            return cand.path
    return path


def coerce(key: Key, raw: str, *, source: str, where: str = "") -> object:
    """Turn a string (from `--set` or an env var) into the key's typed value.

    Strict on purpose: `FATHOM_RISK__TAU=seventy` must produce exit 5 naming the env
    var, not a silently defaulted threshold.
    """
    if key.kind in ("string", "path", "glob"):
        value: object = raw
    elif key.kind == "boolean":
        low = raw.strip().lower()
        if low in ("1", "true", "yes", "on"):
            value = True
        elif low in ("0", "false", "no", "off", ""):
            value = False
        else:
            raise ConfigInvalid(
                "expected a boolean for %s, got %r" % (where or key.path, raw),
                source=source, hint="use one of true/false, 1/0, yes/no")
    elif key.kind in ("integer", "number"):
        try:
            value = int(raw) if key.kind == "integer" else float(raw)
        except ValueError:
            raise ConfigInvalid(
                "expected an %s for %s, got %r" % (key.kind, where or key.path, raw),
                source=source, hint="e.g. `--set %s`" % _example(key)) from None
    elif key.kind in ("string_list", "glob_list"):
        value = [p.strip() for p in raw.split(",") if p.strip()]
    else:  # pragma: no cover - KINDS is closed
        raise AssertionError("unhandled kind %r" % key.kind)

    where = where or key.path
    if key.enum:
        values = value if isinstance(value, list) else [value]
        for item in values:
            if item not in key.enum:
                raise ConfigInvalid(
                    "%s=%r is not one of %s" % (where, item, ", ".join(key.enum)),
                    source=source, hint="a typo here changes behaviour silently, so it "
                                        "is refused rather than ignored")
    if key.pattern and isinstance(value, str) and not re.match(key.pattern, value):
        raise ConfigInvalid("%s=%r does not look like a valid value" % (where, value),
                            source=source)
    _check_range(key, value, source, where)
    return value


def _check_range(key: Key, value, source: str, where: str) -> None:
    if isinstance(value, dict):
        for name, item in value.items():
            if isinstance(item, (int, float)):
                _one_range(key, item, "%s.%s" % (where, name), source)
    elif isinstance(value, (int, float)):
        _one_range(key, value, where, source)


def _one_range(key: Key, value, where: str, source: str) -> None:
    if key.minimum is not None and value < key.minimum:
        raise ConfigInvalid("%s=%s is below the minimum %s" % (where, value, key.minimum),
                            source=source, hint="see `fathom config schema`")
    if key.maximum is not None and value > key.maximum:
        raise ConfigInvalid("%s=%s is above the maximum %s" % (where, value, key.maximum),
                            source=source, hint="see `fathom config schema`")


def _example(key: Key) -> str:
    if key.enum:
        return "%s=%s" % (key.path, key.enum[0])
    return "%s=%s" % (key.path, key.fallback if key.is_budget() else key.default)


def _entry_example(key: Key) -> str:
    for seg in key.segments():
        if seg in PLACEHOLDERS:
            return "%s=… " % key.path.replace(seg, "<%s>" % seg.strip("<>"))
    return key.path


def env_name(path: str) -> str:
    """`cache.budget.mib` -> `FATHOM_CACHE__BUDGET__MIB`."""
    return ENV_PREFIX + path.replace(".", ENV_SEP).upper()


def env_path(name: str) -> str:
    """The inverse; raises nothing, returns "" for names that are not config."""
    if not name.startswith(ENV_PREFIX):
        return ""
    body = name[len(ENV_PREFIX):]
    return body.lower().replace(ENV_SEP, ".")


def documented_env_names() -> list[str]:
    return [env_name(p) for p in concrete_paths() if not key_for(p).is_open()]
