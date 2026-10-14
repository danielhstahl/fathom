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
    "test": {"title": "Test surface", "owner": "e1.7 / e2.6",
             "doc": "What counts as a test, for the test-surface analyzer."},
    "non_code": {"title": "Non-code surface", "owner": "e2.8 (`reco-bjd`)",
                 "doc": "Config, env, flags, migrations and CI — the surfaces a reviewer "
                        "must be told about even though they are not code."},
    "graph": {"title": "Graph inputs", "owner": "e3.2 (`reco-8s4`)",
              "doc": "Directories excluded from the dependency and structure graph."},
    "layers": {"title": "Layering rules", "owner": "ADR-004 §5.1 / e3.1",
               "doc": "The declared intended architecture: named layers, each a list of "
                      "path prefixes. An open map, keyed by layer name."},
    "deps": {"title": "Dependencies", "owner": "e2.5 (`reco-j83`)",
             "doc": "Which ecosystems' manifests and lockfiles are parsed. Also the "
                    "switchboard for the advisory lookup's *subject*, not its consent."},
    "risk": {"title": "Risk weights and threshold", "owner": "e2.9 / e2.10",
             "doc": "The ranking knobs. Changing these changes `risk.score` and "
                    "`delta.compose` and nothing else (ADR-004 §6)."},
    "detectors": {"title": "Detectors", "owner": "e2.7",
                  "doc": "Which finding detectors run."},
    "blast": {"title": "Blast radius", "owner": "e2.10",
              "doc": "How far the blast-radius walk goes before it declares a partial."},
    "rename": {"title": "Rename detection", "owner": "e1.6",
               "doc": "Similarity threshold for calling two paths the same file."},
    "diff": {"title": "Diff rendering", "owner": "e1.6",
             "doc": "Per-file diff knobs."},
    "episode": {"title": "Episode segmentation", "owner": "ADR-003 / e1.3",
                "doc": "Boundary sources in priority order, and the idle gap that ends an "
                       "episode when nothing better is available."},
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
    _k(path="test.globs", kind="glob_list", merge="extend",
       default=["tests/**", "**/test_*.py"], item_pattern=GLOB_ITEM,
       doc="Paths counted as test surface.", analyzer="test.surface", ticket="e2.6"),
    _k(path="non_code.patterns", kind="glob_list", merge="extend",
       default=["**/migrations/**", ".github/workflows/**"], item_pattern=GLOB_ITEM,
       doc="Non-code surfaces a reviewer must be told about: config, env, flags, "
           "migrations and CI.", analyzer="non_code.surface", ticket="e2.8"),
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
    _k(path="blast.hops", kind="integer", default=1, minimum=1, maximum=4, units="hops",
       doc="Blast-radius walk depth; exceeding it is a stated partial, not a hang.",
       analyzer="blast_radius", ticket="e2.10"),
    _k(path="rename.threshold", kind="integer", default=60, minimum=0, maximum=100,
       units="percent", doc="Similarity needed to call two paths the same file.",
       analyzer="diff.rename_detect", ticket="e1.6"),
    _k(path="diff.context_lines", kind="integer", default=3, minimum=0, maximum=20,
       units="lines", doc="Context lines per diff hunk.", analyzer="diff.per_file",
       ticket="e1.6"),
    # --- episode ------------------------------------------------------------
    _k(path="episode.boundaries", kind="string_list", merge="replace",
       default=["hook", "trailer", "author"],
       enum=("hook", "trailer", "author", "time", "merge"),
       doc="Boundary sources in priority order (ADR-003). `replace`, not `extend`: the "
           "*order* is the meaning, and a merged list has no coherent order.",
       analyzer="episode.segment", ticket="e1.3"),
    _k(path="episode.time_gap_minutes", kind="integer", default=30, minimum=1,
       maximum=1440, units="minutes",
       doc="Idle gap that closes an episode when no stronger boundary source fires.",
       analyzer="episode.segment", ticket="e1.3"),
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
