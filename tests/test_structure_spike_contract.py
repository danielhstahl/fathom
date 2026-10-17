"""Spike `s1` (`reco-alj`), as tests: the tree-sitter verdict is measured or it is a rumour.

ADR-002 §6.5 gated each grammar behind its own feature and said "`s1` decides whether any of the
six is not worth shipping". This file is the executable form of that decision: the committed report
(`benchmarks/reports/s1-tree-sitter-accuracy.json`) is checked against the thresholds that were
supposed to produce it (`crates/fathom-analyze/spike/s1/thresholds.json`), against the prose that
records it (`docs/concepts/structure-extraction.md`), and against the corpus the numbers were
pulled from (`crates/fathom-analyze/spike/s1/corpus/`).

The point is not to re-run the harness — that is `cargo run --features langs-all --example
s1_spike`, two seconds of it — the point is that **the document cannot keep a verdict the
measurement does not support**, and that the annotated truth cannot rot into something the corpus
no longer contains.

    PYTHONPATH=src python3 -m pytest -q tests/test_structure_spike_contract.py
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s1-tree-sitter-accuracy.json"
THRESHOLDS = ROOT / "crates" / "fathom-analyze" / "spike" / "s1" / "thresholds.json"
CORPUS = ROOT / "crates" / "fathom-analyze" / "spike" / "s1" / "corpus"
DOC = ROOT / "docs" / "concepts" / "structure-extraction.md"
CARGO_TOML = ROOT / "crates" / "fathom-analyze" / "Cargo.toml"
CARGO_LOCK = ROOT / "Cargo.lock"

LANGUAGES = ["rust", "python", "typescript", "go", "ruby", "java"]
EXTS = {
    "rust": [".rs"],
    "python": [".py"],
    "typescript": [".ts"],
    "go": [".go"],
    "ruby": [".rb"],
    "java": [".java"],
}
KINDS = {"module", "class", "interface", "struct", "enum", "type", "fn", "method", "const", "var"}
RELATIONS = ["definitions", "exports", "imports"]


@pytest.fixture(scope="module")
def report() -> dict:
    assert REPORT.exists(), (
        "spike s1's report is missing; regenerate it with\n"
        "  cargo run --release -p fathom-analyze --features langs-all --example s1_spike -- \\\n"
        "      --repo . --report benchmarks/reports/s1-tree-sitter-accuracy.json")
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def thresholds() -> dict:
    return json.loads(THRESHOLDS.read_text())


@pytest.fixture(scope="module")
def by_language(report: dict) -> dict:
    return {lang["language"]: lang for lang in report["languages"]}


def truth(language: str) -> dict:
    return json.loads((CORPUS / language / "truth.json").read_text())


def corpus_files(language: str) -> list[str]:
    root = CORPUS / language
    return sorted(
        str(p.relative_to(root)).replace("\\", "/")
        for p in root.rglob("*")
        if p.is_file() and p.suffix in EXTS[language] and p.name != "truth.json"
    )


# ----------------------------------------------------------------------------------------------
# the corpus and its annotation
# ----------------------------------------------------------------------------------------------


@pytest.mark.parametrize("language", LANGUAGES)
def test_truth_covers_every_file_in_the_corpus(language: str) -> None:
    """A file nobody annotated is a file the score is silently ignoring."""
    files = set(corpus_files(language))
    annotated = set(truth(language)["files"])
    assert files == annotated, (
        f"{language}: corpus and truth.json disagree — "
        f"unannotated: {sorted(files - annotated)}, annotated but gone: {sorted(annotated - files)}")


@pytest.mark.parametrize("language", LANGUAGES)
def test_every_kind_and_edge_target_is_something_the_corpus_has(language: str) -> None:
    """The vocabulary is closed, and an import edge has to point at a file that exists."""
    data = truth(language)
    files = set(data["files"])
    for name, blob in data["files"].items():
        for row in blob["definitions"]:
            assert row["k"] in KINDS, f"{language}/{name}: kind {row['k']!r} is not in the vocabulary"
            assert row["n"], f"{language}/{name}: an empty symbol name"
            assert row.get("t"), f"{language}/{name}: {row['n']} carries no construct tag"
        source = (CORPUS / language / name).read_text()
        for exported in blob["exports"]:
            # Every segment of an exported name has to appear in the file's own text. This is a
            # ghost check, not a semantics check: a truth file that names a symbol nobody wrote is
            # worse than no truth file, because the score quietly counts it as a miss.
            if exported == "default":
                continue
            for segment in exported.replace("'", ".").split("."):
                if segment:
                    assert segment in source, (
                        f"{language}/{name}: exports {exported!r}, and {segment!r} does not "
                        "appear in the file")
        for edge in blob["imports"]:
            assert edge["f"] in files, f"{language}/{name}: import target {edge['f']} is not in the corpus"


@pytest.mark.parametrize("language", LANGUAGES)
def test_annotations_tag_the_constructs_the_spike_exists_to_test(language: str) -> None:
    """The point of a hand-authored corpus is that the interesting cases are named.

    A language whose tags are all `fn` and `class` measures nothing: it scores the easy half of the
    grammar and calls it accuracy.
    """
    tags = {
        tag
        for blob in truth(language)["files"].values()
        for row in blob["definitions"]
        for tag in row["t"]
    }
    assert len(tags) >= 8, f"{language}: only {len(tags)} construct tags; the corpus is too easy"


# ----------------------------------------------------------------------------------------------
# the verdict, re-derived
# ----------------------------------------------------------------------------------------------


def floors(thresholds: dict, language: str) -> dict:
    rules = thresholds["relations"]
    return {**rules["default"], **rules.get(language, {})}


def recompute_verdict(report: dict, thresholds: dict, language: str) -> str:
    """Independent re-derivation of the harness's decision rule. Two implementations must agree."""
    lang = next(l for l in report["languages"] if l["language"] == language)
    rules = floors(thresholds, language)
    worst = "go"
    for relation in RELATIONS:
        score = lang["relations"][relation]
        rule = rules.get(relation, {})
        if score["recall"] >= rule.get("recall", 1.0) and score["precision"] >= rule.get("precision", 1.0):
            continue
        worst = "no_go" if rule.get("hard") else "go_with_conditions"
        break
    imperfect = any(lang["relations"][r]["missed"] or lang["relations"][r]["false_positive"] for r in RELATIONS)
    if worst == "go" and imperfect:
        worst = "go_with_conditions"
    check = lang.get("throughput_check", {})
    if worst == "go" and check and not check.get("ok", True):
        worst = "go_with_conditions"
    return worst


