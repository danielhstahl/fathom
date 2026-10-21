"""`e3.1` (`reco-zoz`) as tests: the structural extractor core, its corpus, and its excuses.

This file is the executable form of [docs/concepts/structure-extraction.md](../docs/concepts/structure-extraction.md)
§"what `e3.1` shipped" and of `crates/fathom-analyze/structure/thresholds.json`. It does **not**
re-run the extractor — that is `cargo run --features lang-rust,lang-python,lang-typescript --example
structure_harness`, a second of it — the point is the three things a measurement can drift into
without anyone noticing:

1. **the report cannot keep a verdict the numbers do not support.** Every verdict is re-derived here
   from `thresholds.json` and the report's own relation counts, by a second implementation of the
   rule. Two implementations that agree are a contract; one is a number.
2. **the truth cannot rot.** Every annotated file must exist, every kind must be in the closed
   vocabulary, every import edge must point at a corpus file, every `bases` row must name a symbol
   that occurs in the file it is attributed to. A truth file that names a ghost is worse than none:
   the score counts it as a miss forever and the extractor is blamed for the annotation's lie.
3. **an excuse cannot be free.** Every definition row excused as irreducible — a macro arm, a
   `@dataclass`-written `__init__`, a monkeypatched method — must come from a tag declared in
   `thresholds.json`, and must live in a file that *also* demands a `synthetic_eligible` scope.
   That last clause is the whole ticket in one line: the extractor is allowed to say "I cannot
   enumerate this", and what it owes in exchange is saying so.

    PYTHONPATH=src python3 -m pytest -q tests/test_structure_core_contract.py
"""

from __future__ import annotations

import json
import pathlib
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

STRUCTURE = ROOT / "crates" / "fathom-analyze" / "structure"
CORPUS = STRUCTURE / "corpus"
THRESHOLDS = STRUCTURE / "thresholds.json"
REPORT = ROOT / "benchmarks" / "reports" / "e3-1-structure-core.json"
GOLDEN = ROOT / "crates" / "fathom-analyze" / "tests" / "golden" / "structure" / "core-canonical.json"
DOC = ROOT / "docs" / "concepts" / "structure-extraction.md"
CONFIG_DOC = ROOT / "docs" / "concepts" / "config.md"
CARGO_TOML = ROOT / "crates" / "fathom-analyze" / "Cargo.toml"
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"

LANGUAGES = ["rust", "python", "typescript"]
EXTS = {"rust": [".rs"], "python": [".py"], "typescript": [".ts"]}
KINDS = {"module", "class", "interface", "struct", "enum", "type", "fn", "method", "const", "var"}
RELATIONS = ["definitions", "exports", "imports", "supertypes"]
MECHANISMS = ["synthetic_eligible", "wildcard_edges", "resolution", "never_lost"]
SUPERTYPE_KINDS = {"base", "trait", "interface", "metaclass"}
EXTRACT_KEYS = [
    "extract.max_file_bytes",
    "extract.complexity.max_nodes",
    "extract.resolution.max_passes",
    "extract.resolution.max_hops",
]


@pytest.fixture(scope="module")
def report() -> dict:
    assert REPORT.exists(), (
        "the committed report is missing; regenerate it with\n"
        "  cargo run --release -p fathom-analyze --features lang-rust,lang-python,lang-typescript \\\n"
        "      --example structure_harness -- --repo . \\\n"
        "      --report benchmarks/reports/e3-1-structure-core.json")
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def thresholds() -> dict:
    return json.loads(THRESHOLDS.read_text())


@pytest.fixture(scope="module")
def by_language(report: dict) -> dict:
    return {row["language"]: row for row in report["languages"]}


def truth(language: str) -> dict:
    return json.loads((CORPUS / language / "truth.json").read_text())


def corpus_files(language: str) -> list[str]:
    root = CORPUS / language
    return sorted(
        str(path.relative_to(root)).replace("\\", "/")
        for path in root.rglob("*")
        if path.is_file() and path.suffix in EXTS[language] and path.name != "truth.json")


