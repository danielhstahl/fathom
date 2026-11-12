"""`e8.4` (`reco-l9bz.4`) as tests: the index-time complexity pass, its ceilings, and its cost gate.

This file is the executable form of [docs/concepts/complexity-ceilings.md](../docs/concepts/complexity-ceilings.md)
and of ADR-007 §9.7. It does not re-run the pass — `crates/fathom-analyze/tests/complexity_fill.rs` does that
against real repositories, and a second scorer is how a measurement goes quiet. What it re-derives is the
parts a decision spread across five files can drift into without anyone noticing:

1. **The three ceilings cannot be limits only the library knows about.** Each has to be a declared config key,
   documented in the reference, defaulting to what `policy.rs` compiles in (ADR-004 §5.1), and the concept page
   has to print the same numbers in its table.
2. **The pass cannot invent a reason code.** ADR-007 §7.1's table is the vocabulary; every code the fill can
   write has to appear in it. The store's own spellings of `scan_budget` and `too_large` — the two codes that
   leave a row *owed* instead of closed — have to be those exact strings, and §9.7's sentence about them has
   to name the same two.
3. **The cost gate cannot be neutered.** The committed report has to declare an allowance, sit inside its own
   allowance, and the tool that judges it has to still complain when a measurement is outside it. A checker
   that cannot fail is a paragraph with an exit code.
4. **The honest-degradation plumbing cannot be deleted behind a tidy refactor.** The progress tick carries a
   note, the row prints it, the doctor's remedy names a key a person can raise, and the milliseconds the pass
   reports are the same accumulated quantity its ceiling compared against.

    PYTHONPATH=src python3 -m pytest -q tests/test_complexity_fill_contract.py
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tools"))

POLICY = ROOT / "crates" / "fathom-analyze" / "src" / "structure" / "policy.rs"
FILL = ROOT / "crates" / "fathom-analyze" / "src" / "fill.rs"
STORE_COMPLEXITY = ROOT / "crates" / "fathom-store" / "src" / "complexity.rs"
INDEX = ROOT / "crates" / "fathom-analyze" / "src" / "index.rs"
GUIDE = ROOT / "crates" / "fathom-cli" / "src" / "guide.rs"
STATE = ROOT / "crates" / "fathom-cli" / "src" / "state.rs"
ADR = ROOT / "docs" / "adr" / "0007-complexity.md"
CONCEPT = ROOT / "docs" / "concepts" / "complexity-ceilings.md"
CONFIG_DOC = ROOT / "docs" / "concepts" / "config.md"
INDEX_DOC = ROOT / "docs" / "index.md"
WORKFLOW = ROOT / ".github" / "workflows" / "complexity.yml"
REPORT = ROOT / "benchmarks" / "reports" / "e8-4-index-time-complexity.json"

#: Both committed cost measurements. The monorepo one is the only place the byte ceiling is observed
#: actually spending on a corpus nobody engineered, so it is as much a contract as the local one — and it is
#: the one a reader of the concept page's cost table has to be able to check.
REPORTS = (
    "e8-4-index-time-complexity.json",
    "e8-4-index-time-complexity-monorepo.json",
)

#: The three ceilings, and the constant in `policy.rs` that ships each one's default.
CEILINGS = {
    "extract.complexity.max_file_bytes": "DEFAULT_COMPLEXITY_MAX_FILE_BYTES",
    "extract.complexity.max_run_bytes": "DEFAULT_COMPLEXITY_MAX_RUN_BYTES",
    "extract.complexity.max_run_ms": "DEFAULT_COMPLEXITY_MAX_RUN_MS",
}


def rust_const(name: str) -> int:
    """One `pub const NAME: … = 1_2_3;` out of `policy.rs`, underscores and all."""
    match = re.search(rf"pub const {name}: \w+ = ([\d_]+);", POLICY.read_text())
    assert match, f"{name} is not a compiled-in default in policy.rs"
    return int(match.group(1).replace("_", ""))


def report() -> dict:
    assert REPORT.exists(), (
        f"{REPORT.relative_to(ROOT)} is the committed before/after cost measurement `e8.4` owes; write it "
        "with `python3 tools/complexity_cost.py --repo REPO --out <that path>`")
    return json.loads(REPORT.read_text())


def committed(name: str) -> dict:
    path = ROOT / "benchmarks" / "reports" / name
    assert path.exists(), f"{path.relative_to(ROOT)} is missing; see `tools/complexity_cost.py --help`"
    return json.loads(path.read_text())


def adr_section(title: str) -> str:
    """One section of ADR-007, by its heading prefix."""
    parts = ADR.read_text().split(title, 1)
    assert len(parts) == 2, f"ADR-007 no longer has a section starting {title!r} to check against"
    return parts[1].split("\n## ", 1)[0]


# ---------------------------------------------------------------------------
# 1. the ceilings are configuration, in three places that have to agree
# ---------------------------------------------------------------------------


def test_the_three_ceilings_are_declared_keys() -> None:
    from recommender.config import schema as S  # noqa: PLC0415

    for key in CEILINGS:
        assert key in S.KEYS, f"{key} is not declared in the config schema"
        assert key in CONFIG_DOC.read_text(), f"{key} is not documented in docs/concepts/config.md"
        declared = S.KEYS[key]
        assert declared.ticket == "e8.4", f"{key} does not say which ticket needs it"
        assert declared.kind == "integer", f"{key} is not a count of something"


def test_the_declared_defaults_are_the_compiled_in_ones() -> None:
    """The library's default and the documented default cannot be two numbers (ADR-004 §5.1)."""
    from recommender.config import schema as S  # noqa: PLC0415

    for key, constant in CEILINGS.items():
        declared = S.KEYS[key].default
        shipped = rust_const(constant)
        assert declared == shipped, (
            f"{key}: the schema ships {declared} and policy.rs ships {shipped}; only one of them is what "
            "a run without a `fathom.toml` uses, and the other is a document that has started lying")


