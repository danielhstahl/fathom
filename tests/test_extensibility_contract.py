"""`reco-raw`'s acceptance criteria, as tests (`e6.7`: the detector API, an out-of-tree plugin, the events).

This file is the executable form of
[docs/concepts/extensibility.md](../docs/concepts/extensibility.md). It checks the four things a Rust test
structurally cannot, because they are claims about *artefacts agreeing* rather than about behaviour:

1. **"The example plugin builds and runs out of tree against the published crate in CI."** So the plugin must
   be excluded from the root workspace (otherwise "out of tree" is a comment), must depend on
   `fathom-analyze` as a dependency rather than a sibling, and must be built and *run* by a CI step
   (`test_the_example_plugin_is_a_separate_workspace`,
   `test_ci_builds_and_runs_the_example_plugin`).
2. **"The finding schema is versioned, and unknown finding types are ignored rather than fatal."** So the
   published JSON Schema must carry the version and the namespaced-type grammar, and the rules it summarises
   — R1–R8 — must exist in the code that enforces them and in the page that documents them, with the same
   labels (`test_the_plugin_finding_schema_is_versioned_and_namespaced`,
   `test_every_numbered_rule_is_enforced_and_documented`).
3. **"Disabling a built-in detector is supported and documented."** So the knob is declared in `e0.8`'s
   config schema, the flags exist in clap's grammar, and the documentation says both what `disable` does and
   how it differs from `suppress` (`test_the_new_knobs_are_declared_in_the_config_schema`,
   `test_the_flags_the_docs_promise_exist_in_the_grammar`,
   `test_disable_and_suppress_are_documented_as_two_different_switches`).
4. **"A stability policy for the plugin API is written down in the docs."** So there is a section by that
   name, with numbered rules, that states the additive/breaking split and where the version lives
   (`test_the_stability_policy_is_written_down`).

Plus the event bus, which is half contract: every kind `fathom-core` declares is documented, every kind the
page names is declared, and the two numbers the page quotes are the code's
(`test_every_declared_event_kind_is_documented`, `test_the_event_bus_numbers_are_the_codes`).

Stdlib and pytest only. Nothing here re-implements a detector, an event or a filter: the behavioural half of
every one of those claims lives in `crates/fathom-core/tests/events.rs`,
`crates/fathom-analyze/tests/plugin_api.rs` and `crates/fathom-cli/tests/extensibility_verb.rs`, and this
file reads the product's own artefacts — its schema, its grammar, its CI and its documentation.

Run: python3 -m pytest tests/test_extensibility_contract.py -q
"""

from __future__ import annotations

import json
import pathlib
import re

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]

CORE_EVENTS = ROOT / "crates/fathom-core/src/events.rs"
KINDS = ROOT / "crates/fathom-core/src/events/kind.rs"
PLUGIN_MOD = ROOT / "crates/fathom-analyze/src/plugin/mod.rs"
PLUGIN_FINDING = ROOT / "crates/fathom-analyze/src/plugin/finding.rs"
PLUGIN_SCHEMA = ROOT / "crates/fathom-analyze/src/plugin/schema.rs"
PLUGIN_API_TEST = ROOT / "crates/fathom-analyze/tests/plugin_api.rs"
CLI_VERBS = ROOT / "crates/fathom-cli/src/verbs.rs"
CLI_EVENTS = ROOT / "crates/fathom-cli/src/events.rs"
PLUGIN_FINDING_SCHEMA = ROOT / "schemas/plugin-finding.schema.json"
EVENT_SCHEMA = ROOT / "schemas/fathom-analysis-events.schema.json"
DOC = ROOT / "docs/concepts/extensibility.md"
PLUGIN_CRATE = ROOT / "plugins/acme-detectors"
PLUGIN_MANIFEST = PLUGIN_CRATE / "Cargo.toml"
PLUGIN_README = ROOT / "plugins/README.md"
CI = ROOT / ".github/workflows/ci.yml"


def _text(path: pathlib.Path) -> str:
    assert path.exists(), f"{path.relative_to(ROOT)} is missing"
    return path.read_text(encoding="utf-8")


# --------------------------------------------------------------------------- event kinds


def _declared_kinds() -> list[str]:
    """The `pub const X: &str = "family.fact"` list, which is its own file now."""
    body = _text(KINDS).split("pub const ALL", 1)[0]
    return re.findall(r'pub const [A-Z_0-9]+: &str = "([a-z_]+\.[a-z_]+)";', body)


