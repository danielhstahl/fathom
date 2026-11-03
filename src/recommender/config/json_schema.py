"""The JSON Schema for `fathom.toml`, generated — plus a validator small enough to ship.

Two jobs, one source of truth.

**Generate.** `schemas/fathom.schema.json` is editor tooling (ADR-002 §6.1, the
`fathom-cli` `schema-gen` feature: "CI regenerates and diffs the checked-in file").
Every `fathom.toml` a human will ever write is written against this file, so it is not
allowed to be hand-maintained: it is rendered from
:mod:`recommender.config.schema` and CI fails on drift, in the same style as
`tools/gen_adr_tables.py` and ADR-002's generated tables. A schema that drifts from the
loader is worse than no schema, because the editor will be the confident one and it will
be wrong.

**Validate.** `fathom.toml failed the schema` is the definition of exit code 5 in
ADR-002 §7.3, so *something* has to validate. This validator is a deliberately small
subset of JSON Schema — the subset :func:`build_schema` can emit — rather than a
dependency:

* the repository's contract suites are stdlib-only by policy (see `tests/conftest.py`:
  "a check that needs an install is a check that gets skipped");
* the subset is closed and small: `type`, `enum`, `pattern`, `properties`,
  `patternProperties`, `propertyNames`, `additionalProperties` (bool or schema),
  `items`, `required`, `uniqueItems`, `minimum`, `maximum`. No `$ref`, no
  `oneOf`/`anyOf`, no conditionals — because every construct added here is a construct
  the *Rust* deserializer must reproduce, and `oneOf` in a config schema is a
  deserializer that cannot tell you which branch you meant.

Errors come back as data (`SchemaError` with a key path and a pointer) so the CLI can
print `fathom.toml:12: …` rather than a traceback, and so `config lint` can be checked
by a test.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

from . import schema as S
from .command import command

DRAFT = "https://json-schema.org/draft/2020-12/schema"
SCHEMA_ID = "https://fathom.dev/schemas/fathom.schema.json"
#: The remedy every drift message prints. Spelled through :mod:`.command` because the shorter
#: `fathom config schema --write` names a verb the shipped binary does not have.
GENERATOR = command("schema", "--write")
CHECKED_IN = "schemas/fathom.schema.json"

#: The annotation keys this generator emits. Editor tooling ignores `x-*`; our own
#: `explain` reads them, which is why they are generated rather than implied.
ANNOTATIONS = ("x-fathom-merge", "x-fathom-scope", "x-fathom-units", "x-fathom-budget",
               "x-fathom-analyzer", "x-fathom-ticket", "x-fathom-secret",
               "x-fathom-deprecated")


@dataclass(frozen=True)
class SchemaError:
    """One validation failure. `path` is dotted config syntax, `pointer` is JSON's."""

    path: str
    pointer: str
    message: str
    kind: str = "invalid"
    found: object = None

    def render(self) -> str:
        return "%s: %s" % (self.path or "<root>", self.message)


# ---------------------------------------------------------------------------
# generation
# ---------------------------------------------------------------------------
def _leaf(key: S.Key) -> dict:
    node = key.json_shape()
    node["description"] = key.doc
    if key.is_budget() and key.fallback is not None:
        node["default"] = key.fallback
        node["x-fathom-budget-default-for-size-class"] = True
    elif key.default is not None and not key.is_open():
        node["default"] = key.default
    if key.units:
        node["x-fathom-units"] = key.units
    node["x-fathom-merge"] = key.merge
    node["x-fathom-scope"] = key.scope
    if key.is_open():
        node["x-fathom-open"] = [S.PLACEHOLDERS[s]["doc"] for s in key.open_segments()]
    if key.budget:
        node["x-fathom-budget"] = key.budget
    if key.analyzer:
        node["x-fathom-analyzer"] = key.analyzer
    if key.ticket:
        node["x-fathom-ticket"] = key.ticket
    if key.secret:
        node["x-fathom-secret"] = True
    if key.deprecated:
        node["x-fathom-deprecated"] = key.deprecated
    return node