def test_the_concept_page_prints_the_shipped_defaults() -> None:
    """The page an operator reads cannot document numbers the binary does not have."""
    text = CONCEPT.read_text()
    for key, constant in CEILINGS.items():
        shipped = rust_const(constant)
        assert key in text, f"{key} is not in the concept page"
        spaced = f"{shipped:,}".replace(",", " ")
        assert f"| {shipped} |" in text or f"| {spaced} |" in text, (
            f"{key}'s default {shipped} is not the number the concept page's table prints — the table is "
            "the only place a reader decides whether to raise it")


def test_the_concept_page_is_reachable_from_the_front_door() -> None:
    assert CONCEPT.exists(), "the decision needs a page of its own"
    assert "concepts/complexity-ceilings.md" in INDEX_DOC.read_text(), (
        "docs/index.md does not link the page, so nobody finds it")


# ---------------------------------------------------------------------------
# 2. the reason vocabulary: ADR-007's, not a new one
# ---------------------------------------------------------------------------


def adr_reason_codes() -> set[str]:
    """The first column of ADR-007 §7.1's table — the normative reason vocabulary."""
    codes: set[str] = set()
    for line in adr_section("### 7.1").splitlines():
        cells = [cell.strip() for cell in line.strip().split("|")]
        if len(cells) > 2 and len(cells[1]) > 2 and cells[1].startswith("`") and cells[1].endswith("`"):
            codes.add(cells[1][1:-1])
    return codes


def test_the_fill_writes_no_reason_code_the_adr_does_not_allow() -> None:
    written = set(re.findall(r'reason:\s*"([a-z_]+)"', FILL.read_text()))
    assert written, "the fill no longer writes any reason codes; what filled the columns instead?"
    allowed = adr_reason_codes()
    assert len(allowed) > 8, f"ADR-007 §7.1's table could not be parsed (found {sorted(allowed)})"
    invented = written - allowed
    assert not invented, (
        f"the fill writes {sorted(invented)}, which ADR-007 §7.1 does not list. The ADR's table is the "
        "vocabulary: extend it there first, with the remedy sentence that makes it useful, or use a code "
        "already in it")


