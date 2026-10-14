"""The path-class vocabulary: what a path **is**, and the default noise that implies.

This is the whole vocabulary of the `e0.9` (`reco-5vz`) taxonomy on one axis. It is a
closed list on purpose: the ticket names twelve classes, the store's `path_class` column
(ADR-004 §4.3) stores their strings, and a classifier that invented a thirteenth would
write a value no view knows how to render. `tests/test_taxonomy_contract.py` refuses a
class that is in this table but not documented, and one that is documented but not here.

Two axes, deliberately separate
-------------------------------

* **class** — what the path *is*: a reviewable surface or a derived one. Decided by rules.
* **noise** — how much reviewable information a change to it carries. Derived from the
  class, then overridden by explicit noise globs (`noise.globs` in `fathom.toml`).

They are not the same claim and are not allowed to collapse into each other. A `.png`
is `asset` (a real surface a human may want to look at) and *not* noise: it is simply
not diffable, which is a fact about the diff viewer (`e1.7`), not a reason to hide the
change. A lockfile *is* noise-by-default: its content is derived state, and the glossary
defines noise as "change that carries no reviewable information"
([thesis](../../../docs/concepts/thesis.md#4-glossary)). Keeping the axes apart is also
what lets `!`-negation work: re-promoting a path out of noise does not change what it is.

`other` is a real answer, not a failure. `e2.1`'s acceptance criterion is that ambiguous
paths land in `other` rather than being guessed into `source`; a taxonomy that defaults to
"code" makes every noise-share number a lie the moment an unusual extension shows up.
"""

from __future__ import annotations


class PathClass:
    """The twelve classes. Strings, because they go into SQLite and into `explain` output."""

    SOURCE = "source"
    TEST = "test"
    DOCS = "docs"
    CONFIG = "config"
    SCHEMA = "schema"
    MIGRATION = "migration"
    GENERATED = "generated"
    VENDORED = "vendored"
    LOCKFILE = "lockfile"
    ASSET = "asset"
    BINARY = "binary"
    OTHER = "other"


#: Declaration order is display order: reviewable surfaces first, derived ones next, then
#: the things a reviewer only ever wants counted. `fathom taxonomy classes` prints this.
PATH_CLASSES: tuple[str, ...] = (
    PathClass.SOURCE, PathClass.TEST, PathClass.DOCS, PathClass.CONFIG, PathClass.SCHEMA,
    PathClass.MIGRATION, PathClass.GENERATED, PathClass.VENDORED, PathClass.LOCKFILE,
    PathClass.ASSET, PathClass.BINARY, PathClass.OTHER,
)

#: Classes a *user* may assert in `[taxonomy.classes]`. `other` is excluded: asserting it
#: would be indistinguishable from matching nothing, and would hide the rule that did it.
ASSERTABLE_CLASSES: tuple[str, ...] = tuple(c for c in PATH_CLASSES if c != PathClass.OTHER)

#: Per class: the default noise level it implies, whether a text diff means anything for
#: it, whether a change here is signal a reviewer is expected to read, and the one-line
#: definition `explain`, `fathom taxonomy classes` and the docs all render from.
CLASS_INFO: dict[str, dict] = {
    PathClass.SOURCE: {
        "title": "Source", "noise": "informative", "diffable": True, "reviewable": True,
        "doc": "Code a human wrote or is expected to read as logic.",
    },
    PathClass.TEST: {
        "title": "Test", "noise": "informative", "diffable": True, "reviewable": True,
        "doc": "The test surface: suites, specs, fixtures, benches, stories. Not a "
               "coverage number (`e2.9`); a set of paths that must move with source.",
    },
    PathClass.DOCS: {
        "title": "Docs", "noise": "informative", "diffable": True, "reviewable": True,
        "doc": "Prose a reviewer reads: markdown, rst, adoc, RFCs, ADRs, changelogs.",
    },
    PathClass.CONFIG: {
        "title": "Config", "noise": "informative", "diffable": True, "reviewable": True,
        "doc": "Build, tool, CI and runtime configuration: manifests, TOML/YAML/JSON, "
               "Dockerfiles, workflows, IaC. A non-code surface (`e2.8`) that is still "
               "hand-written.",
    },
    PathClass.SCHEMA: {
        "title": "Schema", "noise": "informative", "diffable": True, "reviewable": True,
        "doc": "Contract definitions: protobuf/GraphQL/Avro/OpenAPI/JSON Schema, DDL and "
               "query files. Changing one changes a promise, which is why it is its own "
               "class and not `config`.",
    },
    PathClass.MIGRATION: {
        "title": "Migration", "noise": "informative", "diffable": True, "reviewable": True,
        "doc": "Ordered, executed-once changes to durable state: `migrations/`, "
               "`db/migrate`, alembic versions. The highest-consequence class in the list "
               "and the one an operator has to schedule (`e2.8`).",
    },
    PathClass.GENERATED: {
        "title": "Generated", "noise": "generated", "diffable": True, "reviewable": False,
        "doc": "Tool output committed to the tree: protobuf and gRPC stubs, `*_generated.rs`, "
               "`dist/`, snapshots, storybook and openapi codegen, bundler output.",
    },
    PathClass.VENDORED: {
        "title": "Vendored", "noise": "vendored", "diffable": True, "reviewable": False,
        "doc": "Someone else's code, inside the tree: `vendor/`, `node_modules/`, "
               "`third_party/`, committed site-packages.",
    },
    PathClass.LOCKFILE: {
        "title": "Lockfile", "noise": "lockfile", "diffable": True, "reviewable": False,
        "doc": "Resolved dependency state. Derived from a manifest plus a resolver run, "
               "which makes it informative to a machine and noise to a human unless the "
               "manifest moved too (`e2.5`, `e2.6`).",
    },
    PathClass.ASSET: {
        "title": "Asset", "noise": "informative", "diffable": False, "reviewable": True,
        "doc": "Media, fonts, images, data files. Real change, no text diff: counted, "
               "listed, and never rendered as a line count.",
    },
    PathClass.BINARY: {
        "title": "Binary", "noise": "informative", "diffable": False, "reviewable": False,
        "doc": "Compiled or packed artefacts: `.so`, `.jar`, `.zip`, `.wasm`, `.exe`. A "
               "committed binary is worth flagging, not worth diffing.",
    },
    PathClass.OTHER: {
        "title": "Other", "noise": "informative", "diffable": True, "reviewable": True,
        "doc": "Nothing matched. An honest unknown, surfaced rather than guessed into "
               "`source`; a large `other` count is a signal that this table is missing a "
               "convention for this repository, and `fathom explain` names the path that "
               "got away.",
    },
}


def is_known_class(name: str) -> bool:
    return name in CLASS_INFO


def assertable(name: str) -> bool:
    return name in ASSERTABLE_CLASSES


def default_noise(cls: str) -> str:
    """The noise level a class implies before any explicit noise rule speaks."""
    return CLASS_INFO.get(cls, CLASS_INFO[PathClass.OTHER])["noise"]


def is_diffable(cls: str) -> bool:
    return bool(CLASS_INFO.get(cls, CLASS_INFO[PathClass.OTHER])["diffable"])


def is_reviewable(cls: str) -> bool:
    return bool(CLASS_INFO.get(cls, CLASS_INFO[PathClass.OTHER])["reviewable"])