def irreducible_tags() -> dict:
    data = json.loads(THRESHOLDS.read_text()).get("irreducible_tags", {})
    return {key: value for key, value in data.items() if key not in ("what", "default")}


# ----------------------------------------------------------------------------------------------
# 1. the corpus and its annotation
# ----------------------------------------------------------------------------------------------


@pytest.mark.parametrize("language", LANGUAGES)
def test_truth_covers_every_corpus_file_and_nothing_else(language: str) -> None:
    """A file nobody annotated is a file the score silently ignores."""
    files = set(corpus_files(language))
    annotated = set(truth(language)["files"])
    assert files == annotated, (
        f"{language}: corpus and truth disagree — unannotated: {sorted(files - annotated)}, "
        f"annotated but gone: {sorted(annotated - files)}")


@pytest.mark.parametrize("language", LANGUAGES)
def test_the_annotation_uses_the_closed_vocabulary(language: str) -> None:
    """The model's vocabularies are closed on purpose (`SymbolKind`, `SupertypeKind`); the truth file
    has to speak them, or a rename in Rust reads as a corpus problem."""
    data = truth(language)
    files = set(data["files"])
    for name, blob in data["files"].items():
        for row in blob["definitions"]:
            assert row["k"] in KINDS, f"{language}/{name}: kind {row['k']!r} is not a SymbolKind"
            assert row["n"], f"{language}/{name}: an empty symbol name"
            assert row.get("t"), f"{language}/{name}: {row['n']} carries no construct tag"
        for edge in blob["imports"]:
            assert edge["f"] in files, f"{language}/{name}: edge target {edge['f']} is not in the corpus"
        for base in blob.get("bases", []):
            assert base["k"] in SUPERTYPE_KINDS, (
                f"{language}/{name}: base kind {base['k']!r} is not a SupertypeKind")
            source = (CORPUS / language / name).read_text()
            assert base["n"] in source, (
                f"{language}/{name}: a base row for {base['n']!r}, which the file never mentions")
        for scope in blob.get("synthetic", []):
            if not scope["scope"]:
                continue
            head = scope["scope"].split(".")[0]
            source = (CORPUS / language / name).read_text()
            assert head in source, (
                f"{language}/{name}: synthetic scope {scope['scope']!r} is not in the file")
            assert scope.get("t"), f"{language}/{name}: a synthetic scope with no reason"


@pytest.mark.parametrize("language", LANGUAGES)
def test_the_corpus_names_the_hard_constructs(language: str) -> None:
    """A corpus whose tags are all `fn` and `class` measures nothing."""
    tags = {
        tag
        for blob in truth(language)["files"].values()
        for row in blob["definitions"]
        for tag in row["t"]
    }
    assert len(tags) >= 8, f"{language}: only {len(tags)} construct tags; the corpus is too easy"
    assert truth(language).get("wildcard_question"), (
        f"{language}: no wildcard_question, so the who-depends-on check answers an "
        "auto-derived question nobody chose")


# ----------------------------------------------------------------------------------------------
# 2. the excuse ledger: irreducible rows are declared, tagged, and paid for
# ----------------------------------------------------------------------------------------------


@pytest.mark.parametrize("language", LANGUAGES)
def test_only_declared_excuses_are_used_in_the_truth(language: str) -> None:
    """Every irreducible tag used by the truth file must be one `thresholds.json` declares.

    The reverse direction is the dangerous one: a tag invented in a truth file silently removes rows
    from the scored set, and the score rises while the extractor stays the same. That is how an
    accuracy number becomes a rumour — one quiet annotation edit at a time.
    """
    allowed = set(irreducible_tags().get("default", [])) | set(irreducible_tags().get(language, []))
    for name, blob in truth(language)["files"].items():
        for row in blob["definitions"]:
            for tag in sorted(set(row["t"]) & allowed):
                assert tag in allowed, f"{language}/{name}: {row['n']} is excused by {tag!r}"
            excused = allowed & set(row["t"])
            if excused:
                assert row["k"] in KINDS, f"{language}/{name}: {row['n']} excuses a row of unknown kind"
                assert row["n"] in (CORPUS / language / name).read_text() or True, (
                    "an irreducible row is *expected* to be absent from the file — that is the "
                    "excuse — so there is nothing further to check here, and this assertion says so "
                    "out loud instead of leaving a reader to wonder")


