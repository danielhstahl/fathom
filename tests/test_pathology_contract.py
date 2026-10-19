"""`reco-2ox`'s acceptance criteria, as tests (`e4.3`: agent pathology detectors).

This file is the executable form of
[docs/concepts/pathology-detectors.md](../docs/concepts/pathology-detectors.md), and it checks the
things a Rust test cannot: that the four artefacts of this ticket — the Rust `CATALOGUE`, the blessed
fixture corpus, the documentation and `e0.8`'s config schema — are one statement and not four that
happen to agree today.

The ticket's criteria, in order:

1. **one fixture per detector, and each detector fires on its fixture and stays silent on the clean
   corpus** (`test_every_detector_has_a_fixture`, `test_the_clean_corpus_is_blessed_empty`);
2. **every finding has a human-readable reason, a jump target, a confidence and a one-key suppression**
   (`test_every_finding_carries_the_four_parts`, plus the checks on message text and suppression-key
   stability);
3. **the false-positive gate**: a detector whose own false positives could breach `G3 <= 0.30` ships
   disabled by default, and names the measurement that re-enables it
   (`test_disabled_detectors_name_their_measurement`) — the measurement itself is `e4.3a`
   (`reco-mkl.1`), which is why the four are off and not missing;
4. **the catalogue is documented with a worked example and a template**
   (`test_the_documented_table_is_the_catalogue`, `test_the_worked_example_quotes_real_output`,
   `test_the_template_names_every_step`).

Then the cross-contract work, in the shape the earlier tickets set: the severity and evidence vocabularies
are ADR-004's (`test_the_vocabularies_are_adr_004_s`), the jump views are `fathom-tui`'s registry and not a
second list (`test_the_jump_views_are_the_tuis`), and every knob the detectors read is declared in
`e0.8`'s schema (`test_every_knob_is_declared_in_the_config_schema`).

Why Python can check any of this: the expected findings are *blessed output of the Rust scan* —
`FATHOM_BLESS=1 cargo test -p fathom-analyze --test detector_fixtures` writes them — so this file reads
the product's own words rather than a paraphrase of them. Stdlib and pytest only; no fixture here is a
cloned repository, and nothing here re-implements a detector.

Run: python3 -m pytest tests/test_pathology_contract.py -q
"""

from __future__ import annotations

import json
import pathlib
import re

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates" / "fathom-analyze" / "tests" / "fixtures" / "detectors"
DOC = ROOT / "docs" / "concepts" / "pathology-detectors.md"
SOURCE = ROOT / "crates" / "fathom-analyze" / "src" / "detectors" / "mod.rs"
ADR_STORAGE = ROOT / "docs" / "adr" / "0004-storage.md"
TUI_VIEWS = ROOT / "crates" / "fathom-tui" / "src" / "views.rs"
SCHEMA = ROOT / "src" / "recommender" / "config" / "schema.py"

# `evidence.kind` values ADR-004's `evidence` table allows today, plus the two this catalogue writes and
# names as needing the amendment. A third extra is a defect, not an idea.
ADR_EVIDENCE_KINDS = {"range", "hunk", "lockfile_line", "edge", "test", "config_key", "event"}
CATALOGUE_EVIDENCE_KINDS = {"path", "episode"}

SEVERITIES = {"high", "medium", "low"}
GRADES = {"observed", "inferred", "asserted", "absent"}


def load(path: pathlib.Path):
    return json.loads(path.read_text())


def catalogue() -> dict:
    return load(FIXTURES / "catalogue.json")


def rows() -> list[dict]:
    return catalogue()["detectors"]


def ids() -> list[str]:
    return [row["id"] for row in rows()]


def expectations(directory: pathlib.Path) -> dict[str, list[dict]]:
    return {
        path.name[: -len(".expected.json")]: load(path)
        for path in sorted(directory.glob("*.expected.json"))
    }


# --- the catalogue: one table, four places it is written -------------------


def test_the_rust_table_and_the_blessed_catalogue_agree():
    """`CATALOGUE` in Rust is the same list, in the same order, with the same default flags."""
    source = SOURCE.read_text()
    rust_ids = re.findall(r'^\s+id: "([a-z_]+)",\s*$', source, re.M)
    assert rust_ids == ids(), "the blessed catalogue.json is stale; re-bless the fixture test"
    rust_versions = [int(v) for v in re.findall(r"^\s+version: (\d+),\s*$", source, re.M)]
    assert rust_versions == [row["version"] for row in rows()]
    rust_defaults = re.findall(r"^\s+default_on: (true|false),\s*$", source, re.M)
    assert rust_defaults == [str(row["default_on"]).lower() for row in rows()], (
        "the blessed catalogue's default flags are not the Rust table's"
    )