def test_every_declared_event_kind_is_documented() -> None:
    kinds = _declared_kinds()
    assert len(kinds) >= 12, kinds
    page = _text(DOC)
    missing = [kind for kind in kinds if f"`{kind}`" not in page]
    assert not missing, f"event kinds the page does not name: {missing}"


def test_the_page_names_no_kind_the_code_does_not_declare() -> None:
    # The direction that rots quietly: a kind added to a documentation table by hand, or renamed there and
    # not here. `analysis.begin` in a page and `analysis.started` in the code is a page that teaches a filter
    # that matches nothing.
    kinds = set(_declared_kinds())
    page = _text(DOC)
    table = page.split("The declared kinds", 1)[1].split("Three properties", 1)[0]
    # Only the first column of the table, which is the column of kinds; the prose beside them names
    # `counts.findings` and friends, and those are field paths rather than kinds.
    named = set()
    for row in table.splitlines():
        cells = [cell.strip() for cell in row.split("|")]
        if len(cells) > 2 and cells[1].startswith("`") and cells[1].endswith("`"):
            named.add(cells[1].strip("`"))
    invented = sorted(named - kinds)
    assert not invented, f"kinds the page names and `kind::ALL` does not: {invented}"


def declared_kinds() -> set[str]:
    """Every kind name `fathom-core` declares, read from the module that declares them."""
    return set(
        re.findall(r'pub const [A-Z_0-9]+: &str = "([a-z_]+\.[a-z_]+)";', _text(KINDS))
    )


def test_the_declared_kinds_are_the_all_list() -> None:
    # The two lists inside the module have to agree, or `Kind::is_declared` and the documentation page are
    # describing different sets and one of them is a lie.
    body = _text(KINDS)
    all_list = re.findall(r"^\s{4}([A-Z_0-9]+),$", body.split("pub const ALL", 1)[1], re.M)
    consts = dict(re.findall(r'pub const ([A-Z_0-9]+): &str = "([a-z_]+\.[a-z_]+)";', body))
    assert all_list, "kind::ALL is empty or its shape changed"
    assert all(name in consts for name in all_list), all_list
    assert len(all_list) == len(consts), "a declared kind is missing from kind::ALL"


def test_the_published_event_schema_names_every_declared_kind() -> None:
    """The published event schema, the Rust list and the page are one list (ADR-002 §4.3).

    `crates/fathom-core/tests/events.rs` diffs the file against the generator that writes it; this checks the
    generator's own vocabulary against the `kind::ALL` a plugin author reads, so documentation, schema and
    code cannot drift in a circle and agree with each other while disagreeing with the crate.
    """
    schema = json.loads(_text(EVENT_SCHEMA))
    assert set(schema["properties"]["kind"]["enum"]) == declared_kinds(), (
        "the published schema and `kind::ALL` are not the same list"
    )
    assert schema["required"] == ["contract", "schema_version", "kind", "seq", "run"], (
        "the five keys every line carries are the contract; widening them invalidates every stream written "
        "before the change"
    )
    assert schema["x-fathom"]["reader_ignores_unknown_fields"] is True, (
        "additive has to stay additive, and the document has to say so"
    )
    assert "line" in schema["description"], "the schema describes one line, and says which thing it describes"


def test_the_event_bus_numbers_are_the_codes() -> None:
    text = _text(CORE_EVENTS)
    cap = int(re.search(r"pub const MAX_SUBSCRIBERS: usize = (\d+);", text).group(1))
    page = _text(DOC)
    assert f"`MAX_SUBSCRIBERS`, {cap}" in page or f"({cap})" in page, (
        "the page quotes a subscriber cap; it must be the code's"
    )
    version = int(re.search(r"pub const SCHEMA_VERSION: u32 = (\d+);", text).group(1))
    assert f"`schema_version` (currently `{version}`)" in page, (
        f"the page states the event schema version; it must be {version}"
    )


# --------------------------------------------------------------------------- finding schema