def _object_node(description: str) -> dict:
    return {"type": "object", "description": description, "additionalProperties": False}


def _attach(node: dict, segments: list[str], key: S.Key) -> None:
    """Hang one key off an object node, creating intermediates and pattern maps.

    One recursive function covers the fixed keys, the open maps (`layers.<layer-name>`,
    `risk.weights.<weight>`) and the doubly-open one
    (`keys.overrides.<mode>.<binding>`) with no special cases — which matters, because a
    special case in a generator is where the generator and the loader stop agreeing.
    """
    seg, *rest = segments
    if seg in S.PLACEHOLDERS:
        pattern = S.PLACEHOLDERS[seg]["pattern"]
        child = node.setdefault("patternProperties", {}).setdefault(pattern, {})
        child.clear()
        child.update(_object_node(key.doc) if rest else _leaf(key))
        if rest:
            _attach(child, rest, key)
        return
    props = node.setdefault("properties", {})
    if not rest:
        props[seg] = _leaf(key)
        return
    child = props.setdefault(seg, _object_node(key.doc))
    _attach(child, rest, key)


def build_schema() -> dict:
    """The full document for `schemas/fathom.schema.json`."""
    sections: dict[str, dict] = {}
    for key in S.KEY_LIST:
        head, *rest = key.segments()
        section = sections.setdefault(head, _object_node(S.SECTIONS[head]["doc"]))
        section["x-fathom-section-title"] = S.SECTIONS[head]["title"]
        section["x-fathom-owner"] = S.SECTIONS[head]["owner"]
        if rest:
            _attach(section, rest, key)

    doc = {
        "$schema": DRAFT,
        "$id": SCHEMA_ID,
        "title": "Fathom configuration (`fathom.toml`)",
        "description": (
            "Configuration for Fathom, the terminal UI that keeps a human's mental model "
            "of a codebase a coding agent is writing. Values are layered: command-line "
            "flags > environment variables (FATHOM_*, segments joined by __) > repo "
            "fathom.toml > user config > built-in defaults, and every effective value is "
            "explainable with `fathom config explain`. GENERATED from "
            "src/recommender/config/schema.py by `%s`; do not edit by hand." % GENERATOR),
        "type": "object",
        "properties": sections,
        "additionalProperties": False,
        "x-fathom": {
            "generator": GENERATOR,
            "config_version": S.CONFIG_VERSION,
            "layers": list(S.Layer.ORDER),
            "precedence": " > ".join(S.Layer.ORDER),
            "env_prefix": S.ENV_PREFIX,
            "env_separator": S.ENV_SEP,
            "sections": {name: {"title": meta["title"], "owner": meta["owner"]}
                         for name, meta in S.SECTIONS.items()},
            "budget_aliases": dict(sorted(S.BUDGET_ALIASES.items())),
        },
    }
    return doc


def serialize(doc: dict | None = None) -> str:
    """Byte-stable rendering. `sort_keys` matters: a generated file whose diff is
    ordering noise teaches nobody anything."""
    return json.dumps(doc if doc is not None else build_schema(), indent=2,
                      sort_keys=True) + "\n"


def drift(expected_path) -> tuple[bool, str]:
    """`(ok, message)` for CI: regenerate and compare, never silently rewrite."""
    from pathlib import Path

    path = Path(expected_path)
    want = serialize()
    if not path.exists():
        return False, "%s is missing — run `%s`" % (path, GENERATOR)
    have = path.read_text(encoding="utf-8")
    if have != want:
        return False, ("%s is stale — run `%s` (diff: %s)"
                       % (path, GENERATOR, _first_diff(have, want)))
    return True, ""


def _first_diff(have: str, want: str) -> str:
    for i, (a, b) in enumerate(zip(have.splitlines(), want.splitlines())):
        if a != b:
            return "line %d: have %r, want %r" % (i + 1, a.strip(), b.strip())
    return "line count differs"