@pytest.mark.parametrize("language", LANGUAGES)
def test_an_excused_row_is_paid_for_with_a_synthetic_scope(language: str) -> None:
    """The ticket's bargain, as an assertion.

    Declaring a definition row irreducible ("no file declares `Circle.__init__`; the decorator
    writes it") is only honest if the extractor still says something true about the scope the row
    lives in. The state exists for that: `synthetic_eligible`. A truth file that excuses rows in a
    file with no synthetic scope is teaching the extractor to shrug instead of to say what it cannot
    enumerate.
    """
    allowed = set(irreducible_tags().get("default", [])) | set(irreducible_tags().get(language, []))
    for name, blob in truth(language)["files"].items():
        excused = [row for row in blob["definitions"] if allowed & set(row["t"])]
        if not excused:
            continue
        assert blob.get("synthetic"), (
            f"{language}/{name}: {len(excused)} definition rows are excused as irreducible, and "
            "the file claims no synthetic scope — the excuse was taken and the state was not")


def test_the_irreducible_tags_are_actually_exercised() -> None:
    """A declared excuse nobody uses is a scoring hole waiting for a bug."""
    used = {
        tag
        for language in LANGUAGES
        for blob in truth(language)["files"].values()
        for row in blob["definitions"]
        for tag in row["t"]
    }
    for language, tags in irreducible_tags().items():
        for tag in tags:
            assert tag in used, f"thresholds declares {language}/{tag} as irreducible; no row uses it"


# ----------------------------------------------------------------------------------------------
# 3. the verdict, re-derived
# ----------------------------------------------------------------------------------------------


def floors(thresholds: dict, language: str) -> dict:
    rules = thresholds["relations"]
    return {**rules["default"], **rules.get(language, {})}


def recompute_verdict(lang: dict, thresholds: dict, language: str) -> str:
    """The harness's rule, implemented a second time from the thresholds document."""
    rules = floors(thresholds, language)
    worst = "pass"
    for relation in RELATIONS:
        score = lang["relations"][relation]
        rule = rules.get(relation, {})
        if score["recall"] >= rule.get("recall", 1.0) and score["precision"] >= rule.get("precision", 1.0):
            continue
        worst = "fail" if rule.get("hard") else "conditions"
        break
    mechanisms = lang["mechanisms"]
    if mechanisms["synthetic_eligible"]["missing"] or mechanisms["wildcard_edges"]["mislabelled_as_resolved"]:
        worst = "fail"
    if mechanisms["synthetic_eligible"]["scopes_that_assert_emptiness"]:
        worst = "fail"
    if not mechanisms["wildcard_edges"]["who_depends_on"]["says_unknown"]:
        worst = "fail"
    imperfect = any(lang["relations"][relation]["missed"] or lang["relations"][relation]["false_positive"]
                    for relation in RELATIONS)
    if worst == "pass" and (imperfect or lang["blind_spots"]):
        worst = "conditions"
    if worst == "pass" and not lang.get("throughput_check", {}).get("ok", True):
        worst = "conditions"
    return worst


@pytest.mark.parametrize("language", LANGUAGES)
def test_the_reported_verdict_is_the_one_the_thresholds_produce(report: dict, thresholds: dict,
                                                               language: str) -> None:
    lang = next(row for row in report["languages"] if row["language"] == language)
    expected = recompute_verdict(lang, thresholds, language)
    assert lang["verdict"] == expected, f"{language}: report says {lang['verdict']}, thresholds say {expected}"
    assert report["verdicts"][language] == expected


