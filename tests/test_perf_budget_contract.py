"""The perf budget gate, kept honest (`e0.11`, `reco-pef`).

ADR-004 §11 publishes 172 numbers and a block of gate rules, and says `e0.11` asserts on them. This
file is the check that the assertion is real:

* `benchmarks/BUDGETS.md` lists **every** budget with its numeric threshold, and agrees with
  `benchmarks/budgets.json` rather than with someone's memory of it;
* the gate fails a deliberate regression (a real `sleep` injected in front of every measured process),
  which is the acceptance criterion — a gate nobody has seen fail is decoration;
* the arithmetic that cannot be tested by one run is tested directly: the threshold comes from the
  budget file, RSS budgets are never scaled by the runner factor while wall clocks are, a recorded
  breach is bounded rather than waived, and an improvement past the threshold demands a re-bless;
* RSS is measured, not assumed: the blessed baseline carries resident-set numbers from `VmHWM`, marked
  unscaled, beside the timings they came from;
* the plumbing is wired: `make bench` exists, the CI job publishes the trend, the trend is gitignored
  because it is an artefact, and the gate speaks `fathom-core`'s exit code 11.

Run: python3 -m pytest tests/test_perf_budget_contract.py -q
The one slow test — the real injected regression — needs a built binary; `$FATHOM_BIN`,
`target/release/fathom` or `target/debug/fathom`, and skips with none, because a check that cannot find
a binary is not evidence about one.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import perf_gate as G  # noqa: E402

BUDGETS_MD = ROOT / "benchmarks" / "BUDGETS.md"
BUDGETS_JSON = ROOT / "benchmarks" / "budgets.json"
BASELINE = ROOT / "benchmarks" / "baselines" / "perf-gate.json"
WORKFLOW = ROOT / ".github" / "workflows" / "perf.yml"
MAKEFILE = ROOT / "Makefile"

STATISTICS = ("p95", "p50", "peak", "cap", "ceiling", "hard limit", "count")

# | `cold.index.ms` | ms | p95 | 1,500 | 20,000 | 75,000 | 240,000 | this harness | … |
#
# The statistic column may carry its denominator with it — `p95 per commit`, `p50 per file row`, the
# rate budgets of ADR-004 §11.9 — so the keyword is matched at the head of the cell and the rest of the
# cell is skipped rather than required to be nothing.
BUDGET_ROW = re.compile(
    r"^\|\s*`([a-z0-9_.]+)`\s*\|([^|]*)\|\s*(" + "|".join(STATISTICS) + r")[^|]*\|"
    r"([^|]*)\|([^|]*)\|([^|]*)\|([^|]*)\|")


def _budgets() -> tuple[dict, dict]:
    return G.load_budgets()


def _binary() -> Path:
    for candidate in (os.environ.get("FATHOM_BIN"), str(ROOT / "target" / "release" / "fathom"),
                      str(ROOT / "target" / "debug" / "fathom")):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    pytest.skip("no fathom binary built; set FATHOM_BIN or run `make bench` once")


# ---------------------------------------------------------------------------------------------
# AC 1 — benchmarks/BUDGETS.md lists every budget with its numeric threshold
# ---------------------------------------------------------------------------------------------
def test_budgets_md_lists_every_budget_with_its_numeric_threshold():
    budgets, _gates = _budgets()
    text = BUDGETS_MD.read_text()
    rows = {}
    for line in text.splitlines():
        match = BUDGET_ROW.match(line)
        if match:
            rows[match.group(1)] = [cell.strip().replace(",", "") for cell in match.groups()[3:7]]
    families = sorted({row["family"] for row in budgets.values()})
    assert sorted(rows) == families, (
        "BUDGETS.md must carry one row per budget family; missing: "
        f"{sorted(set(families) - set(rows))}, extra: {sorted(set(rows) - set(families))}")
    assert len(rows) >= 39, "the budget page went missing rows, not gained them"
    for family, cells in rows.items():
        for size_class, cell in zip(("small", "medium", "large", "xlarge"), cells):
            budget_id = f"{family}.{size_class}"
            assert budget_id in budgets, f"{budget_id} is not a budget id in budgets.json"
            published = float(budgets[budget_id]["budget"])
            if cell == "—":
                continue
            assert float(cell) == published, (
                f"{budget_id}: BUDGETS.md says {cell}, budgets.json says {published}. "
                "Run `python3 tools/gen_budgets.py write`.")
    # Every id is a family-class pair, so the number of numbers is the number of rows times four.
    assert f"**{len(budgets)} numbers" in text, (
        "the page must state how many numbers it carries, and that count must be the file's")


def test_budgets_page_and_generator_do_not_drift():
    """`gen_budgets.py check` is what keeps the prose and the numbers married (`e0.7` runs it too)."""
    result = subprocess.run([sys.executable, "tools/gen_budgets.py", "check"], cwd=ROOT,
                            capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr


def test_every_budget_family_declares_how_it_is_measured():
    budgets, _ = _budgets()
    coverage = G.coverage()
    for family in sorted({row["family"] for row in budgets.values()}):
        assert family in coverage, f"{family} has no coverage entry in tools/perf_gate.py"
        by, how = coverage[family]
        assert by and how, f"{family}'s coverage row is empty"
        if by == "not measured yet":
            assert re.search(r"`(e|s|reco)[\w.]*`", how), (
                f"{family} is unmeasured, which is allowed only with a named reason: {how!r}")
    measured = [family for family, (by, _how) in coverage.items()
                if by in ("this harness", "criterion", "both")]
    assert len(measured) >= 12, (
        f"only {len(measured)} budget families are measured by the gate; it is supposed to be the gate")


# ---------------------------------------------------------------------------------------------
# The baseline: blessed, complete, and the thing the regression gate compares against
# ---------------------------------------------------------------------------------------------
def test_the_baseline_blesses_every_measurement_the_gate_runs():
    baseline = json.loads(BASELINE.read_text())
    blessed = baseline["measurements"]
    assert blessed, "the baseline has no measurements"
    assert baseline["calibration"]["reference_ms"] > 0, "no calibration reference: no runner factor"
    for plan in G.measurement_plan() + G.criterion_plan():
        assert plan["budget"] is None or plan["budget"] in _budgets()[0], (
            f"{plan['id']} gates `{plan['budget']}`, which budgets.json does not contain")
        found = [key for key in blessed if key == plan["id"] or key.startswith(plan["id"])]
        assert found, (f"{plan['id']} is in the plan but not in the blessed baseline: run "
                       "`make bench-bless`")


def test_the_baseline_is_machine_neutral_text():
    """A committed baseline is a claim about the code, not about the laptop that ran it last."""
    text = BASELINE.read_text()
    assert "/home/" not in text and "/Users/" not in text, "an absolute home path got blessed in"
    baseline = json.loads(text)
    assert baseline["blessed_commit"], "a blessed number without the commit it came from is folklore"
    assert baseline["blessed_environment"]["cpu_count"], "the baseline must say what ran it"


# ---------------------------------------------------------------------------------------------
# AC 4 — RSS measured, not just wall time
# ---------------------------------------------------------------------------------------------
def test_rss_is_measured_not_just_wall_time():
    baseline = json.loads(BASELINE.read_text())
    blessed = baseline["measurements"]
    rss = {key: row for key, row in blessed.items()
           if (row["budget"] or "").startswith("rss.") or key.startswith("rss.")}
    assert len(rss) >= 3, (
        f"only {len(rss)} resident-set measurements are blessed; the ticket requires RSS, not just "
        "wall time")
    for key, row in rss.items():
        assert row["scaled"] is False, f"{key}: an RSS budget marked scaled would be self-deception"
    for key, row in blessed.items():
        if row["unit"] in ("MiB", "bytes"):
            assert row["scaled"] is False, (
                f"{key}: a size on disk or in memory is not a machine property, so the runner factor "
                "must not touch it")
    kinds = {row["budget"] for row in rss.values() if row["budget"]}
    assert {"rss.cold_index.peak_mib.small", "rss.tui.mib.small"} <= kinds, (
        f"the gate must measure cold-index and TUI RSS; blessed: {sorted(kinds)}")
    # And the measurement functions really sample from outside the process: a peak read from a corpse
    # is zero, and zero passes every budget.
    source = (ROOT / "tools" / "perf_gate.py").read_text()
    assert "VmHWM" in source, "the gate must sample VmHWM, not guess a resident set"


# ---------------------------------------------------------------------------------------------
# AC 2 — a deliberate regression turns CI red
# ---------------------------------------------------------------------------------------------
def test_a_deliberate_regression_turns_the_gate_red(tmp_path):
    """The acceptance criterion itself: inject a `sleep`, and the build must go red and say why."""
    result = subprocess.run(
        [sys.executable, "tools/perf_gate.py", "--check", "--quick", "--inject-ms", "400",
         "--binary", str(_binary()), "--trend-dir", str(tmp_path)],
        cwd=ROOT, capture_output=True, text=True, timeout=600)
    assert result.returncode == G.EXIT_BUDGET, (
        f"an injected 400 ms regression must exit {G.EXIT_BUDGET} (BUDGET_EXCEEDED); got "
        f"{result.returncode}\n{result.stdout}\n{result.stderr}")
    assert "GATE RED" in result.stderr, result.stderr
    blown = [line for line in result.stderr.splitlines() if line.strip().startswith("·")]
    assert blown, "the gate went red without naming what it blew"
    assert any("cold.index" in line or "warm.reindex" in line for line in blown), (
        f"400 ms added to every process must land on cold index or warm re-index: {blown}")


def test_the_gate_is_capable_of_failing_with_no_build_at_all():
    """The same claim, at zero cost: synthetic numbers, every rule, any runner."""
    assert G.self_test() == 0, "the gate's own self-test says it cannot fail properly"


def test_the_gate_reads_its_threshold_from_the_budget_file():
    """`regression_fail_pct` is read, not typed twice: change the rule, change the verdict."""
    budgets, gates = _budgets()
    blessed = {"cold.index.ms.small": {"value": 1000.0, "unit": "ms", "budget": "cold.index.ms.small",
                                       "scaled": True}}
    baseline = {"measurements": blessed}
    row = G.Measurement("cold.index.ms.small", "ms", 1100.0, "cold.index.ms.small")
    lenient = G.evaluate([row], baseline, budgets, {"regression_fail_pct": 15.0}, 1.0, strict=True)
    assert not any(verdict.failed for verdict in lenient), "10 % is not a regression at 15 %"
    strict = G.evaluate([row], baseline, budgets, {"regression_fail_pct": 5.0}, 1.0, strict=True)
    assert any(verdict.status == "regression" for verdict in strict), (
        "the gate hard-coded a threshold instead of reading `gates.regression_fail_pct`")


def test_rss_budgets_are_never_scaled_by_the_runner_factor():
    """ADR-004 §11.8: `rss_budgets_scaled_by_factor: false`. A slow machine may not grow the heap."""
    budgets, gates = _budgets()
    budget = float(budgets["rss.cold_index.peak_mib.small"]["budget"])
    row = G.Measurement("rss.cold_index.peak_mib.small", "MiB", budget * 1.5,
                        "rss.cold_index.peak_mib.small", scaled=False)
    verdicts = G.evaluate([row], {"measurements": {}}, budgets, gates, 4.0, strict=False)
    assert any(verdict.status == "over_budget" for verdict in verdicts), (
        "a 4× runner excused a 1.5× resident set: RSS is being scaled and must never be")


def test_wall_clocks_do_get_the_runner_factor():
    """The other half of the same rule: a CI runner is not a laptop (§11.8)."""
    budgets, gates = _budgets()
    budget = float(budgets["cold.index.ms.small"]["budget"])
    row = G.Measurement("cold.index.ms.small", "ms", budget * 2.0, "cold.index.ms.small")
    verdicts = G.evaluate([row], {"measurements": {}}, budgets, gates, 4.0, strict=False)
    assert not any(verdict.status == "over_budget" for verdict in verdicts), (
        "a wall clock inside 4× of budget failed on a 4× runner: the calibration is not applied")
    tight = G.evaluate([row], {"measurements": {}}, budgets, gates, 1.0, strict=False)
    assert any(verdict.status == "over_budget" for verdict in tight), (
        "twice the budget failed to fail on a reference-speed runner")


def test_an_improvement_past_the_threshold_demands_a_re_bless():
    """`improvement_baseline_commit_required`: budgets ratchet, so a fast commit is a chosen commit."""
    budgets, gates = _budgets()
    blessed = {"cold.index.ms.small": {"value": 1000.0, "unit": "ms", "budget": "cold.index.ms.small",
                                       "scaled": True}}
    row = G.Measurement("cold.index.ms.small", "ms", 700.0, "cold.index.ms.small")
    checking = G.evaluate([row], {"measurements": blessed}, budgets, gates, 1.0, strict=True)
    assert any(verdict.status == "stale_baseline" for verdict in checking), (
        "a 30 % improvement passed silently; the baseline should have been re-blessed")
    local = G.evaluate([row], {"measurements": blessed}, budgets, gates, 1.0, strict=False)
    assert not any(verdict.failed for verdict in local), (
        "`make bench` locally should nag about an improvement, not fail: only CI enforces the ratchet")


def test_a_slow_runner_is_not_credited_an_improvement():
    """The ratchet is judged on the raw number: a busy machine is not a faster commit.

    Calibration above 1.0 divides every measurement down for the *regression* comparison — that is the
    point of a runner factor — and if the same division drove the improvement rule, a runner sharing
    itself with eight `cargo test` jobs would report the whole suite as 20 % faster and demand a
    re-bless of numbers that had not moved.
    """
    budgets, gates = _budgets()
    blessed = {"cold.index.ms.small": {"value": 1000.0, "unit": "ms", "budget": "cold.index.ms.small",
                                       "scaled": True}}
    unchanged_but_slow_runner = G.Measurement("cold.index.ms.small", "ms", 1000.0,
                                              "cold.index.ms.small")
    verdicts = G.evaluate([unchanged_but_slow_runner], {"measurements": blessed}, budgets, gates,
                          1.3, strict=True)
    assert not any(verdict.failed for verdict in verdicts), (
        f"a 1.3× runner made an unchanged number read as a 23 % improvement: "
        f"{[v.status for v in verdicts]}")
    # …and the same normalisation still protects it from the regression side.
    slower = G.Measurement("cold.index.ms.small", "ms", 1280.0, "cold.index.ms.small")
    verdicts = G.evaluate([slower], {"measurements": blessed}, budgets, gates, 1.3, strict=True)
    assert not any(verdict.status == "regression" for verdict in verdicts), (
        "a 1.3× runner was blamed for a 28 % wall-clock increase the calibration accounts for")


def test_a_rate_is_normalised_the_other_way_from_a_duration():
    """rows/s is the inverse of a duration, so the runner factor applies in the opposite direction.

    Get this backwards and a runner shared with a `cargo build` reports the write bench as regressed:
    the same code really does push fewer rows per second on a machine that is busy, and the calibration
    exists precisely to say "that is the machine, not the commit".
    """
    budgets, gates = _budgets()
    blessed = {"write.batch_rows/one_batch_at_the_cap": {
        "value": 200_000.0, "unit": "rows/s", "budget": None, "scaled": True,
        "higher_is_better": True}}
    same_code_slower_runner = G.Measurement(
        "write.batch_rows/one_batch_at_the_cap", "rows/s", 200_000.0 / 1.16, None, scaled=True,
        higher_is_better=True)
    verdicts = G.evaluate([same_code_slower_runner], {"measurements": blessed}, budgets, gates, 1.16,
                          strict=True)
    assert not any(verdict.status == "regression" for verdict in verdicts), (
        f"a 1.16× runner was blamed for a throughput drop the calibration explains: "
        f"{[v.detail for v in verdicts]}")
    genuinely_slower = G.Measurement(
        "write.batch_rows/one_batch_at_the_cap", "rows/s", 100_000.0, None, scaled=True,
        higher_is_better=True)
    verdicts = G.evaluate([genuinely_slower], {"measurements": blessed}, budgets, gates, 1.0,
                          strict=True)
    assert any(verdict.status == "regression" for verdict in verdicts), (
        "a halved write throughput is the regression §13.1's 12× batching story says to catch")


def test_a_recorded_breach_is_bounded_not_waived():
    """A known failure is a ceiling and an owner, not a hole in the gate."""
    budgets, gates = _budgets()
    blessed = {"warm.reindex.noop.ms.small": {
        "value": 48.0, "unit": "ms", "budget": "warm.reindex.noop.ms.small", "scaled": True}}
    baseline = {"measurements": blessed, "known_breaches": {
        "warm.reindex.noop.ms.small": {"ceiling": 70.0, "tolerance": 1.1, "owner": "reco-7tc",
                                       "why": "the walk is repeated, not skipped"}}}
    inside = G.Measurement("warm.reindex.noop.ms.small", "ms", 60.0, "warm.reindex.noop.ms.small")
    verdicts = G.evaluate([inside], baseline, budgets, gates, 1.0, strict=True)
    assert [verdict.status for verdict in verdicts] == ["known_breach"], verdicts
    assert not any(verdict.failed for verdict in verdicts)
    worse = G.Measurement("warm.reindex.noop.ms.small", "ms", 120.0, "warm.reindex.noop.ms.small")
    grown = G.evaluate([worse], baseline, budgets, gates, 1.0, strict=True)
    assert any(verdict.status == "over_budget" for verdict in grown), (
        "a recorded breach grew past its ceiling and stayed green")


def test_the_calibration_factor_is_clamped_to_the_published_range():
    bounds = _budgets()[1]["runner_calibration_factor_range"]
    low, high = bounds[0], bounds[1]
    assert G.runner_factor(50.0, 100.0, bounds)[0] == low, (
        "a fast runner was allowed to buy extra headroom")
    assert G.runner_factor(900.0, 100.0, bounds)[0] == high, (
        "a machine far outside the window should clamp and say so, not hand out an unlimited pass")
    assert "ceiling" in G.runner_factor(900.0, 100.0, bounds)[1]
    factor, note = G.runner_factor(120.0, 100.0, bounds)
    assert abs(factor - 1.2) < 1e-9 and note == "", "a runner inside the window should be believed"


def test_a_measurement_needs_both_a_percentage_and_an_absolute_change_to_count():
    """Otherwise a 2 ms jitter on a 12 ms number is a 'regression' and the gate gets ignored."""
    budgets, gates = _budgets()
    blessed = {"query.card.ms.small": {"value": 12.0, "unit": "ms", "budget": "query.card.ms.small",
                                       "scaled": True}}
    jitter = G.Measurement("query.card.ms.small", "ms", 13.9, "query.card.ms.small")
    verdicts = G.evaluate([jitter], {"measurements": blessed}, budgets, gates, 1.0, strict=True)
    assert not any(verdict.failed for verdict in verdicts), (
        "a 1.9 ms change on a 12 ms number read as a regression: the noise floor is missing")
    real = G.Measurement("query.card.ms.small", "ms", 40.0, "query.card.ms.small")
    verdicts = G.evaluate([real], {"measurements": blessed}, budgets, gates, 1.0, strict=True)
    assert any(verdict.status == "regression" for verdict in verdicts)


# ---------------------------------------------------------------------------------------------
# The plumbing: exit code, make target, artefact, and where the bench code lives
# ---------------------------------------------------------------------------------------------
def test_the_gate_speaks_the_products_exit_code():
    """`BUDGET_EXCEEDED` is one number in one place: `fathom-core/src/exit.rs`."""
    source = (ROOT / "crates" / "fathom-core" / "src" / "exit.rs").read_text()
    match = re.search(r"BudgetExceeded\s*=\s*(\d+)", source)
    assert match, "exit.rs no longer names BudgetExceeded"
    assert int(match.group(1)) == G.EXIT_BUDGET == 11, (
        f"the gate exits {G.EXIT_BUDGET} and fathom-core says {match.group(1)}")


def test_make_bench_exists_and_is_the_documented_command():
    make = MAKEFILE.read_text()
    for target in ("bench:", "bench-check:", "bench-bless:", "bench-selftest:", "bench-criterion:"):
        assert re.search(rf"^{target}", make, re.M), f"`make {target[:-1]}` does not exist"
    assert "perf_gate.py" in make, "the make targets must call the gate, not a copy of it"
    budgets_md = BUDGETS_MD.read_text()
    for command in ("make bench", "make bench-check", "make bench-bless"):
        assert command in budgets_md, f"BUDGETS.md must document `{command}`"


def test_the_promised_runtime_is_under_five_minutes():
    """AC 3's number, as data: the harness promises it and the CI timeout allows it."""
    assert G.PROMISED_SECONDS <= 300, "`make bench` promised five minutes"
    workflow = WORKFLOW.read_text()
    timeout = re.search(r"timeout-minutes:\s*(\d+)", workflow)
    assert timeout and int(timeout.group(1)) * 60 > G.PROMISED_SECONDS, (
        "the CI job's timeout must be longer than the promise, or a slow runner is a false red")