def test_the_plugin_finding_schema_is_versioned_and_namespaced() -> None:
    schema = json.loads(_text(PLUGIN_FINDING_SCHEMA))
    assert schema["additionalProperties"] is True, "additive has to stay additive"
    properties = schema["properties"]
    for field in ("schema_version", "finding_type", "subject", "message", "severity", "suppression_key"):
        assert field in properties, field
    for field in ("schema_version", "finding_type", "plugin", "episode_id", "subject", "message"):
        assert field in schema["required"], field
    # The version is a closed set in the document — one value, this one — and it is the value the Rust type
    # writes, so a schema and a struct cannot disagree about what a finding is.
    shape = properties["schema_version"]
    version = shape.get("const", (shape.get("enum") or [None])[0])
    assert version == 1, shape
    rust_version = re.search(
        r"pub const SCHEMA_VERSION: u32 = (\d+);", _text(PLUGIN_FINDING)
    )
    assert rust_version and int(rust_version.group(1)) == version
    # A finding type is namespaced: the pattern requires the dot, and a bare dotless id — a built-in's — is
    # not a match. That is the sentence "no plugin may take a built-in's name", in the schema rather than
    # only in the code that enforces it.
    pattern = properties["finding_type"]["pattern"]
    assert r"\." in pattern, pattern
    assert re.match(pattern.replace("^", "").replace("$", ""), "acme.orphan_test")
    assert not re.match(pattern.replace("^", "").replace("$", ""), "secret_string")
    assert properties["suppression_key"]["minLength"] == 1


def test_the_schema_is_generated_and_not_hand_written() -> None:
    # Every published schema here is the output of a generator in the crate that owns the model, so a file
    # edited to make a test pass is impossible rather than discouraged.
    generator = _text(PLUGIN_SCHEMA)
    assert "pub fn document" in generator and "SCHEMA_FILE" in generator
    checked_in = _text(PLUGIN_FINDING_SCHEMA)
    assert '"x-fathom"' in checked_in, "the schema names its contract and version in the file"
    blessed = _text(PLUGIN_API_TEST)
    assert "FATHOM_BLESS" in blessed, "the schema is blessed, the way every schema here is"
    assert "schema::to_text" in blessed, "the checked-in file is diffed against the generator"


def test_every_numbered_rule_is_enforced_and_documented() -> None:
    # R1–R8 are the containment story: a plugin's bad finding costs the reader that finding and nothing else.
    # The labels are the contract between the code, the page and the test names, so all three must carry them.
    code = _text(PLUGIN_FINDING) + _text(PLUGIN_MOD)
    page = _text(DOC)
    tests = _text(PLUGIN_API_TEST)
    for rule in ("R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8"):
        assert rule in code, f"{rule} is documented but not named in the enforcing code"
        assert f"**{rule}**" in page, f"{rule} is not in the documentation's rule table"
        assert rule in tests, f"{rule} has no test file naming it (the map at the top of plugin_api.rs)"
    # And every rule in the map names a test that exists, or the map is decoration.
    names = set(re.findall(r"^fn ([a-z0-9_]+)\(", tests, re.M))
    for row in re.findall(r"\| (R\d) — .*`([a-z0-9_]+)` \|", tests):
        rule, test = row
        assert test in names, f"{rule} points at `{test}`, which this file does not define"


def test_unknown_and_undeclared_findings_are_ignored_rather_than_fatal() -> None:
    # The acceptance criterion, in its behavioural half: the test names state it, and the notices state the
    # reason, so a reader of the report learns what was dropped instead of finding an unexplained gap.
    tests = _text(PLUGIN_API_TEST)
    assert "fn a_finding_from_a_schema_this_build_cannot_read_is_ignored_by_name(" in tests, (
        "no test that a newer finding version is refused by name"
    )
    assert "fn an_unknown_finding_type_is_ignored_and_the_run_carries_on(" in tests, (
        "no test that an undeclared finding type is ignored"
    )
    code = _text(PLUGIN_FINDING)
    assert "schema_version" in code and "declare" in code.lower()


# --------------------------------------------------------------------------- disable / enable


def test_the_new_knobs_are_declared_in_the_config_schema() -> None:
    import sys

    sys.path.insert(0, str(ROOT / "src"))
    from recommender.config import schema as schema_module

    keys = schema_module.KEYS
    for key in ("detectors.disable", "events.enabled", "events.on", "paths.analysis_events"):
        assert key in keys, key

    assert keys["detectors.disable"].merge == "extend", "a repo adds to the list, not replaces"
    assert keys["events.enabled"].default is False, (
        "the product writes no file a repository did not ask for (ADR-001 §4.6)"
    )
    assert keys["detectors.disable"].ticket == "e6.7"
    assert "events" in schema_module.SECTIONS


def test_the_generated_schema_carries_the_knobs_too() -> None:
    generated = json.loads((ROOT / "schemas/fathom.schema.json").read_text(encoding="utf-8"))
    properties = generated["properties"]
    assert "events" in properties["detectors"]["properties"] | properties, None
    assert "disable" in properties["detectors"]["properties"], "detectors.disable is not published"
    assert "analysis_events" in properties["paths"]["properties"]
    assert "enabled" in properties["events"]["properties"]