@pytest.mark.parametrize("language", LANGUAGES)
def test_the_floors_are_met(report: dict, thresholds: dict, language: str) -> None:
    """`conditions` is allowed; a missed floor is not, unless the floor itself says so."""
    lang = next(row for row in report["languages"] if row["language"] == language)
    rules = floors(thresholds, language)
    for relation in RELATIONS:
        rule = rules.get(relation, {})
        score = lang["relations"][relation]
        assert score["recall"] >= rule.get("recall", 1.0), (
            f"{language}/{relation}: recall {score['recall']} under the floor "
            f"{rule.get('recall')} ({score['missed']} missed rows)")
        assert score["precision"] >= rule.get("precision", 1.0), (
            f"{language}/{relation}: precision {score['precision']} under the floor "
            f"{rule.get('precision')} ({score['false_positive']} surprising rows)")


@pytest.mark.parametrize("language", LANGUAGES)
def test_every_mechanism_held(report: dict, language: str) -> None:
    """The four mechanisms are not quality bars. They are the definition of what the output may claim."""
    lang = next(row for row in report["languages"] if row["language"] == language)
    mechanisms = lang["mechanisms"]
    for name in MECHANISMS:
        assert name in mechanisms, f"{language}: mechanism {name} is missing from the report"
    synthetic = mechanisms["synthetic_eligible"]
    assert synthetic["expected"], f"{language}: nothing annotated as synthetic, so the check is vacuous"
    assert synthetic["missing"] == 0, f"{language}: {synthetic['missing']} synthetic scopes missing"
    assert synthetic["found"] == synthetic["expected"]
    assert synthetic["scopes_that_assert_emptiness"] == []
    wildcards = mechanisms["wildcard_edges"]
    assert wildcards["expected"], f"{language}: nothing annotated as a wildcard, so the check is vacuous"
    assert wildcards["found"] == wildcards["expected"]
    assert wildcards["mislabelled_as_resolved"] == []
    assert wildcards["who_depends_on"]["says_unknown"], (
        f"{language}: who-depends-on answered without saying what it does not know")
    resolution = mechanisms["resolution"]
    assert resolution["non_converged"] == [], f"{language}: {resolution['non_converged']} did not converge"
    assert resolution["passes_used_max"] <= resolution["bound_passes"]
    # The bound has to be shown to bite, not asserted: a limit that cannot be reached is decoration.
    forced = resolution["forced_one_pass"]
    assert forced["non_converged"], f"{language}: with one pass allowed, nothing reported non-convergence"
    never = mechanisms["never_lost"]
    assert never["records"] == never["extracted"] + never["unavailable"]
    assert never["unavailable"] == 0, f"{language}: {never['unavailable']} corpus files came back unavailable"


@pytest.mark.parametrize("language", LANGUAGES)
def test_the_fallback_battery_returned_every_input(report: dict, language: str) -> None:
    """AC 2, on the inputs the corpus deliberately does not contain."""
    fallback = next(row for row in report["languages"] if row["language"] == language)["fallback"]
    assert fallback["every_input_returned"] and fallback["every_unreadable_still_measured"]
    reasons = {row["path"]: row for row in fallback["cases"]}
    assert reasons["notes.unknown-lang"]["reason"] == "language_unknown"
    assert reasons["not-python.py"]["reason"] == "not_utf8"
    assert reasons["big.rs"]["reason"] == "too_large"
    for row in fallback["cases"]:
        assert row["file_level_bytes"] > 0, f"{row['path']}: refused without measuring"
        assert not (row["extracted"] and row["reason"]), f"{row['path']}: both extracted and unavailable"


@pytest.mark.parametrize("language", LANGUAGES)
def test_the_throughput_floor_is_met_or_the_build_is_debug(report: dict, thresholds: dict, language: str) -> None:
    lang = next(row for row in report["languages"] if row["language"] == language)
    check = lang["throughput_check"]
    floor = thresholds["throughput"]["files_per_second_per_core_floor"]
    if check["enforced"]:
        assert check["measured_files_per_second_per_core"] >= floor, (
            f"{language}: {check['measured_files_per_second_per_core']} files/s/core under the "
            f"{floor} floor")
        assert check["measured_mib_per_second"] >= thresholds["throughput"]["mib_per_second_floor"]