def test_the_trend_is_an_artefact_not_a_committed_file():
    assert "benchmarks/trend/" in (ROOT / ".gitignore").read_text(), (
        "the trend is a job artefact; committing it is a merge conflict on every PR")
    tracked = subprocess.run(["git", "-C", str(ROOT), "ls-files", "benchmarks/trend"],
                             capture_output=True, text=True).stdout.strip()
    assert tracked == "", f"the trend must not be tracked: {tracked}"
    workflow = WORKFLOW.read_text()
    assert "upload-artifact" in workflow, "the trend must be published as a job artefact"
    assert "if: always()" in workflow, (
        "the trend is most interesting on the run that went red; publishing it only on green hides "
        "the growth the artefact exists to make visible")


def test_criterion_is_a_dev_dependency_only():
    """The benches are criterion; criterion must never be linked into anything that ships."""
    manifest = (ROOT / "crates" / "fathom-store" / "Cargo.toml").read_text()
    dependencies, section = [], None
    for line in manifest.splitlines():
        header = re.match(r"^\[([^\]]+)\]", line.strip())
        if header:
            section = header.group(1)
        if "criterion" in line and not line.strip().startswith("#"):
            dependencies.append(section)
    assert dependencies and set(dependencies) <= {"dev-dependencies"}, (
        f"criterion leaked out of [dev-dependencies]: {dependencies}")
    assert re.search(r"^\[\[bench\]\]", manifest, re.M), "the bench target is not declared"
    assert "harness = false" in manifest, "criterion needs harness = false or libtest eats the bench"
    bench = ROOT / "crates" / "fathom-store" / "benches" / "budgets.rs"
    assert bench.is_file(), "the criterion benches are the ticket's headline deliverable"
    text = bench.read_text()
    for family in ("query.timeline.ms", "query.card.ms", "query.frame_store_budget.ms",
                   "write.batch_rows"):
        assert f'benchmark_group("{family}")' in text, f"no criterion group for `{family}`"


def test_the_criterion_samples_become_percentiles():
    """Criterion writes per-iteration samples; the gate must turn them into the published statistic."""
    budgets, _ = _budgets()
    assert "query.timeline.ms.small" in budgets
    source = (ROOT / "tools" / "perf_gate.py").read_text()
    assert "sample.json" in source, (
        "criterion's samples live in sample.json; parsing estimates.json instead would lose the tail")
    assert G.p95([1.0, 2.0, 3.0, 4.0, 5.0]) == 5.0, "p95 of five samples is the worst one"
    assert G.p95([float(index) for index in range(1, 101)]) == 95.0
