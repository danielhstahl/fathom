"""`e3.1b` (`reco-dlp.1`) as tests: the structural extractor tail, and the three scan-policy knobs.

This file is the executable form of the tail half of
[docs/concepts/structure-extraction.md](../docs/concepts/structure-extraction.md) and of the three
sections `thresholds.json` grew (`throughput.exempt`, `same_package`, `granularity.edge_granularity`).
Like `tests/test_structure_core_contract.py` it does **not** re-run the extractor — the harness does
that, and a second scorer is how a measurement goes quiet. What it re-derives is the parts a report
can drift into without anyone noticing:

1. **Go's package-granularity cannot be applied to something that is not a package.** The count is
   checked against the annotation that asked for it, the label has to exist, and every language
   without package-granular edges has to say it has none — otherwise the label becomes a decoration
   a pane prints on everything.
2. **The Ruby scan budget cannot become a silent data-loss switch.** The configured caps have to
   match `policy.rs`, the demonstration has to actually refuse files, every refused file has to
   still carry its file-level numbers, and the *cost* of the cap has to fit the budget it is claimed
   against. The exemption that lets Ruby under the throughput floor is bought with that arithmetic,
   so the arithmetic is asserted rather than trusted.
3. **The Java same-package rate cannot be a paragraph.** The corpus size, the budget, the measured
   rate and the wide-scan comparison all have to be in the report, and the wide column has to be
   *worse* — a scoping decision that changes nothing was not a decision.
4. **The knobs have to exist in the config component**, with the same defaults Rust uses, or the
   report describes limits nobody can set (ADR-004 §5.1).

    PYTHONPATH=src python3 -m pytest -q tests/test_structure_tail_contract.py
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

STRUCTURE = ROOT / "crates" / "fathom-analyze" / "structure"
CORPUS = STRUCTURE / "corpus"
THRESHOLDS = STRUCTURE / "thresholds.json"
REPORT = ROOT / "benchmarks" / "reports" / "e3-1-structure-core.json"
DOC = ROOT / "docs" / "concepts" / "structure-extraction.md"
CONFIG_DOC = ROOT / "docs" / "concepts" / "config.md"
CARGO_TOML = ROOT / "crates" / "fathom-analyze" / "Cargo.toml"
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
POLICY_RS = ROOT / "crates" / "fathom-analyze" / "src" / "structure" / "policy.rs"
MOD_RS = ROOT / "crates" / "fathom-analyze" / "src" / "structure" / "mod.rs"
JAVA_GENERATOR = ROOT / "crates" / "fathom-analyze" / "examples" / "structure_harness" / "java_corpus.rs"
GOLDEN = ROOT / "crates" / "fathom-analyze" / "tests" / "golden" / "structure" / "core-canonical.json"

CORE = ["rust", "python", "typescript"]
TAIL = ["go", "ruby", "java"]
EXTS = {"go": [".go"], "ruby": [".rb"], "java": [".java"]}
KINDS = {"module", "class", "interface", "struct", "enum", "type", "fn", "method", "const", "var"}
SUPERTYPE_KINDS = {"base", "trait", "interface", "metaclass", "self", "capability"}
SCAN_KEYS = {
    "extract.ruby.max_files_per_run": "DEFAULT_RUBY_MAX_FILES_PER_RUN",
    "extract.ruby.max_bytes_per_run": "DEFAULT_RUBY_MAX_BYTES_PER_RUN",
    "extract.java.same_package_scan": "DEFAULT_JAVA_SAME_PACKAGE_SCAN",
}
RELATIONS = ["definitions", "exports", "imports", "supertypes"]


@pytest.fixture(scope="module")
def report() -> dict:
    assert REPORT.exists(), (
        "the committed report is missing; regenerate it with\n"
        "  cargo run --release -p fathom-analyze --features langs-all --example structure_harness "
        "-- --repo . --report benchmarks/reports/e3-1-structure-core.json")
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def thresholds() -> dict:
    return json.loads(THRESHOLDS.read_text())


def truth(language: str) -> dict:
    return json.loads((CORPUS / language / "truth.json").read_text())


def row(report: dict, language: str) -> dict:
    return next(entry for entry in report["languages"] if entry["language"] == language)


def corpus_files(language: str) -> list[str]:
    root = CORPUS / language
    return sorted(
        str(path.relative_to(root)).replace("\\", "/")
        for path in root.rglob("*")
        if path.is_file() and path.suffix in EXTS[language] and path.name != "truth.json")


def ruby_priority() -> list[str] | None:
    """`DEFAULT_RUBY_PRIORITY` out of `policy.rs`, as the list of prefixes it is."""
    found = re.search(
        r"DEFAULT_RUBY_PRIORITY: &\[&str\] = &\[([^\]]*)\]", POLICY_RS.read_text()
    )
    if not found:
        return None
    return [part.strip().strip('"') for part in found.group(1).split(",")]


def rust_constant(name: str) -> str:
    """One `pub const NAME: _ = value;` line out of `policy.rs`, as text."""
    for line in POLICY_RS.read_text().splitlines():
        if name in line and line.strip().startswith("pub const"):
            return line.split("=", 1)[1].split(";")[0].strip().strip('"')
    raise AssertionError(f"{name} is not declared in policy.rs")


# ---------------------------------------------------------------------------
# 1. the tail joined the report rather than starting a second one
# ---------------------------------------------------------------------------


def test_the_three_new_languages_are_in_the_same_report_as_the_old_three(report: dict) -> None:
    """The acceptance criterion is one harness, because a second harness is a second set of rules
    about what "measured" means."""
    assert [entry["language"] for entry in report["languages"]] == CORE + TAIL, (
        "the report's languages moved; the tail was supposed to join this list, not restart it")
    tail = report["ticket"]["tail"]
    assert tail["id"] == "reco-dlp.1"
    assert tail["plan_key"] == "e3.1b"
    assert tail["added"] == TAIL
    assert report["ticket"]["id"] == "reco-zoz", "the core ticket is still the one that owns the report"


@pytest.mark.parametrize("language", TAIL)
def test_the_tail_corpora_are_annotated_as_carefully_as_the_core_ones(language: str) -> None:
    """The same rules `e3.1` set for its corpus: nothing unannotated, a closed vocabulary, and a
    chosen wildcard question — a truth file that names a ghost is worse than none."""
    data = truth(language)
    files = set(corpus_files(language))
    assert files == set(data["files"]), (
        f"{language}: the corpus and the annotation disagree on which files exist: "
        f"{sorted(files ^ set(data['files']))}")
    assert data.get("wildcard_question"), f"{language}: no wildcard_question"
    assert data.get("notes"), f"{language}: no notes, so the report prints an empty excuse"
    for name, blob in data["files"].items():
        for definition in blob["definitions"]:
            assert definition["k"] in KINDS, f"{language}/{name}: kind {definition['k']!r}"
            assert definition["n"], f"{language}/{name}: an empty symbol name"
            assert definition.get("t"), f"{language}/{name}: {definition['n']} carries no tag"
        for edge in blob["imports"]:
            assert edge["f"] in files or not edge["f"], (
                f"{language}/{name}: edge target {edge['f']} is not a corpus file")
        for base in blob.get("bases", []):
            assert base["k"] in SUPERTYPE_KINDS, f"{language}/{name}: base kind {base['k']!r}"
            source = (CORPUS / language / name).read_text()
            # Ruby and Java name a type from its namespace (`Demo.Core.Shape`, `app.model.Circle`)
            # while the file spells it locally (`module Shape`, `class Circle`), so the local tail is
            # the part that has to appear. Checking the dotted name would be checking the annotation
            # against itself.
            assert base["n"].split(".")[-1] in source, (
                f"{language}/{name}: a base row for {base['n']!r}, which the file never mentions")
        for scope in blob.get("synthetic", []):
            assert scope.get("t"), f"{language}/{name}: a synthetic scope with no reason"


@pytest.mark.parametrize("language", TAIL)
def test_every_language_came_back_and_none_of_them_was_lost(report: dict, language: str) -> None:
    entry = row(report, language)
    never = entry["mechanisms"]["never_lost"]
    assert never["records"] == never["extracted"] + never["unavailable"]
    assert never["unavailable"] == 0, f"{language}: {never['unavailable']} files came back unavailable"
    for relation in RELATIONS:
        score = entry["relations"][relation]
        assert score["recall"] >= 0.9, f"{language}/{relation}: recall {score['recall']}"
        assert score["precision"] >= 0.9, f"{language}/{relation}: precision {score['precision']}"


@pytest.mark.parametrize("language", TAIL)
def test_the_tail_declares_its_excuses_and_uses_them(language: str, thresholds: dict) -> None:
    """The same bargain `e3.1` set: an irreducible tag has to be declared in `thresholds.json`, has to
    be used by a real row, and has to be paid for with a marked scope.

    Ruby's `define_method` loop and Java's record accessors are the two places the tail could have
    quietly widened the excuse instead of widening the extractor, so both directions are checked: no
    undeclared tag, and no declared tag nobody uses."""
    declared = thresholds.get("irreducible_tags", {})
    allowed = set(declared.get("default", [])) | set(declared.get(language, []))
    used: set[str] = set()
    for name, blob in truth(language)["files"].items():
        rows = [row for row in blob["definitions"] if allowed & set(row["t"])]
        for row in blob["definitions"]:
            used.update(row["t"])
        if not rows:
            continue
        assert blob.get("synthetic"), (
            f"{language}/{name}: {len(rows)} definition rows are excused as irreducible, and the "
            "file claims no synthetic scope — the excuse was taken and the state was not")
    for tag in declared.get(language, []):
        assert tag in used, f"thresholds declares {language}/{tag}; no corpus row carries it"


# ---------------------------------------------------------------------------
# 2. Go: a package edge is labelled, and nothing else borrows the label
# ---------------------------------------------------------------------------


def test_go_edges_are_package_granular_and_the_annotation_is_what_asked_for_it(report: dict) -> None:
    go = row(report, "go")
    granularity = go["granularity"]
    assert granularity["edges_package_granular"] > 0, "no Go edge is package-granular at all"
    assert granularity["edges_package_granular"] <= granularity["edges_total"]
    assert granularity["label_for_a_pane"], (
        "the model owes a surface the wording; a count without a sentence is a count a pane invents "
        "its own wording for")
    # The count is not free-floating: the annotation's wildcard rows are exactly the package edges,
    # which is the same fact the harness scores as `wildcard_edges`, approached from the other side.
    assert granularity["edges_package_granular"] == go["mechanisms"]["wildcard_edges"]["expected"], (
        "Go's package-granular edges and the annotation's wildcard rows are no longer the same set")
    assert granularity["declared_units"], "no `package` clause was recorded, so the unit is unnamed"


@pytest.mark.parametrize("language", [language for language in CORE + TAIL if language != "go"])
def test_no_other_language_borrows_the_package_label(report: dict, language: str) -> None:
    """A label applied to a file-level edge is a dependency the language never declared, and a label
    applied to everything is a label nobody reads."""
    entry = row(report, language)
    assert entry["granularity"]["edges_package_granular"] == 0, (
        f"{language}: package-granular edges appeared in a language whose imports name files")
    assert entry["granularity"]["label_for_a_pane"] is None


def test_the_granularity_rule_is_written_down_where_a_reader_will_hit_it(thresholds: dict) -> None:
    assert "edge_granularity" in thresholds["granularity"], (
        "thresholds.json is where a reader learns what an edge row means; the granularity rule has "
        "to be there and not only in the Rust doc comment")


# ---------------------------------------------------------------------------
# 3. Ruby: the scan budget is a bound with a price, not a switch
# ---------------------------------------------------------------------------


def test_ruby_is_the_only_language_with_a_default_cap(report: dict) -> None:
    """One cap, for the one grammar `s1` measured an order of magnitude off the pack. Caps on
    everything are caps nobody tunes; caps on nothing leave finding 7 unfixed."""
    configured = {
        entry["language"]: entry["scan_budget"]["configured"] for entry in report["languages"]
    }
    assert isinstance(configured["ruby"], dict), "a default budget that is not a budget"
    for language, budget in configured.items():
        if language != "ruby":
            assert budget is None, f"{language} has a scan budget by default; s1 measured it inside the pack"
    assert configured["ruby"]["priority"], "a budget with no priority order refuses by accident"


def test_the_scan_budget_defaults_are_the_same_numbers_rust_uses(report: dict) -> None:
    """Two defaults that disagree is a report that describes limits no operator can reproduce —
    ADR-004 §5.1's whole argument, applied to the tail's keys."""
    budget = row(report, "ruby")["scan_budget"]["configured"]
    assert budget["max_files_per_run"] == int(rust_constant("DEFAULT_RUBY_MAX_FILES_PER_RUN"))
    assert budget["max_bytes_per_run"] == int(rust_constant("DEFAULT_RUBY_MAX_BYTES_PER_RUN"))
    priority = ruby_priority()
    assert priority, "DEFAULT_RUBY_PRIORITY is not a list in policy.rs"
    assert budget["priority"] == priority


def test_the_budget_is_shown_biting_and_a_refused_file_is_still_a_measured_file(report: dict) -> None:
    """The cap would be a rumour if nothing had ever seen it refuse."""
    demonstration = row(report, "ruby")["scan_budget"]["demonstration"]
    assert demonstration["refused"] > 0, "the budget never bit, so nothing proves the cap exists"
    assert demonstration["refused_paths"], "a refusal with no name is not a demonstration"
    assert demonstration["every_refused_file_still_measured"], (
        "a refused file with no file-level numbers is a hole in the graph wearing a bound's clothes")


def test_rubys_throughput_exemption_is_bought_with_the_arithmetic_it_claims(report: dict, thresholds: dict) -> None:
    """The ticket's most tempting shortcut: let the slow grammar under the floor with a sentence.

    The exemption in `thresholds.json` says Ruby may miss the MiB/s floor *because* the byte cap
    bounds what a cold run can spend on it. That is an arithmetic claim, so this test checks the
    arithmetic — miss it and the exemption is a number with an excuse attached.
    """
    entry = row(report, "ruby")
    exempt = thresholds["throughput"]["exempt"]["ruby"]
    assert exempt["via"] == "scan_budget"
    cost = entry["scan_budget"]["cost"]
    assert cost is not None, "the exemption names the scan budget and the report has no cost block"
    assert cost["measured_mib_per_second"] > 0
    implied = cost["implied_ms_at_the_cap"]
    assert implied <= cost["graph_budget_ms"], (
        f"the byte cap costs {implied:.0f} ms against a {cost['graph_budget_ms']:.0f} ms cold-graph "
        "budget — the exemption is not free, and this is the price")
    assert cost["inside_graph_budget"]
    assert entry["throughput_check"]["basis"] == "scan_budget"
    assert entry["throughput_check"]["exemption_satisfied"]


# ---------------------------------------------------------------------------
# 4. Java: the same-package rate is a measurement with a budget, and the scope is worth something
# ---------------------------------------------------------------------------


def test_the_java_same_package_scan_is_measured_at_the_size_the_criterion_asks_for(report: dict) -> None:
    """`e0.12`: "re-measure on a corpus of at least 1 000 files, publish the rate, state the budget."
    Every clause is a separate assertion, because the easy way to satisfy the sentence is to publish a
    rate off nine files and call the budget a paragraph."""
    same = row(report, "java")["same_package"]
    assert same["corpus"]["files"] >= 1_000, (
        f"the same-package scan was re-measured on {same['corpus']['files']} files")
    assert same["budget"]["max_false_positive_rate"] is not None
    assert same["budget"]["min_recall"] is not None
    assert same["budget"]["min_files"] >= 1_000
    assert same["budget_met"], f"the published rate misses its own budget: {same['scoped']}"


def test_the_scoped_scan_clears_the_budget_and_the_wide_one_does_not(report: dict) -> None:
    """The scoping decision has to stay a measurement. If the wide column ever matches the scoped one,
    the scope is doing nothing and someone should delete it and say why."""
    same = row(report, "java")["same_package"]
    scoped, wide = same["scoped"], same["wide"]
    budget = same["budget"]
    assert scoped["false_positive_rate"] <= budget["max_false_positive_rate"]
    assert scoped["recall"] >= budget["min_recall"]
    assert wide["false_positive_rate"] > scoped["false_positive_rate"] * 10, (
        "the wide scan is no worse than the scoped one; the scoping is not earning its keep")
    assert wide["recall"] >= scoped["recall"], "the wide scan lost recall it was supposed to buy"
    unbound = same["counterfactual"]["without_the_bound_name_rule"]
    assert unbound["over_budget"], (
        "the bound-name rule is stated as load-bearing; a scan without it still clears the budget, so "
        "either the rule goes or the sentence does")
    assert unbound["false_positive_rate"] > scoped["false_positive_rate"], (
        "switching the bound-name rule off did not change the rate, so it is not doing what the doc says")
    assert unbound["added_edges"] >= same["corpus"]["type_variable_baits"], (
        "the corpus plants type-variable baits and the scan without the rule did not claim them")


def test_the_machine_authored_corpus_is_named_as_machine_authored(report: dict) -> None:
    """Nobody hand-annotates 1 200 Java files, and a published rate whose provenance is vague is
    worse than no rate: it gets quoted."""
    same = row(report, "java")["same_package"]
    assert "java_corpus.rs" in same["corpus"]["authored_by"]
    assert JAVA_GENERATOR.exists(), "the generator the report names does not exist"
    assert "machine-authored" in same["corpus"]["mean"]
    assert same["corpus"]["type_names_declared_in_more_than_one_package"] > 0, (
        "no colliding type names means the corpus never tested the thing the scan gets wrong")
    assert same["corpus"]["shadowed_imports"] > 0, (
        "no same-name imports means the shadowing rule was never exercised")


# ---------------------------------------------------------------------------
# 5. Ruby's surface question, stated as a question the extractor can answer
# ---------------------------------------------------------------------------


def test_ruby_says_its_export_list_is_not_a_public_api_and_offers_the_other_question(report: dict) -> None:
    """s1 finding 3. A Ruby `exports` list printed under "public symbols" is a lie about the
    language, so the report has to say which question Ruby answers instead."""
    ruby = row(report, "ruby")
    surface = ruby["surface"]
    assert surface["may_claim_public_symbols"] is False
    assert surface["loads"]["sentence"], "the replacement question has to have an answer"
    assert surface["loads"]["files"], f"{surface['loads']}"
    for language in [entry for entry in CORE + TAIL if entry != "ruby"]:
        assert row(report, language)["surface"]["may_claim_public_symbols"] is True, (
            f"{language}: its export rule is a real access rule, so the flag must not be a shrug")


# ---------------------------------------------------------------------------
# 6. the knobs, as configuration (ADR-004 §5.1: a key lives in the config component or it does not)
# ---------------------------------------------------------------------------


def test_the_scan_policy_keys_exist_and_are_documented() -> None:
    from recommender.config import schema as S  # noqa: PLC0415

    for key in SCAN_KEYS:
        assert key in S.KEYS, f"{key} is not declared in the config schema"
        assert key in CONFIG_DOC.read_text(), f"{key} is not documented in docs/concepts/config.md"
        declared = S.KEYS[key]
        assert declared.ticket == "e3.1b", f"{key} does not say which ticket needs it"
        assert declared.analyzer == "structure.extract", f"{key} does not name the analyzer that reads it"
    assert "extract.ruby.priority" in S.KEYS
    assert "extract.ruby.priority" in CONFIG_DOC.read_text()


def test_the_scan_policy_defaults_match_the_rust_policy() -> None:
    from recommender.config import schema as S  # noqa: PLC0415

    for key, constant in SCAN_KEYS.items():
        rust = rust_constant(constant)
        declared = S.KEYS[key].default
        assert str(declared) == rust or declared == int(rust) or str(declared) == rust, (
            f"{key}: the schema says {declared!r}, policy.rs says {rust!r}")
    from recommender.config import schema as S  # noqa: PLC0415

    assert S.KEYS["extract.ruby.priority"].default == ruby_priority()
    assert set(S.KEYS["extract.java.same_package_scan"].enum) == {
        "imports_plus_package",
        "all_type_references",
    }, "the schema must offer exactly the scopes the extractor implements"


# ---------------------------------------------------------------------------
# 7. the verdicts, re-derived (the same rule as `e3.1`, plus the two new exemption kinds)
# ---------------------------------------------------------------------------


def floors(thresholds: dict, language: str) -> dict:
    rules = thresholds["relations"]
    return {**rules["default"], **rules.get(language, {})}


def recompute_verdict(entry: dict, thresholds: dict, language: str) -> str:
    rules = floors(thresholds, language)
    worst = "pass"
    for relation in RELATIONS:
        score = entry["relations"][relation]
        rule = rules.get(relation, {})
        if score["recall"] >= rule.get("recall", 1.0) and score["precision"] >= rule.get("precision", 1.0):
            continue
        worst = "fail" if rule.get("hard") else "conditions"
        break
    mechanisms = entry["mechanisms"]
    if mechanisms["synthetic_eligible"]["missing"] or mechanisms["wildcard_edges"]["mislabelled_as_resolved"]:
        worst = "fail"
    if mechanisms["synthetic_eligible"]["scopes_that_assert_emptiness"]:
        worst = "fail"
    if not mechanisms["wildcard_edges"]["who_depends_on"]["says_unknown"]:
        worst = "fail"
    imperfect = any(entry["relations"][relation]["missed"] or entry["relations"][relation]["false_positive"]
                    for relation in RELATIONS)
    if worst == "pass" and (imperfect or entry["blind_spots"]):
        worst = "conditions"
    if worst == "pass" and not entry.get("throughput_check", {}).get("ok", True):
        worst = "conditions"
    return worst


@pytest.mark.parametrize("language", TAIL)
def test_the_reported_verdict_is_the_one_the_thresholds_produce(report: dict, thresholds: dict,
                                                              language: str) -> None:
    entry = row(report, language)
    expected = recompute_verdict(entry, thresholds, language)
    assert entry["verdict"] == expected, f"{language}: report says {entry['verdict']}, thresholds say {expected}"
    assert report["verdicts"][language] == expected


@pytest.mark.parametrize("language", TAIL)
def test_a_throughput_exemption_is_granted_only_against_a_corpus_that_clears_the_floor(
    report: dict, thresholds: dict, language: str
) -> None:
    """The tail's two *kinds* of throughput exemption, both checked.

    `scan_budget` (Ruby) is arithmetic about a bound. `repository_shape` (Go, Java) is a statement
    that the replica mis-measures a package-granular language — which is only ever true if the
    repository-shaped corpus clears the floor. An exemption that could be satisfied by the corpus it
    is exempting is not an exemption, it is the floor removed.
    """
    entry = row(report, language)
    check = entry["throughput_check"]
    assert check["ok"], f"{language}: throughput check failed and nothing explains it"
    basis = check["basis"]
    if basis == "replica":
        return
    exempt = thresholds["throughput"]["exempt"].get(language, {})
    assert basis == exempt.get("via"), f"{language}: basis {basis!r} is not what thresholds say"
    assert check["exemption_satisfied"]
    if exempt.get("via") == "repository_shape":
        shape = check["repository_shape"]
        assert shape is not None, "a repository_shape exemption with no repository-shaped corpus"
        floor = thresholds["throughput"]["files_per_second_per_core_floor"]
        assert shape["files_per_second_per_core"] >= floor, (
            f"{language}: {shape['files_per_second_per_core']} files/s in the shape the exemption "
            f"names as the honest one")
        assert shape["files"] >= 1_000, "the 'repository-shaped' corpus is not repository-sized"


# ---------------------------------------------------------------------------
# 8. the wiring, so none of the above is a one-off
# ---------------------------------------------------------------------------


def test_the_tail_test_and_harness_are_feature_gated_on_all_six_grammars() -> None:
    """A default `cargo test` must not pay for six C builds (ADR-002 §6.5), and the tail has nothing
    true to say at any smaller feature set."""
    text = CARGO_TOML.read_text()
    for target in ("structure_tail", "structure_harness"):
        block = text.split(f'name = "{target}"', 1)
        assert len(block) == 2, f"{target} is not declared as a Cargo target"
        following = block[1].split("[[", 1)[0]
        assert "langs-all" in following, f"{target} is not gated on langs-all"


def test_ci_re_measures_the_whole_report_with_all_six_grammars() -> None:
    text = WORKFLOW.read_text()
    assert "structure_tail" in text, "CI does not run the tail's contract test"
    # YAML folds a long `run:` across lines, so the step has to be found as a block rather than as a
    # line — a check that reads lines silently passes on a workflow it cannot read.
    steps = [block for block in re.split(r"- name:", text) if "structure_harness" in block]
    assert steps, "CI does not run the structure harness at all"
    assert any("--check" in block and "langs-all" in block for block in steps), (
        "CI re-checks the report with fewer grammars than the report contains")


def test_the_schema_version_bump_explains_what_a_reader_loses(mod_rs: str = MOD_RS.read_text()) -> None:
    """A stored shape whose version went up without a sentence about *why* is a migration nobody can
    write: the number is useless without the difference."""
    assert re.search(r"SCHEMA_VERSION: u32 = 2", mod_rs), "the structure schema version is not 2"
    block = mod_rs.split("SCHEMA_VERSION", 2)[0]
    assert "e3.1b" in block and "granularity" in block, (
        "the version bump's doc comment has to name what the new field lets a reader ask")
    golden = json.loads(GOLDEN.read_text())
    assert golden["schema_version"] == 2


def test_the_doc_records_the_tail_and_what_it_makes_a_claim_about(report: dict) -> None:
    """The doc is where `e3.2` and `e3.7` read what they may build on, so the tail has to be written
    there in the same breath as the numbers."""
    text = DOC.read_text()
    assert report["ticket"]["tail"]["id"] in text
    assert "e3.1b" in text
    for language in TAIL:
        assert language in text, f"docs/concepts/structure-extraction.md does not mention {language}"
    for phrase in ("package-granular", "scan budget", "same-package"):
        assert phrase in text, f"the doc does not state the {phrase} rule"
    for key in SCAN_KEYS:
        assert key in text, f"the doc does not name the {key} knob"
    assert "same_package" in text