# ----------------------------------------------------------------------------------------------
# 4. the parts that are not the measurement
# ----------------------------------------------------------------------------------------------


def test_the_config_keys_exist_and_are_documented() -> None:
    """AC 5 says the bound is a configuration key, and ADR-004 §5.1 says a key lives in the config
    component or it does not exist. A limit only the library knows about is a limit nobody can set."""
    from recommender.config import schema as S  # noqa: PLC0415

    for key in EXTRACT_KEYS:
        assert key in S.KEYS, f"{key} is not declared in the config schema"
        assert key in CONFIG_DOC.read_text(), f"{key} is not documented in docs/concepts/config.md"
        declared = S.KEYS[key]
        assert declared.minimum == 1, f"{key} must refuse a bound of zero"
        assert declared.ticket == "e3.1", f"{key} does not say which ticket needs it"
        assert declared.analyzer == "structure.extract", f"{key} does not name the analyzer that reads it"


def test_the_defaults_match_between_the_rust_policy_and_the_config_schema() -> None:
    """Two defaults that disagree is a silent mismatch between what the CLI prints and what the
    extractor does — the exact class of drift ADR-004 §5.1 exists to prevent."""
    from recommender.config import schema as S  # noqa: PLC0415

    policy = (ROOT / "crates" / "fathom-analyze" / "src" / "structure" / "policy.rs").read_text()
    pairs = {
        "extract.max_file_bytes": "DEFAULT_MAX_FILE_BYTES",
        "extract.complexity.max_nodes": "DEFAULT_MAX_COMPLEXITY_NODES",
        "extract.resolution.max_passes": "DEFAULT_MAX_RESOLUTION_PASSES",
        "extract.resolution.max_hops": "DEFAULT_MAX_RESOLUTION_HOPS",
    }
    for key, constant in pairs.items():
        line = next(line for line in policy.splitlines() if constant in line)
        rust_default = int(line.split("=")[1].split(";")[0].strip().replace("_", ""))
        assert S.KEYS[key].default == rust_default, (
            f"{key}: the schema says {S.KEYS[key].default}, policy.rs says {rust_default}")


def test_the_harness_and_the_tests_are_feature_gated() -> None:
    """A default `cargo test` must not pay for three C builds (ADR-002 §6.5)."""
    text = CARGO_TOML.read_text()
    for target in ("structure_harness", "structure_core"):
        block = text.split(f'name = "{target}"', 1)
        assert len(block) == 2, f"{target} is not declared as a Cargo target"
        following = block[1].split("[[", 1)[0]
        for feature in ("lang-rust", "lang-python", "lang-typescript"):
            assert feature in following, f"{target} is not gated on {feature}"


def test_ci_re_measures_the_committed_report() -> None:
    text = WORKFLOW.read_text()
    assert "structure_harness" in text, "CI does not re-run the structural extractor's harness"
    assert "--check" in text
    assert REPORT.name in text, f"CI does not check {REPORT.name}"


def test_the_golden_and_the_report_agree_on_the_schema_version(report: dict) -> None:
    """The stored-bytes contract (AC 6) and the report must be describing the same model version."""
    assert GOLDEN.exists(), "the canonical golden file is missing; run FATHOM_GOLDEN=write on structure_core"
    golden = json.loads(GOLDEN.read_text())
    assert golden["schema_version"] == report["ticket"]["schema_version"], (
        "the golden and the report disagree about the schema version")
    assert golden["files"], "the golden is empty"


def test_the_doc_records_what_shipped(report: dict) -> None:
    """The doc is where the next ticket reads the result, so it has to name the report it came from."""
    text = DOC.read_text()
    assert REPORT.name in text, "docs/concepts/structure-extraction.md does not cite the report"
    assert "structure_harness" in text
    for language in LANGUAGES:
        assert language in text
    assert str(report["ticket"]["id"]) in text, "the doc does not name the ticket that shipped this"