def test_the_two_reopenable_codes_are_the_store_and_the_adr_agreeing() -> None:
    """`scan_budget` and `too_large` are the codes that leave a row owed; the store names them, §9.7 promises
    those two and no others, and §7.1 lists both."""
    store = STORE_COMPLEXITY.read_text()
    assert 'pub const BUDGET_REASON: &str = "scan_budget";' in store
    assert 'pub const SIZE_REASON: &str = "too_large";' in store
    assert {"scan_budget", "too_large"} <= adr_reason_codes(), (
        "both codes the owed set re-opens must be §7.1's, or the store is re-opening a code that has no "
        "documented remedy")
    assert "`scan_budget` or `too_large`" in adr_section("### 9.7"), (
        "§9.7 is the promise that a ceiling's refusal stays owed. If the two names changed, the sentence and "
        "`Store::complexity_owed` have to change in the same commit")


# ---------------------------------------------------------------------------
# 3. the cost report, and the gate that judges it
# ---------------------------------------------------------------------------


def test_the_committed_report_carries_both_arms_and_a_declared_gate() -> None:
    data = report()
    for arm in ("declined", "shipped"):
        assert arm in data["arms"], f"the report is missing the `{arm}` arm"
        entry = data["arms"][arm]
        assert entry["wall_ms_median"] > 0, f"{arm}: no wall clock measured"
        assert entry["peak_rss_mib"] > 0, f"{arm}: no peak RSS sampled"
        assert "pass" in entry and entry["pass"], (
            f"{arm}: no ledger from the pass, so the cost has no explanation attached to it")
    gate = data["gate"]
    assert gate["max_index_time_over_pct"] > 0 and gate["max_peak_rss_over_mib"] > 0, (
        "a gate of zero fails everything and a gate of infinity gates nothing")
    assert data["ticket"] == "reco-l9bz.4", "the report must name the bead it closes"
    assert data["repository"]["commits"], "the report must say how big the measured corpus was"


def test_the_committed_reports_audit_clean() -> None:
    """A report whose headline does not follow from its own numbers is worse than no report."""
    import complexity_cost as C  # noqa: PLC0415

    for name in REPORTS:
        problems = C.audit(json.loads((ROOT / "benchmarks" / "reports" / name).read_text()))
        assert not problems, f"{name}: {problems}"


def test_the_gate_still_complains_when_a_measurement_is_over_it() -> None:
    """The checker is tested against synthetic failures, because a gate that cannot fail is not a gate."""
    import complexity_cost as C  # noqa: PLC0415

    committed = report()

    broken = json.loads(json.dumps(committed))
    broken["overhead"]["index_time_pct"] = committed["gate"]["max_index_time_over_pct"] * 10
    broken["overhead"]["peak_rss_mib"] = committed["gate"]["max_peak_rss_over_mib"] * 10
    problems = C.audit(broken)
    assert any("index time" in problem for problem in problems), problems
    assert any("peak RSS" in problem for problem in problems), problems

    broken = json.loads(json.dumps(committed))
    broken["overhead"]["index_time_ms"] += 5_000.0
    assert any("index_time_ms" in problem for problem in C.audit(broken)), (
        "the headline milliseconds no longer agree with the arm medians they were derived from")


def test_the_auditor_notices_an_arm_that_did_its_job_wrong() -> None:
    """A `declined` arm that read something, or a `shipped` arm that read nothing, measured nothing."""
    import complexity_cost as C  # noqa: PLC0415

    committed = report()

    broken = json.loads(json.dumps(committed))
    broken["arms"]["declined"]["pass"]["bytes_read"] = 4_096
    assert any("read 4,096 bytes" in problem for problem in C.audit(broken)), (
        "a `declined` arm that read content means the baseline is not a baseline")

    broken = json.loads(json.dumps(committed))
    broken["arms"]["declined"]["pass"]["ceilings"]["max_run_ms"] = 60_000
    assert any("was not declined" in problem for problem in C.audit(broken)), (
        "the `declined` arm has to carry the ceiling that declined it")

    broken = json.loads(json.dumps(committed))
    broken["arms"]["shipped"]["proved"] = False
    assert any("read nothing" in problem for problem in C.audit(broken)), (
        "a `shipped` arm that read no content means the gate compared two refusals")

    assert not C.audit(committed), "the auditor rejects the report it was handed as good"