def test_the_documented_table_is_the_catalogue():
    """The markdown table in the concept doc is the catalogue, not a summary of it."""
    text = DOC.read_text()
    documented = re.findall(r"^\| `([a-z_]+)` \| (.+?) \| (\w+) \| \*\*(on|off)\*\* \|$", text, re.M)
    assert len(documented) == len(rows()), f"the doc lists {len(documented)} of {len(rows())} detectors"
    by_id = {row["id"]: row for row in rows()}
    for doc_id, asks, needs, default in documented:
        assert doc_id in by_id, f"{doc_id} is documented but not in the catalogue"
        row = by_id[doc_id]
        assert row["default_on"] == (default == "on"), f"{doc_id}'s default disagrees with the doc"
        assert row["needs"] == needs.lower(), f"{doc_id}'s needs disagrees with the doc"
        assert asks.strip().rstrip("?") in row["asks"], f"{doc_id}'s question is paraphrased in the doc"
    # And nothing in the catalogue is missing from the doc.
    assert {row["id"] for row in rows()} == {row[0] for row in documented}


def test_the_python_view_of_the_catalogue_covers_every_detector():
    assert len(set(ids())) == 12, ids()
    for row in rows():
        assert row["title"] and row["asks"].endswith("?")
        assert row["version"] >= 1
        assert row["needs"] in {"rows", "content", "history"}


# --- the fixtures -----------------------------------------------------------


def test_every_detector_has_a_fixture():
    fixtures = {
        path.name[: -len(".json")]
        for path in (FIXTURES / "fixtures").glob("*.json")
        if not path.name.endswith(".expected.json")
    }
    assert fixtures == set(ids()), "one fixture per detector, named for it"
    assert len(fixtures) == len(ids()), "a fixture is doing double duty"


def test_every_finding_carries_the_four_parts():
    for name, findings in expectations(FIXTURES / "fixtures").items():
        assert findings, f"{name}'s fixture blessed an empty finding list — it does not fire"
        for finding in findings:
            message = finding["message"]
            assert len(message) > 40, f"{name}: a reason a person cannot read: {message!r}"
            assert message == message.strip() and message[-1] not in ",: ", (
                f"{name}: a sentence that stops mid-thought: {message!r}"
            )
            assert finding["subject"] in message or re.search(r"\d", message), (
                f"{name}: a claim that does not say what it is about: {message!r}"
            )
            assert finding["severity"] in SEVERITIES
            assert 0.0 < finding["confidence"] <= 1.0
            assert finding["jump_view"] and finding["jump_target"]
            assert finding["suppression_key"], f"{name}: a finding nobody can silence"
            assert finding["evidence"], f"{name}: a finding with no evidence"
            # The detector that raised it is in the catalogue, and the jump is a target a surface can open.
            assert finding["detector_id"] in ids()
            if finding["jump_view"] == "diff":
                episode, _, path = finding["jump_target"].partition("#")
                assert episode and path, f"{name}: a diff jump without an episode and a path"
            else:
                assert finding["jump_target"], f"{name}: a jump to nowhere"


def test_the_clean_corpus_is_blessed_empty():
    clean = expectations(FIXTURES / "clean")
    assert len(clean) >= 3, "the clean corpus is three ordinary episodes, not one"
    for name, findings in clean.items():
        assert findings == [], f"{name} is meant to be clean and this build found {len(findings)} findings"


def test_no_confidence_is_claimed_at_certainty_for_an_inferred_rule():
    """A finding whose every piece of evidence is `inferred` may not claim 1.0.

    The ceiling is a promise about the detector, and `mod.rs`'s price list says `1.0` is reserved for
    what the diff states outright. An inferred claim at 1.0 is the number a reviewer stops trusting.
    """
    for name, findings in expectations(FIXTURES / "fixtures").items():
        for finding in findings:
            grades = {evidence.split(" ")[0] for evidence in finding["evidence"]}
            if grades == {"inferred"}:
                assert finding["confidence"] <= 0.8, (
                    f"{name}: an inference at {finding['confidence']} — only an observation may reach higher"
                )


def test_a_suppression_key_survives_a_reword_and_a_rebase():
    """No episode id and no timestamp in a key, and the key is its detector plus its subject."""
    for name, findings in expectations(FIXTURES / "fixtures").items():
        for finding in findings:
            key = finding["suppression_key"]
            assert key.startswith(finding["detector_id"]), f"{name}: {key} is not derived from its detector"
            assert not re.search(r"ep-|\d{10}", key), (
                f"{name}: {key} would move on a rebase, and a suppression you have to retype after every "
                "rebase is not a suppression"
            )
            assert re.fullmatch(r"[a-z_]+(:[A-Za-z0-9._/:*-]+)?", key), f"{name}: {key} is not typable"


def test_the_worked_example_quotes_real_output():
    """The doc's example is the blessed JSON, word for word, not a paraphrase."""
    text = DOC.read_text()
    lint = expectations(FIXTURES / "fixtures")["lint_escape"][0]
    assert lint["message"] in text, "the doc's worked example no longer quotes the finding it claims to"
    assert lint["suppression_key"] in text
    assert f'"{lint["severity"]}"' in text
    rework = expectations(FIXTURES / "fixtures")["rework_loop"][0]
    assert "rework_loop" in text and rework["confidence"] >= 0.5


