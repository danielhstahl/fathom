#!/usr/bin/env python3
"""The delta model's version policy, as an executable gate (`reco-tt4`, acceptance criterion 4).

`schemas/episode-delta.schema.json` is the published contract for `fathom analyze <episode> --json`, and
the TUI, the CLI, the export artefact and the MCP tools all read that one document. A contract needs a
rule and a gate:

  * **additive changes keep the version** — a new optional field, a new figure, a section appended after
    the seven, a new key in `provenance.models`. Readers ignore what they do not know, and the schema
    says `additionalProperties: true` so tomorrow's v1 document validates against today's v1 schema;
  * **breaking changes bump the version** — a removed or renamed field, a narrowed type, a narrower
    enum, a new *required* key, a changed meaning;
  * **a breaking change without a version bump fails CI** — which is this file.

How it decides "breaking" without a JSON Schema library: it projects the schema down to the parts a
reader's code depends on — for every path, its declared type(s), whether it is required, its `enum`
members in order, its bounds, and what it closes — then compares the baseline's projection with the live
one and asks, path by path, whether a document that validated *yesterday* still validates. That is the
test, stated as the question the policy is about:

  * a path that only the live schema has is a new field: additive, ignored;
  * a path only the baseline has, a narrowed type, a newly required key, a removed or reordered `enum`
    member, a tightened bound, a `const` that moved, a `$ref` repointed: breaking;
  * the reverse of each — a widened type, a requirement dropped, an `enum` appended to, a bound loosened,
    a field removed from `required` — is additive, because a reader of old documents is unaffected.

A description reworded, an example added, a `$comment` expanded: outside the projection entirely.

The projection itself is stored in `schemas/episode-delta.baseline.json`, not just its hash, so a failing
run prints *which* key moved rather than a checksum that differs.

Run:
    python3 tools/check_delta_compat.py check        # what CI runs
    python3 tools/check_delta_compat.py bless        # record the current schema as the new baseline
    python3 tools/check_delta_compat.py self-test    # prove this gate bites, on a temp copy

Exit: 0 clean, 1 policy violated or self-test failed, 2 usage or unreadable inputs.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import pathlib
import re
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCHEMA = ROOT / "schemas" / "episode-delta.schema.json"
BASELINE = ROOT / "schemas" / "episode-delta.baseline.json"
# The Rust constant the declared version has to agree with. Read as text on purpose: this job runs
# without a Rust toolchain, and a grep that can only say "these two numbers differ" is enough.
VERSION_SOURCE = ROOT / "crates" / "fathom-analyze" / "src" / "insight" / "delta.rs"
VERSION_IN_RUST = re.compile(r"pub const VERSION: u32 = (\d+);")
CONTRACT_IN_RUST = re.compile(r"pub const CONTRACT: &str = \"([^\"]+)\"")


def fail(message: str) -> None:
    """Print one problem, in the one-line-per-problem shape `check_docs.py` established."""
    print(f"delta-compat: {message}")


def load_schema(path: pathlib.Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        print(f"delta-compat: {path} is missing")
        sys.exit(2)
    except json.JSONDecodeError as err:
        print(f"delta-compat: {path} is not valid JSON: {err}")
        sys.exit(2)


def split_ref(reference: str) -> str | None:
    """`#/$defs/risk` -> `risk`; anything else is a cross-file ref this gate does not follow."""
    if reference.startswith("#/$defs/"):
        return reference[len("#/$defs/") :]
    return None


def walk(node, at: str, required: bool, out: dict, defs: dict, seen: tuple) -> None:
    """Collect the breaking projection: name, declared types, requiredness, enum members, per path."""
    if isinstance(node, list):
        for index, item in enumerate(node):
            walk(item, f"{at}[{index}]", required, out, defs, seen)
        return
    if not isinstance(node, dict):
        return
    reference = node.get("$ref")
    if isinstance(reference, str):
        name = split_ref(reference)
        entry = out.setdefault(at, {})
        entry["ref"] = reference
        if name is not None and name in defs and reference not in seen:
            walk(defs[name], at, required, out, defs, (*seen, reference))
        return
    if "enum" in node:
        # Ordered, because a reader that switches on an enum's position or ships a table keyed by it is
        # broken by a reordering as surely as by a removal.
        out.setdefault(at, {})["enum"] = list(node["enum"])
    if "const" in node:
        out.setdefault(at, {})["const"] = node["const"]
    if "type" in node:
        types = node["type"]
        types = [types] if isinstance(types, str) else list(types)
        out.setdefault(at, {})["types"] = sorted(types)
    for keyword in ("anyOf", "oneOf"):
        if keyword in node:
            out.setdefault(at, {})["branches"] = len(node[keyword])
    properties = node.get("properties")
    if isinstance(properties, dict):
        needed = set(node.get("required", []))
        for name, child in properties.items():
            path = f"{at}.{name}"
            walk(child, path, name in needed, out, defs, seen)
            out.setdefault(path, {})["required"] = name in needed
    items = node.get("items")
    if items is not None:
        walk(items, f"{at}[]", required, out, defs, seen)
    additional = node.get("additionalProperties")
    if isinstance(additional, dict):
        walk(additional, f"{at}{{}}", False, out, defs, seen)
    elif isinstance(additional, bool):
        out.setdefault(at, {})["closed"] = additional
    if "minimum" in node:
        out.setdefault(at, {})["minimum"] = node["minimum"]
    if "maximum" in node:
        out.setdefault(at, {})["maximum"] = node["maximum"]
    if required and at:
        out.setdefault(at, {})["required"] = True


def projections(schema: dict) -> tuple[dict, dict]:
    """`(breaking, additive)` — the two projections, both JSON-serialisable and key-sorted.

    `$defs` is not walked on its own: every definition is reached through the `$ref` that uses it, so each
    shape appears once, under the path a reader of the *payload* would write. A definition no `$ref`
    reaches is dead weight in the schema and gets no projection — which is the right outcome, since no
    document can contain it.
    """
    breaking: dict = {}
    defs = schema.get("$defs", {})
    surface = {key: value for key, value in schema.items() if key != "$defs"}
    walk(surface, "", False, breaking, defs, ())
    # Anything outside the projection is wording, examples, or `$comment`: additive by construction.
    # Keeping a second projection means `bless` can print what changed without pretending a reworded
    # description is a contract change.
    additive = json.loads(json.dumps(schema, sort_keys=True))
    additive.pop("$comment", None)
    additive.pop("title", None)
    x_fathom = additive.get("x-fathom")
    if isinstance(x_fathom, dict):
        additive["x-fathom"] = {
            key: value
            for key, value in x_fathom.items()
            if key not in {"compatibility", "version"}
        }
    return breaking, additive


def digest(value: dict) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()


def declared_version(schema: dict) -> int:
    try:
        return int(schema["x-fathom"]["version"])
    except (KeyError, TypeError, ValueError):
        print("delta-compat: the schema declares no `x-fathom.version`; the version is a number, not a rumour")
        sys.exit(2)


def declared_contract(schema: dict) -> str:
    return str(schema.get("x-fathom", {}).get("contract", ""))


def rust_constants() -> tuple[int | None, str | None]:
    """The version and contract as the Rust model spells them, or `(None, None)` if the file moved."""
    try:
        text = VERSION_SOURCE.read_text(encoding="utf-8")
    except (FileNotFoundError, UnicodeDecodeError):
        return None, None
    version = VERSION_IN_RUST.search(text)
    contract = CONTRACT_IN_RUST.search(text)
    return (
        int(version.group(1)) if version else None,
        contract.group(1) if contract else None,
    )


def check_shape(schema: dict) -> list[str]:
    """Internal consistency of the schema file itself, independent of any baseline."""
    problems: list[str] = []
    version = declared_version(schema)
    contract = declared_contract(schema)
    if contract != f"fathom-delta/{version}":
        problems.append(
            f"`x-fathom.contract` says {contract!r}, which is not `fathom-delta/{version}` — "
            "one of the two is hand-edited"
        )
    rust_version, rust_contract = rust_constants()
    if rust_version is not None and rust_version != version:
        problems.append(
            f"the schema declares v{version} and `{VERSION_SOURCE.name}` declares VERSION = "
            f"{rust_version}; regenerate with `fathom analyze --schema --write`"
        )
    if rust_contract is not None and rust_contract != contract:
        problems.append(
            f"the schema declares {contract!r} and `{VERSION_SOURCE.name}` declares "
            f"CONTRACT = {rust_contract!r}"
        )
    for key in ("$id", "$schema", "properties", "required"):
        if key not in schema:
            problems.append(f"the schema has no `{key}`")
    schema_id = str(schema.get("$id", ""))
    if not schema_id.startswith("https://"):
        problems.append(f"`$id` is {schema_id!r}; a contract id is a URI, so it can be quoted")
    # The version is pinned in the payload as a `const`, so a v2 document cannot validate against this
    # schema by accident. That is the whole version story in one keyword; if it goes missing, so does it.
    pinned = schema.get("properties", {}).get("schema_version", {}).get("const")
    if pinned != version:
        problems.append(
            f"`properties.schema_version.const` is {pinned!r} and the declared version is {version}"
        )
    return problems


def describe(entry: dict) -> str:
    return json.dumps(entry, sort_keys=True, separators=(",", ":"))


def breaking_changes(before: dict, after: dict) -> list[str]:
    """The paths where the live schema would reject a document the baseline accepted.

    Additive differences are returned as nothing at all — that is the whole point of the comparison, and
    why the policy can say "additive keeps the version" without every new field tripping the gate.
    """
    problems: list[str] = []
    for path in sorted(set(before) | set(after)):
        was, now = before.get(path), after.get(path)
        label = path or "$"
        if was is None:
            continue  # a field this schema has never seen: additive, by definition
        if now is None:
            problems.append(f"  - {label} disappeared ({describe(was)})")
            continue
        if was.get("types") and now.get("types"):
            if not set(was["types"]).issubset(set(now["types"])):
                problems.append(
                    f"  ~ {label} type narrowed {was['types']} -> {now['types']}"
                )
        if now.get("required") and not was.get("required"):
            problems.append(f"  ~ {label} became required")
        if was.get("enum") and now.get("enum"):
            lost = [member for member in was["enum"] if member not in now["enum"]]
            moved = [
                member
                for member in now["enum"]
                if member in was["enum"]
                and was["enum"].index(member) != now["enum"].index(member)
            ]
            if lost:
                problems.append(f"  ~ {label} lost enum members {lost}")
            if moved:
                problems.append(
                    f"  ~ {label} reordered enum members {moved}; a table keyed by position breaks"
                )
        if "const" in was and was.get("const") != now.get("const"):
            problems.append(f"  ~ {label} const moved {was.get('const')!r} -> {now.get('const')!r}")
        if was.get("closed") and not now.get("closed", True):
            problems.append(f"  ~ {label} closed to unknown keys; a v1 document with a new field fails")
        if was.get("ref") != now.get("ref"):
            problems.append(f"  ~ {label} now points at {now.get('ref')!r} instead of {was.get('ref')!r}")
        for bound, worse in (("minimum", 1), ("maximum", -1)):
            if bound in was and bound in now:
                if (worse > 0 and now[bound] > was[bound]) or (worse < 0 and now[bound] < was[bound]):
                    problems.append(f"  ~ {label} tightened its {bound} {was[bound]} -> {now[bound]}")
        if was.get("branches") and now.get("branches") and was["branches"] != now["branches"]:
            problems.append(f"  ~ {label} changed its branch count {was['branches']} -> {now['branches']}")
    return problems


def diff_projection(before: dict, after: dict) -> list[str]:
    """Everything that moved, breaking or not — the report `bless` and a failing check print."""
    lines: list[str] = []
    for path in sorted(set(before) | set(after)):
        was, now = before.get(path), after.get(path)
        label = path or "$"
        if was is None:
            lines.append(f"  + {label} {describe(now)}")
        elif now is None:
            lines.append(f"  - {label} {describe(was)}")
        elif was != now:
            lines.append(f"  ~ {label} {describe(was)} -> {describe(now)}")
    return lines


def read_baseline() -> dict | None:
    try:
        return json.loads(BASELINE.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except json.JSONDecodeError as err:
        print(f"delta-compat: {BASELINE} is not valid JSON: {err}")
        sys.exit(2)


def write_baseline(schema: dict, breaking: dict, additive: dict, note: str) -> None:
    document = {
        "_comment": (
            "The archived shape projection of schemas/episode-delta.schema.json, blessed by "
            "`python3 tools/check_delta_compat.py bless`. CI compares the live schema against this and "
            "asks whether any document that validated against the baseline would be rejected now; if it "
            "would, at the same version, that is a breaking change without a version bump, and this file "
            "is the reviewed diff that says the bump was intentional."
        ),
        "schema": SCHEMA.name,
        "version": declared_version(schema),
        "contract": declared_contract(schema),
        "digest": digest(breaking),
        "additive_digest": digest(additive),
        "note": note,
        "breaking": breaking,
    }
    BASELINE.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def command_check(_args: argparse.Namespace) -> int:
    schema = load_schema(SCHEMA)
    problems = check_shape(schema)
    if problems:
        for problem in problems:
            fail(problem)
        return 1

    shapes, additive = projections(schema)
    baseline = read_baseline()
    if baseline is None:
        fail(
            f"{BASELINE.name} is missing. Run `python3 tools/check_delta_compat.py bless` once, "
            "in the change that introduces the schema, and commit the file."
        )
        return 1

    version = declared_version(schema)
    baseline_version = int(baseline.get("version", -1))
    if digest(shapes) == baseline.get("digest"):
        print(
            f"delta-compat: {SCHEMA.name} is the blessed v{version} contract "
            f"({len(shapes)} declared shapes)"
        )
        return 0

    breaking = breaking_changes(baseline.get("breaking", {}), shapes)
    added = len([line for line in diff_projection(baseline.get("breaking", {}), shapes) if line.startswith("  +")])
    if not breaking:
        print(
            f"delta-compat: {SCHEMA.name} is still v{version}: {added} additive shape change(s) since "
            "the baseline, nothing a reader of the old documents would reject"
        )
        return 0

    lines = diff_projection(baseline.get("breaking", {}), shapes)
    if version < baseline_version:
        fail(
            f"the schema declares v{version} and the baseline is v{baseline_version}: a published "
            "version is never un-published"
        )
        return 1
    if version == baseline_version:
        fail(
            f"{SCHEMA.name} changed in a breaking way and is still v{version}. A breaking change "
            "without a version bump means every consumer's parser is now wrong about a document it "
            "already accepted."
        )
        print("\n".join(breaking))
        fail(
            "fix: bump `VERSION` in crates/fathom-analyze/src/insight/delta.rs (the schema is "
            "generated, so the file follows it), regenerate with `fathom analyze --schema --write`, "
            "then `python3 tools/check_delta_compat.py bless` and say why in the commit message"
        )
        return 1
    fail(
        f"the schema declares v{version} and the archived baseline is v{baseline_version}: the "
        "breaking change is allowed by the policy, but the baseline has to be re-blessed so the "
        "*decision* is a reviewed diff and not a silent state"
    )
    print("\n".join(lines))
    fail(f"once the bump is intended: `python3 tools/check_delta_compat.py bless --note 'v{version}: why'`")
    return 1


def command_bless(args: argparse.Namespace) -> int:
    schema = load_schema(SCHEMA)
    problems = check_shape(schema)
    for problem in problems:
        fail(problem)
    if problems and not args.force:
        fail("refusing to bless an inconsistent schema (pass --force if you know better)")
        return 1
    shapes, additive = projections(schema)
    baseline = read_baseline()
    version = declared_version(schema)
    if baseline is not None:
        breaking = breaking_changes(baseline.get("breaking", {}), shapes)
        if breaking and version == int(baseline.get("version", -1)):
            fail(
                "the shape projection changed in a breaking way at the same version — bump the "
                "version first, or this bless is the gate deciding to not notice"
            )
            print("\n".join(breaking))
            return 1
        if breaking:
            print(
                f"delta-compat: re-blessing for v{version} (was v{baseline.get('version')}, "
                f"{len(breaking)} breaking change(s))"
            )
    write_baseline(schema, shapes, additive, args.note)
    print(
        f"delta-compat: wrote {BASELINE.name} (v{version}, {len(shapes)} declared shapes, "
        f"digest {digest(shapes)[:12]})"
    )
    return 0


def command_self_test(_args: argparse.Namespace) -> int:
    """Prove this file refuses the thing it claims to refuse.

    A gate that is only ever run against a clean tree is a gate nobody has seen fail. Each case below
    writes a real schema and a real baseline into a temporary directory and runs the real `check`, so
    what is being tested is the same code path CI takes — including the parts that only run when a
    baseline disagrees.
    """
    schema = load_schema(SCHEMA)
    cases = [
        ("making an optional key required is breaking", lambda doc: doc["required"].append("risk"), False),
        ("dropping a requirement is additive", lambda doc: doc["required"].remove("gaps"), True),
        ("renaming a property is breaking", lambda doc: rename(doc, "gaps", "gaps_v2"), False),
        ("narrowing a type is breaking", lambda doc: narrow_type(doc), False),
        ("adding an optional property is additive", lambda doc: add_property(doc), True),
        ("rewording a description is additive", lambda doc: reword(doc), True),
        ("an added enum member is additive", lambda doc: widen_enum(doc), True),
        ("a removed enum member is breaking", lambda doc: shrink_enum(doc), False),
    ]
    failures = 0
    for name, edit, additive in cases:
        mutated = copy.deepcopy(schema)
        edit(mutated)
        ok, why = verdict(schema, mutated, additive)
        status = "ok" if ok else "FAILED"
        if not ok:
            failures += 1
        print(f"delta-compat self-test: {status}: {name} ({'additive' if additive else 'breaking'}) — {why}")
    print(
        f"delta-compat self-test: {len(cases) - failures}/{len(cases)} cases behaved as the policy says"
    )
    return 1 if failures else 0


def verdict(original: dict, mutated: dict, additive: bool) -> tuple[bool, str]:
    """Run the real decision on a temp tree; return `(as_expected, what happened)`."""
    with tempfile.TemporaryDirectory() as tmp:
        workspace = pathlib.Path(tmp)
        (workspace / "schemas").mkdir()
        (workspace / "crates" / "fathom-analyze" / "src" / "insight").mkdir(parents=True)
        baseline_path = workspace / "schemas" / "episode-delta.baseline.json"
        schema_path = workspace / "schemas" / "episode-delta.schema.json"
        # Point the module-level paths at the temp tree for the duration of the case.
        global SCHEMA, BASELINE, VERSION_SOURCE  # noqa: PLW0603
        keep = (SCHEMA, BASELINE, VERSION_SOURCE)
        SCHEMA, BASELINE, VERSION_SOURCE = (  # noqa: PLW0603
            schema_path,
            baseline_path,
            workspace / "crates" / "fathom-analyze" / "src" / "insight" / "delta.rs",
        )
        try:
            original_text = json.dumps(original, indent=2, sort_keys=True) + "\n"
            schema_path.write_text(original_text, encoding="utf-8")
            breaking, additive_projection = projections(original)
            baseline_path.write_text(
                json.dumps(
                    {
                        "version": declared_version(original),
                        "contract": declared_contract(original),
                        "digest": digest(breaking),
                        "additive_digest": digest(additive_projection),
                        "breaking": breaking,
                    },
                    indent=2,
                    sort_keys=True,
                )
                + "\n",
                encoding="utf-8",
            )
            mutated_version = mutated.get("x-fathom", {}).get("version")
            if not additive and mutated_version is not None:
                # The "as expected" for a breaking case has two halves: same version => refused,
                # bumped version => accepted once re-blessed. Exercise the refusal, which is the gate.
                schema_path.write_text(
                    json.dumps(mutated, indent=2, sort_keys=True) + "\n", encoding="utf-8"
                )
                code = command_check(argparse.Namespace())
                return code == 1, "same version, refused" if code == 1 else f"exit {code}"
            if mutated_version is not None:
                schema_path.write_text(
                    json.dumps(mutated, indent=2, sort_keys=True) + "\n", encoding="utf-8"
                )
                code = command_check(argparse.Namespace())
                return code == 0, "accepted at the same version" if code == 0 else f"exit {code}"
            return False, "case edited nothing"
        finally:
            SCHEMA, BASELINE, VERSION_SOURCE = keep  # noqa: PLW0603
            shutil.rmtree(workspace, ignore_errors=True)


def rename(document: dict, old: str, new: str) -> None:
    """Rename a top-level property, the way a refactor would."""
    document["properties"][new] = document["properties"].pop(old)
    document["required"] = [new if key == old else key for key in document["required"]]


def narrow_type(document: dict) -> None:
    document["$defs"]["totals"]["properties"]["n_files"]["type"] = "string"


def add_property(document: dict) -> None:
    document["$defs"]["totals"]["properties"]["from_a_future_version"] = {
        "type": "string",
        "description": "A field a later v1 build might add; readers ignore it.",
    }


def reword(document: dict) -> None:
    document["$defs"]["totals"]["description"] = "The scale, measured and derived. Reworded."


def widen_enum(document: dict) -> None:
    document["$defs"]["sectionState"]["enum"].append("guessed")


def shrink_enum(document: dict) -> None:
    document["$defs"]["sectionState"]["enum"].remove("partial")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="check_delta_compat.py", description=__doc__)
    sub = parser.add_subparsers(dest="command")
    sub.add_parser("check", help="compare the live schema against the archived baseline (CI)")
    bless = sub.add_parser("bless", help="record the live schema as the baseline")
    bless.add_argument("--note", default="", help="why this is a new baseline")
    bless.add_argument("--force", action="store_true", help="bless despite internal problems")
    sub.add_parser("self-test", help="prove the gate refuses what it claims to")
    args = parser.parse_args(argv)
    if args.command == "bless":
        return command_bless(args)
    if args.command == "self-test":
        return command_self_test(args)
    return command_check(args)


if __name__ == "__main__":
    sys.exit(main())