def test_the_checker_notices_a_report_about_a_different_experiment() -> None:
    """`--check` cannot re-clone a corpus or re-time a busy machine, so what it *does* guarantee is that the
    committed report describes the experiment it is being compared against."""
    import complexity_cost as C  # noqa: PLC0415

    committed = report()

    moved = json.loads(json.dumps(committed))
    moved["repository"]["name"] = "something-else"
    assert any("something-else" in problem for problem in C.check(moved, committed)), (
        "the checker accepted a report about a different repository")

    other_corpus = json.loads(json.dumps(committed))
    other_corpus["repository"]["corpus"] = "a corpus nobody named"
    assert any("corpus" in problem for problem in C.check(other_corpus, committed)), (
        "a corpus that changed underneath the report changed the benchmark without saying so")

    retuned = json.loads(json.dumps(committed))
    retuned["arms"]["shipped"]["config"] = "[extract.complexity]\nmax_run_ms = 5\n"
    assert any("configuration changed" in problem for problem in C.check(committed, retuned)), (
        "the arms are the experiment; comparing across different arms is not a gate")

    assert not C.check(committed, committed), "the checker rejects the report it was handed as matching"


def test_the_checker_notices_a_different_repository() -> None:
    """A gate that measures one corpus against another's declaration is measuring nothing."""
    import complexity_cost as C  # noqa: PLC0415

    committed = report()
    other = json.loads(json.dumps(committed))
    other["repository"]["name"] = "something-else"
    assert any("committed report measured" in problem for problem in C.check(other, committed))

    other = json.loads(json.dumps(committed))
    other["repository"]["commits"] = int((committed["repository"]["commits"] or 1) * 4)
    assert any("corpus moved" in problem for problem in C.check(other, committed))


def test_the_tool_never_touches_the_network() -> None:
    """A benchmark that re-clones has a corpus that changes on its own — `tools/perf_realrepo.py`'s rule,
    and the reason the committed report names the clone it came from instead of fetching one."""
    text = (ROOT / "tools" / "complexity_cost.py").read_text()
    body = "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))
    for token in ("git clone", "curl", "wget", "urllib", "requests", "https://"):
        assert token not in body.lower(), (
            f"complexity_cost.py reaches for {token}; it must measure a corpus somebody placed")


# ---------------------------------------------------------------------------
# 4. the CI wiring, so the gate runs somewhere other than this machine
# ---------------------------------------------------------------------------


def test_the_workflow_runs_the_fill_tests_and_the_cost_gate() -> None:
    workflow = WORKFLOW.read_text()
    assert "--test complexity_fill" in workflow, (
        "the ceilings' test target is not wired into CI, so it only ever runs on the machine that wrote it")
    assert "complexity_cost.py" in workflow
    assert "--corpus-note" in workflow, (
        "`--check` compares the corpus the committed report names against the corpus this run was told; a "
        "workflow that measures this repository without naming it that way fails its own gate")
    assert "--report benchmarks/reports/e8-4-index-time-complexity.json --check" in workflow, (
        "the cost gate is not wired into CI, so the declared allowance is a paragraph rather than a gate")
    for report in REPORTS:
        assert report in workflow, (
            f"{report} is committed but no path filter re-runs the gate when it moves")


def test_the_fill_test_target_names_the_features_it_needs() -> None:
    manifest = (ROOT / "crates" / "fathom-analyze" / "Cargo.toml").read_text()
    blocks = [part for part in manifest.split("[[test]]")[1:]]
    fill = [part for part in blocks if 'name = "complexity_fill"' in part]
    assert fill, "tests/complexity_fill.rs has no [[test]] entry, so a default build cannot compile it"
    assert "sqlite-bundled" in fill[0] and "lang-rust" in fill[0], (
        "the target must name the features it needs, or a `--no-default-features` build fails to compile "
        "it instead of skipping it, which ADR-002 §6.1 forbids")


# ---------------------------------------------------------------------------
# 5. the honest-degradation plumbing
# ---------------------------------------------------------------------------


def test_the_progress_tick_carries_a_note_and_the_row_prints_it() -> None:
    index = INDEX.read_text()
    assert "pub note: Option<&'static str>" in index, (
        "the tick no longer carries a note, so a phase that finished every commit it was asked to analyse "
        "and still declined most of the work cannot say so on the row a watcher operator reads")
    assert "note:" in index, "the analyses phase must still set the note when a ceiling is spent"
    assert "note" in GUIDE.read_text(), "the progress row no longer prints the note it is handed"