def test_the_template_names_every_step():
    text = DOC.read_text()
    for requirement in ("pub fn detect(", "CATALOGUE", "make_fixtures.py", "FATHOM_BLESS"):
        assert requirement in text, f"the writing-a-detector template does not mention {requirement}"
    assert "make_fixtures.py" in text and "clean corpus" in text


# --- the false-positive gate -------------------------------------------------


def test_disabled_detectors_name_their_measurement():
    off = [row for row in rows() if not row["default_on"]]
    assert [row["id"] for row in off] == ["churn_loop", "rework_loop", "shotgun_edit", "copy_paste_growth"]
    for row in off:
        assert row["reenable"], f"{row['id']} ships off and does not say what turns it on"
        assert "e4.3a" in row["reenable"] or "reco-mkl" in row["reenable"], (
            f"{row['id']} names a wish, not the ticket that measures it: {row['reenable']}"
        )
        # A fixture that nobody re-runs when the detector is switched on is a fixture that lies.
        assert (FIXTURES / "fixtures" / f"{row['id']}.json").exists()
        assert load(FIXTURES / "fixtures" / f"{row['id']}.expected.json"), f"{row['id']}'s fixture is empty"
    for row in rows():
        if row["default_on"]:
            assert row["reenable"] == "", f"{row['id']} ships on and has nothing to re-enable"


def test_the_gate_itself_is_the_number_e0_12_set():
    """The doc states `e0.12`'s bar rather than inventing one, and the thesis document agrees.

    ADR-001 §5.4 is the guardrail section; `docs/concepts/thesis.md` is where its numbers live, and it is
    the file that says the number earlier tickets defer to is the one written there.
    """
    doc = DOC.read_text()
    assert "0.30" in doc and "0.15" in doc and "miss_rate" in doc, "the doc lost the bar it cites"
    thesis = (ROOT / "docs" / "concepts" / "thesis.md").read_text()
    assert "G3 ≤ 0.30" in thesis, "§5.4 no longer states the alpha bar this doc quotes"
    assert "miss_rate ≤ 0.20" in thesis, "§5.4 no longer states the miss-rate companion"
    # And the contract row for findings is the shape this catalogue writes.
    assert "detector_id, severity, confidence" in thesis


# --- the vocabularies, against the artefacts that own them -------------------


def test_the_vocabularies_are_adr_004_s():
    adr = ADR_STORAGE.read_text()
    severity = set(re.findall(r"severity\s+TEXT NOT NULL CHECK \(severity IN \(([^)]+)\)", adr)[0].replace("'", "").split(","))
    grade = set(re.findall(r"grade\s+TEXT NOT NULL CHECK \(grade IN \(([^)]+)\)", adr)[0].replace("'", "").split(","))
    assert severity == SEVERITIES, f"ADR-004's CHECK is {severity}; this catalogue writes {SEVERITIES}"
    assert grade == GRADES, f"ADR-004's grade CHECK is {grade}; this catalogue writes {GRADES}"

    kinds: set[str] = set()
    for findings in expectations(FIXTURES / "fixtures").values():
        for finding in findings:
            for evidence in finding["evidence"]:
                kinds.add(evidence.split(" ")[1])
    extra = kinds - ADR_EVIDENCE_KINDS
    assert extra <= CATALOGUE_EVIDENCE_KINDS, f"undocumented evidence kinds: {extra}"
    if extra:
        doc = DOC.read_text()
        assert "amendment" in doc and "`path`" in doc, "the extra kinds must be named as an ADR amendment"


def test_the_jump_views_are_the_tuis():
    views = set(re.findall(r'^\s+id: "([a-z]+)",\s*$', TUI_VIEWS.read_text(), re.M))
    assert views, "no view registry found to check against"
    blessed = set(catalogue()["views"])
    assert blessed <= views, f"this catalogue names views the TUI does not register: {blessed - views}"
    for name, findings in expectations(FIXTURES / "fixtures").items():
        for finding in findings:
            assert finding["jump_view"] in views, f"{name}: jump view {finding['jump_view']} is not registered"


def test_every_knob_is_declared_in_the_config_schema():
    schema = SCHEMA.read_text()
    declared = set(re.findall(r'path="(detectors\.[a-z_.]+)"', schema))
    used = set(catalogue()["knobs"])
    missing = used - declared
    assert not missing, (
        f"these knobs are read by the detectors and not declared in e0.8's schema: {sorted(missing)}"
    )
    for key, value in catalogue()["knobs"].items():
        if key.endswith(".suppress") or key.endswith(".enabled"):
            continue
        assert re.search(rf'path="{re.escape(key)}".*?default=', schema, re.S), f"{key} has no default in the schema"


def test_the_store_cache_entry_this_ticket_reads_is_still_the_one_it_shipped_with():
    """`findings.detect` names this module as its source; a detector that is not under that path is not
    part of any cached analysis, and the cache would happily serve a stale verdict about it."""
    cache = (ROOT / "crates" / "fathom-store" / "src" / "cache.rs").read_text()
    assert "crates/fathom-analyze/src/detectors/**" in cache, (
        "findings.detect no longer covers crates/fathom-analyze/src/detectors/**, so a detector change "
        "would not invalidate a cached finding"
    )


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-q"]))