# ---------------------------------------------------------------------------
# validation (the subset this generator emits)
# ---------------------------------------------------------------------------
_TYPE_CHECKS = {
    "object": lambda v: isinstance(v, dict),
    "array": lambda v: isinstance(v, list),
    "string": lambda v: isinstance(v, str),
    "boolean": lambda v: isinstance(v, bool),
    "integer": lambda v: isinstance(v, int) and not isinstance(v, bool),
    "number": lambda v: isinstance(v, (int, float)) and not isinstance(v, bool),
}


def validate(instance, schema: dict, *, path: str = "", pointer: str = "") -> list[SchemaError]:
    """Validate `instance` against the generated `schema`; returns every problem.

    Collects rather than raises: a config file with three typos should produce three
    lines, not one per run of the tool.
    """
    errors: list[SchemaError] = []
    expected = schema.get("type")
    if expected:
        for one in (expected if isinstance(expected, list) else [expected]):
            if _TYPE_CHECKS[one](instance):
                break
        else:
            errors.append(SchemaError(path, pointer,
                                      "expected %s, found %s" % (expected, _kind_of(instance)),
                                      kind="type", found=instance))
            return errors

    if "enum" in schema and instance not in schema["enum"]:
        errors.append(SchemaError(path, pointer, "%r is not one of %s"
                                  % (instance, ", ".join(repr(v) for v in schema["enum"])),
                                  kind="enum", found=instance))
    if isinstance(instance, str) and "pattern" in schema:
        if not re.match(schema["pattern"], instance):
            errors.append(SchemaError(path, pointer, "%r does not match %s"
                                      % (instance, schema["pattern"]), kind="pattern",
                                      found=instance))
    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        if "minimum" in schema and instance < schema["minimum"]:
            errors.append(SchemaError(path, pointer, "%s is below the minimum %s"
                                      % (instance, schema["minimum"]), kind="minimum",
                                      found=instance))
        if "maximum" in schema and instance > schema["maximum"]:
            errors.append(SchemaError(path, pointer, "%s is above the maximum %s"
                                      % (instance, schema["maximum"]), kind="maximum",
                                      found=instance))

    if isinstance(instance, list):
        item_schema = schema.get("items")
        if schema.get("uniqueItems") and len({json.dumps(v, sort_keys=True) for v in instance}
                                             ) != len(instance):
            errors.append(SchemaError(path, pointer, "list contains duplicates",
                                      kind="uniqueItems", found=instance))
        if item_schema:
            for i, item in enumerate(instance):
                errors.extend(validate(item, item_schema,
                                       path="%s[%d]" % (path, i),
                                       pointer="%s/items/%d" % (pointer, i)))

    if isinstance(instance, dict):
        props = schema.get("properties", {})
        patterns = schema.get("patternProperties", {})
        extra = schema.get("additionalProperties", True)
        for name, value in instance.items():
            child_path = "%s.%s" % (path, name) if path else name
            child_pointer = "%s/%s" % (pointer, name)
            matched = False
            if name in props:
                matched = True
                errors.extend(validate(value, props[name], path=child_path,
                                       pointer=child_pointer))
            for pattern, sub in patterns.items():
                if re.match(pattern, name):
                    matched = True
                    errors.extend(validate(value, sub, path=child_path, pointer=child_pointer))
            if not matched:
                if extra is False:
                    errors.append(SchemaError(child_path, child_pointer,
                                              "unknown key %r" % name, kind="unknown",
                                              found=value))
                elif isinstance(extra, dict):
                    errors.extend(validate(value, extra, path=child_path,
                                           pointer=child_pointer))
            if "propertyNames" in schema and not re.match(
                    schema["propertyNames"].get("pattern", ".*"), name):
                errors.append(SchemaError(child_path, child_pointer,
                                          "%r is not a valid name here (%s)"
                                          % (name, schema["propertyNames"]["pattern"]),
                                          kind="propertyNames", found=name))
        for required in schema.get("required", []):
            if required not in instance:
                errors.append(SchemaError(path, pointer, "required key %r is missing"
                                          % required, kind="required"))
    return errors


def _kind_of(value) -> str:
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, dict):
        return "object"
    if isinstance(value, list):
        return "array"
    if isinstance(value, str):
        return "string"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    return "null"