def test_the_doctor_remedy_names_a_key_a_person_can_raise() -> None:
    state = STATE.read_text()
    for key in ("extract.complexity.max_run_bytes", "extract.complexity.max_run_ms"):
        assert key in state, f"the doctor's remedy has to name `{key}`; `raise the budget` is not a remedy"
    assert "stays owed" in state, (
        "the finding has to say why re-indexing is the fix: a row refused by a ceiling stays owed")
    assert "scan_budget" in state, "the finding is keyed on the one code whose remedy is a person"


def test_the_pass_reports_the_quantity_its_ceiling_bounds() -> None:
    """`max_run_ms` bounds this pass's own accumulated clock, so `ms` must not be a deadline's elapsed time."""
    fill = FILL.read_text()
    assert "self.spent += began.elapsed()" in fill, (
        "the pass must accumulate its own work: a clock read from the start of the analyses phase charges "
        "the complexity budget for every other analyzer that ran in it, and then the reported number is not "
        "the number that was bounded")
    assert "self.spent >= Duration::from_millis(self.policy.complexity_max_run_ms)" in fill, (
        "the time ceiling must compare the accumulated quantity, not a deadline")


def test_the_index_json_schema_carries_the_ceilings_and_the_spend() -> None:
    """`fathom index --json` is a published contract with `additionalProperties: false`, so a field the
    degradation story leans on is only part of the contract once the schema says so. A consumer deciding
    "was this index limited, and by what, and would raising it help" reads exactly these keys — and the Rust
    schema test is the one that would go red first, which is the point of pinning the shape in both places.
    """
    schema = json.loads((ROOT / "schemas" / "index.schema.json").read_text())
    fill = schema["properties"]["complexity_fill"]
    assert fill["additionalProperties"] is False, "an open shape makes the contract mean nothing"
    for key in (
        "rows",
        "measured",
        "refused",
        "bytes_read",
        "base_reused",
        "ms",
        "written",
        "missing",
        "declined",
        "ceilings",
    ):
        assert key in fill["properties"], f"complexity_fill.{key} is not declared"
        assert key in fill["required"], f"complexity_fill.{key} is optional, so a consumer cannot read it"
    ceilings = fill["properties"]["ceilings"]
    assert set(ceilings["properties"]) == {
        "max_file_bytes",
        "max_run_bytes",
        "max_run_ms",
    }, "the ceilings in force are exactly the three keys the pass compares against"
    # And the spend is a config key, not a prose sentence: a consumer gates on the name.
    declined = fill["properties"]["declined"]["type"]
    assert "string" in declined and "null" in declined, "no ceiling spent must be distinguishable from a key"


def test_the_run_report_prints_the_ceilings_it_obeyed() -> None:
    """Docs promise the line says what bounded the run. A promise in a doc page with no line behind it is how
    documentation rots, so the sentence and the `say!` are checked together."""
    page = (ROOT / "docs" / "concepts" / "complexity-ceilings.md").read_text()
    verbs = (ROOT / "crates" / "fathom-cli" / "src" / "verbs.rs").read_text()
    assert "ceilings" in page, "the page does not say the ceilings are printed"
    assert "ceilings {}" in verbs, "the run report no longer prints the ceilings in force"
    assert "as_text()" in verbs, "…and not through the one formatter that names the three keys"


def test_the_prose_quotes_the_committed_reports() -> None:
    """The cost story is told in three places — the ADR, the concept page, the changelog — and every number in
    them is quoted from one of the two committed reports. A doc that quotes a stale measurement is worse than
    a doc that quotes none, because nobody re-checks a number that looks sourced."""
    adr = ADR.read_text()
    concept = CONCEPT.read_text()
    changelog = (ROOT / "CHANGELOG.md").read_text()
    for name in REPORTS:
        data = committed(name)
        over = data["overhead"]
        pct = f"{over['index_time_pct']} %"
        where = " ".join((adr, concept, changelog))
        assert pct in where, (
            f"{name}: the reports say +{pct} of index time and nothing in the prose quotes it; the table in "
            "docs/concepts/complexity-ceilings.md is where a reader expects the number")
        assert f"{over['peak_rss_mib']} MiB" in where or f"{abs(over['peak_rss_mib'])} MiB" in where, (
            f"{name}: the peak RSS delta the report records is quoted nowhere")