def test_the_flags_the_docs_promise_exist_in_the_grammar() -> None:
    grammar = _text(CLI_VERBS)
    for flag in (
        'long = "detectors"',
        'long = "enable-detector"',
        'long = "disable-detector"',
        'long = "events"',
        "Events(EventsArgs)",
    ):
        assert flag in grammar, flag


def test_disable_and_suppress_are_documented_as_two_different_switches() -> None:
    # The distinction is the deliverable: "did it look?" has to stay answerable after a switch is thrown.
    page = _text(DOC)
    assert "detectors.disable" in page and "detectors.suppress" in page
    assert "`disabled`" in page, "the report's row state for a switched-off detector is documented"
    assert "applied after" in page, "the precedence between enabled and disable is stated"
    policy = _text(ROOT / "crates/fathom-analyze/src/detectors/policy.rs")
    assert "pub fn disabled" in policy, "the policy cannot tell 'off on purpose' from 'never on'"


# --------------------------------------------------------------------------- the out-of-tree example


def test_the_example_plugin_is_a_separate_workspace() -> None:
    root_manifest = _text(ROOT / "Cargo.toml")
    assert "plugins" in root_manifest, "plugins/* must be excluded from the root workspace"
    members = re.search(r"members = \[(.*?)\]", root_manifest, re.S).group(1)
    assert "plugins" not in members
    exclude = re.search(r"exclude = \[(.*?)\]", root_manifest, re.S)
    assert exclude and "plugins" in exclude.group(1), (
        "`plugins/acme-detectors` is its own workspace and must be excluded, or cargo will walk it from the "
        "root and the out-of-tree claim is a comment"
    )
    manifest = _text(PLUGIN_MANIFEST)
    assert "[workspace]" in manifest, "the example is its own workspace root"
    assert 'path = "../../crates/fathom-analyze"' in manifest
    assert 'version = "' in manifest.split("fathom-analyze =")[1].split("\n")[0], (
        "the dependency states the version it is published at, so switching from `path` to a registry "
        "requirement is a deletion rather than a guess"
    )
    assert "fathom-store" not in manifest and "fathom-ingest" not in manifest, (
        "a plugin that needs the store is a plugin that needs a fact added to the delta model"
    )


def test_ci_builds_and_runs_the_example_plugin() -> None:
    ci = _text(CI)
    assert "plugins/acme-detectors" in ci, "the example plugin is not built in CI"
    assert "out-of-tree-plugin:" in ci, "ci.yml has no `out-of-tree-plugin` job"
    body = re.split(r"\n  [a-z][a-z-]*:\n", ci.split("out-of-tree-plugin:", 1)[1], maxsplit=1)[0]
    assert "cargo test" in body and "cargo run" in body, (
        "the criterion is that it builds *and runs*: a build-only job proves the types, not the pipeline"
    )


def test_the_example_is_introduced_where_somebody_would_look() -> None:
    assert "`acme.untested_agent_logic`" in _text(PLUGIN_README) or "acme.untested_agent_logic" in _text(
        PLUGIN_README
    )
    page = _text(DOC)
    assert "plugins/acme-detectors" in page
    index = _text(ROOT / "docs/index.md")
    assert "concepts/extensibility.md" in index, "the page is unreachable from the front door"


# --------------------------------------------------------------------------- stability policy


def test_the_stability_policy_is_written_down() -> None:
    page = _text(DOC)
    section = page.split("## Stability policy", 1)
    assert len(section) == 2, "the docs have no stability policy section"
    body = section[1].split("\n## ", 1)[0]
    rules = re.findall(r"^\d+\. \*\*", body, re.M)
    assert len(rules) >= 4, f"the policy has {len(rules)} numbered rules"
    for promise in ("Additive", "Breaking", "deprecated", "schema_version"):
        assert promise in body, promise
    # The API states the major it was written against, which is how a host reports a stale plugin.
    assert "api_version" in _text(PLUGIN_MOD)


def test_the_surface_the_policy_covers_is_public() -> None:
    lib = _text(ROOT / "crates/fathom-analyze/src/lib.rs")
    assert "pub mod plugin;" in lib
    events = _text(ROOT / "crates/fathom-core/src/lib.rs")
    assert "pub mod events;" in events


@pytest.mark.parametrize(
    "artifact",
    [KINDS, CORE_EVENTS, PLUGIN_MOD, PLUGIN_FINDING, PLUGIN_SCHEMA, CLI_EVENTS, DOC, PLUGIN_README],
)
def test_the_artefacts_are_there_to_be_read(artifact: pathlib.Path) -> None:
    assert artifact.exists() and artifact.stat().st_size > 500, artifact
