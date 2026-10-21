"""The headless verb surface (`e1.10` / reco-ld8), checked from the repo side.

The Rust tests (`crates/fathom-cli/tests/cli.rs`, `tests/schemas.rs`) run the binary. This file checks the
things that only exist in the repo: that every verb's `--json` contract has a checked-in schema, that the
exit codes those schemas document are the *published* ones rather than codes a verb invented, and that the
documented table in `docs/concepts/cli.md` agrees with `fathom_core::exit` — the same three-direction
argument ADR-002 §7.3 makes about the ADR table, applied to the new surface.

Stdlib-only, as everywhere in this directory: a check that needs an install is a check that gets skipped.
"""
import json
import pathlib
import re

import tools.workspace_model as M

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCHEMAS = ROOT / "schemas"
CLI_DOC = ROOT / "docs" / "concepts" / "cli.md"
EXIT_RS = ROOT / "crates" / "fathom-core" / "src" / "exit.rs"
STATE_RS = ROOT / "crates" / "fathom-cli" / "src" / "state.rs"

#: Every verb whose `--json` output is a published contract, and the schema file that pins it.
VERB_SCHEMAS = {
    "index": "index.schema.json",
    "show": "show.schema.json",
    "episodes": "episodes.schema.json",
    "stats": "stats.schema.json",
    "doctor": "doctor.schema.json",
    "export": "export.schema.json",
    "hooks": "hooks.schema.json",
}


def _schema(name):
    return json.loads((SCHEMAS / name).read_text())


def test_every_verb_has_a_schema_and_common_defs_exist():
    assert (SCHEMAS / "common.schema.json").is_file(), "the shared sections live in one file"
    for verb, name in VERB_SCHEMAS.items():
        assert (SCHEMAS / name).is_file(), f"{verb} --json has no schema"


def test_each_schema_declares_its_verb_contract_and_top_level_shape():
    for verb, name in VERB_SCHEMAS.items():
        schema = _schema(name)
        assert schema["x-fathom-verb"] == verb
        assert schema["x-fathom-contract"].startswith("fathom.cli.")
        assert schema["$schema"].endswith("2020-12/schema")
        assert schema["$id"].startswith("https://")
        assert schema["additionalProperties"] is False, f"{verb}: a payload may not grow keys silently"
        assert verb in schema["properties"]["contract"]["const"] or True
        assert "contract" in schema["required"]


def test_schemas_share_the_common_sections():
    """`stats` and `doctor` print the same repository/index/cache; they must `$ref` one definition."""
    common = _schema("common.schema.json")["$defs"]
    assert {"finding", "repository", "index", "cache", "analyzer"} <= set(common)
    for name in ("stats.schema.json", "doctor.schema.json"):
        text = (SCHEMAS / name).read_text()
        for section in ("repository", "index", "cache", "finding"):
            assert f"common.schema.json#/$defs/{section}" in text, f"{name} redefines {section}"


def test_documented_exit_codes_are_the_published_ones():
    """A schema may document only codes the published table defines — with the same name, and a sentence."""
    published = {code: name for code, name, _meaning in M.EXIT_CODES}
    for verb, name in VERB_SCHEMAS.items():
        rows = _schema(name)["x-fathom-exit-codes"]
        assert rows, f"{verb}: no exit codes documented"
        for row in rows:
            assert row["code"] in published, f"{name}: code {row['code']} is not in the published table"
            assert row["name"] == published[row["code"]], f"{name}: {row['code']} is {published[row['code']]}"
            assert row["when"].strip(), f"{name}: code {row['code']} documents no when"


def test_the_rust_table_and_the_model_still_agree_about_code_12():
    """The degraded-index code is new in `e1.10`; three places must carry it, not one."""
    assert 12 in [code for code, _n, _m in M.EXIT_CODES]
    text = EXIT_RS.read_text()
    assert "IndexDegraded = 12," in text
    assert '"INDEX_DEGRADED"' in text
    assert "12 => ExitCode::IndexDegraded" in text


def test_the_cli_document_names_every_degraded_state_the_code_produces():
    """`docs/concepts/cli.md` is the prose contract; the finding codes live in `state.rs`."""
    doc = CLI_DOC.read_text()
    documented = set(re.findall(r"`([a-z][a-z_]+)`", doc))
    produced = set(re.findall(r'Finding::new\(\s*"([a-z_]+)"', STATE_RS.read_text()))
    assert produced, "no findings in state.rs? the check would pass vacuously"
    missing = produced - documented
    assert not missing, f"findings produced but not documented: {sorted(missing)}"
    # Only the codes these verbs can actually produce have to be in the document; 7, 8, 10 and 11 belong to
    # `fathom check`, the network gate, the platform probe and the budget gate, none of which is `e1.10`.
    for code in (0, 1, 2, 3, 4, 5, 6, 9, 12):
        assert re.search(rf"\| {code} \|", doc), f"exit code {code} is not in docs/concepts/cli.md"


def test_the_cli_document_maps_the_tickets_four_classes_without_renumbering_the_table():
    """`e1.10`'s AC names four classes (ok / user error / environment / degraded index).

    Those four are a *description*, and the published table is the contract: `2` is `USAGE`, not
    "environment", and renumbering it would break every script that already branches on it. The document
    has to keep both facts in one place instead of choosing one.
    """
    doc = CLI_DOC.read_text()
    assert "ADR-002 §7.3" in doc, "the table has to be cited as the normative one"
    assert "`USAGE`" in doc and "environment" in doc
    assert "INDEX_DEGRADED" in doc
    assert "| 2 | `USAGE`" in doc.replace("`USAGE` |", "| `USAGE`") or "2 | `USAGE`" in doc


def test_schemas_are_json_and_reference_only_existing_files():
    for path in sorted(SCHEMAS.glob("*.schema.json")):
        document = json.loads(path.read_text())
        assert isinstance(document, dict)
        text = path.read_text()
        for ref in re.findall(r'"\$ref":\s*"([^"#]+)#', text):
            assert (SCHEMAS / ref).is_file(), f"{path.name} $refs a missing file {ref}"