@pytest.mark.parametrize("language", LANGUAGES)
def test_the_reported_verdict_is_the_one_the_thresholds_produce(report: dict, thresholds: dict,
                                                               language: str) -> None:
    expected = recompute_verdict(report, thresholds, language)
    lang = next(l for l in report["languages"] if l["language"] == language)
    assert lang["verdict"] == expected, f"{language}: report says {lang['verdict']}, thresholds say {expected}"
    assert report["verdicts"][language] == expected


@pytest.mark.parametrize("language", LANGUAGES)
def test_no_language_is_no_go_and_every_condition_is_written_down(report: dict, language: str) -> None:
    """ADR-002 §6.5 asked whether to drop a grammar. The answer must stay readable and justified."""
    lang = next(l for l in report["languages"] if l["language"] == language)
    assert lang["verdict"] != "no_go", f"{language}: a NO-GO would reopen ADR-002 §6.5"
    if lang["verdict"] == "go":
        return
    irreducible = irreducible_list()
    kinds = {spot["kind"] for spot in lang["blind_spots"]}
    imperfect = any(lang["relations"][r]["missed"] for r in RELATIONS)
    assert kinds or imperfect, f"{language}: conditions with nothing to justify them"
    if imperfect:
        assert kinds & set(irreducible.get(language, [])) or lang.get("throughput_check", {}).get("ok") is False, (
            f"{language}: it missed declarations, and none of its blind spots is listed as "
            "irreducible — either the extractor has a bug or the thresholds are wrong")


def irreducible_list() -> dict:
    data = json.loads(THRESHOLDS.read_text()).get("irreducible_blind_spots", {})
    return {k: v for k, v in data.items() if k not in ("what", "default")}


# Categories listed in the thresholds as a synonym or a near-miss of one that did fire; they are
# kept so a run on a different corpus does not silently become "unlisted, therefore a bug".
ALIASES = {
    "rust": {"macro_generated_item"},
    "python": {"dataclass_generated", "generated_method"},
    "ruby": {"define_method", "monkeypatch"},
    "java": set(),
    "typescript": set(),
    "go": set(),
}


def test_irreducible_categories_are_all_recorded_or_explained(thresholds: dict,
                                                             by_language: dict) -> None:
    """A listed category that never fired is dead weight unless it is a declared near-miss; a fired
    one that is unlisted claims the extractor is broken."""
    irreducible = irreducible_list()
    assert set(irreducible) == set(LANGUAGES), "every language needs a list, even an empty one"
    for language, lang in by_language.items():
        recorded = {spot["kind"] for spot in lang["blind_spots"]}
        listed = set(irreducible.get(language, []))
        dead = sorted(listed - recorded - ALIASES[language])
        assert not dead, f"{language}: thresholds list {dead} but the run recorded none of them"


def test_accuracy_is_stated_at_the_granularity_the_plan_needs(report: dict) -> None:
    """`e3.1`'s deliverable is a graph, so the report has to score edges, not just symbols."""
    for lang in report["languages"]:
        for relation in RELATIONS:
            score = lang["relations"][relation]
            assert 0.0 <= score["precision"] <= 1.0 and 0.0 <= score["recall"] <= 1.0
            assert score["true_positive"] > 0, f"{lang['language']}/{relation}: nothing was found"
        assert lang["corpus"]["import_edges"] >= 7, f"{lang['language']}: too few annotated edges to score"


# ----------------------------------------------------------------------------------------------
# the document
# ----------------------------------------------------------------------------------------------


def test_generated_tables_match_the_report() -> None:
    proc = subprocess.run([sys.executable, "tools/gen_structure_tables.py", "check"], cwd=ROOT,
                          capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr


def test_the_document_keeps_the_verdict_it_claims(report: dict) -> None:
    """The intro says no grammar is a dealbreaker; if a NO-GO appeared, the prose has to change."""
    text = DOC.read_text()
    assert "no_go" not in json.dumps(report["verdicts"]), (
        "structure-extraction.md opens by saying all six grammars ship; a NO-GO needs new prose")
    for phrase in ("all six grammars ship", "macro", "Ruby", "package"):
        assert phrase in text, f"structure-extraction.md must keep saying: {phrase!r}"
    for language in LANGUAGES:
        assert f"**{language}**" in text or f"| {language} " in text, f"{language} is not in the tables"


def test_the_document_names_the_normalisation_rules_the_corpus_commits_to() -> None:
    """The identity rule is the part `e3.1` inherits; if the section goes, the corpus is orphaned."""
    text = DOC.read_text()
    for rule in ("(file, kind, dotted name)", "Declarations inside a function body are not symbols",
                 "Fields are not symbols", "export is a name nameable"):
        assert rule in text, f"the normalisation rules must keep stating: {rule}"


def test_the_harness_stays_out_of_a_default_build() -> None:
    """Six grammars are six C builds (ADR-002 §6.5); the spike must not make `cargo test` pay."""
    toml = CARGO_TOML.read_text()
    assert 'name = "s1_spike"' in toml
    block = toml.split('name = "s1_spike"', 1)[1].split("[[", 1)[0]
    assert "required-features" in block and "langs-all" in block


def test_the_report_names_the_grammars_the_lock_file_pins(report: dict) -> None:
    """A grammar bump changes what is extractable; the report is where that becomes visible."""
    lock = CARGO_LOCK.read_text()
    for grammar in report["grammars"]:
        assert f'name = "{grammar["crate"]}"' in lock
        assert f'version = "{grammar["version"]}"' in lock, (
            f"{grammar['crate']}: the report pins {grammar['version']}, Cargo.lock does not")
        assert grammar["grammar_abi"] > 0
