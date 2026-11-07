"""The perf budget gate, kept honest (`e0.11`, `reco-pef`).

ADR-004 §11 publishes 172 numbers and a block of gate rules, and says `e0.11` asserts on them. This
file is the check that the assertion is real:

* `benchmarks/BUDGETS.md` lists **every** budget with its numeric threshold, and agrees with
  `benchmarks/budgets.json` rather than with someone's memory of it;
* the gate fails a deliberate regression (a real `sleep` injected in front of every measured process),
  which is the acceptance criterion — a gate nobody has seen fail is decoration;
* the arithmetic that cannot be tested by one run is tested directly: the threshold comes from the
  budget file, RSS budgets are never scaled by the runner factor while wall clocks are, a recorded
  breach is bounded rather than waived, and an improvement past the threshold demands a re-bless —
  judged in reference-machine units, so neither a busy runner nor a fast one moves the ratchet;
* a red gate says so where a reader looks: the failing measurements are GitHub annotations, not just
  lines in a log nobody opened (`reco-axp`);
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
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import perf_gate as G  # noqa: E402
import mvp_acceptance as M  # noqa: E402  (the dbstat split the store budgets are computed from)

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


def test_a_red_gate_announces_its_failures_as_ci_annotations(tmp_path):
    """A red perf job whose check page says only "exit code 11" is a job nobody can read.

    `reco-axp` was filed from precisely that: the check carried the cannot-classify warnings and
    `Process completed with exit code 11`, and the budget that actually stopped the build was in the
    log and nowhere else — so the failing row had to be reconstructed by re-running the gate on a
    different machine. The verdict that stops the build announces itself as loudly as the ones that do
    not.
    """
    result = subprocess.run(
        [sys.executable, "tools/perf_gate.py", "--check", "--quick", "--inject-ms", "400",
         "--binary", str(_binary()), "--trend-dir", str(tmp_path)],
        cwd=ROOT, capture_output=True, text=True, timeout=600,
        env={**os.environ, "GITHUB_ACTIONS": "true"})
    assert result.returncode == G.EXIT_BUDGET, (
        f"expected {G.EXIT_BUDGET}; got {result.returncode}\n{result.stdout}\n{result.stderr}")
    annotated = [line for line in result.stdout.splitlines()
                 if line.startswith("::error title=Perf budget gate::")]
    assert annotated, "the gate went red on a CI runner and annotated nothing: " + result.stdout
    assert any("cold.index" in line or "warm.reindex" in line for line in annotated), (
        f"the injected regression must name itself in the annotations: {annotated}")


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


def test_a_runner_faster_than_the_blessed_machine_is_not_credited_an_improvement():
    """The other half of `reco-axp`: the clamp is about budget headroom, not about what a runner proves.

    A blessed time is a reference-machine time (§11.8), so a runner the calibration bench measures at
    0.8× of it measures every wall clock two tenths low, and the raw-number rule read that as the whole
    suite getting faster: a `stale_baseline` red that was the runner and nobody else, which is what
    "this may just be a slower runner on GitHub" was reaching for. Dividing the improvement half by the
    *unclamped* ratio says the machine's speed out loud in the one direction the clamp was never about.
    """
    budgets, gates = _budgets()
    blessed = {"cold.index.ms.small": {"value": 1000.0, "unit": "ms", "budget": "cold.index.ms.small",
                                       "scaled": True}}
    unchanged_on_a_fast_runner = G.Measurement("cold.index.ms.small", "ms", 800.0,
                                               "cold.index.ms.small")
    verdicts = G.evaluate([unchanged_on_a_fast_runner], {"measurements": blessed}, budgets, gates,
                          1.0, strict=True, machine_factor=0.8)
    assert not any(verdict.failed for verdict in verdicts), (
        "a 0.8× runner turned unchanged code into a 20 % improvement: "
        f"{[(v.id, v.status, v.detail) for v in verdicts]}")
    # …and a speed-up that is real still has to be re-blessed, on a fast runner like on any other.
    genuinely_faster = G.Measurement("cold.index.ms.small", "ms", 560.0, "cold.index.ms.small")
    verdicts = G.evaluate([genuinely_faster], {"measurements": blessed}, budgets, gates, 1.0,
                          strict=True, machine_factor=0.8)
    assert any(verdict.status == "stale_baseline" for verdict in verdicts), (
        "a 30 % speed-up in reference-machine units went uncredited: dividing out a fast runner may "
        "not swallow a real improvement along with the machine")
    # And the clamp still stands where it was always about: a fast runner buys no extra budget.
    over = G.Measurement("cold.index.ms.small", "ms", 3000.0, "cold.index.ms.small")
    verdicts = G.evaluate([over], {"measurements": blessed}, budgets, gates, 1.0, strict=True,
                          machine_factor=0.8)
    assert any(verdict.status == "over_budget" for verdict in verdicts), (
        "a 0.8× runner was handed budget headroom; the clamp belongs to the budget, not to the ratchet")


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
# The ratchet's resolution, measured per measurement (`reco-dkr`)
# ---------------------------------------------------------------------------------------------
def test_the_noise_floor_is_measured_per_measurement_not_a_constant_per_unit():
    """A 5 ms floor on a 15 ms measurement is a 33 % gate, which is a ratchet that cannot read.

    The floor a value is judged by is now blessed beside it, so a verb that scatters by 0.3 ms is not
    gated by the constant chosen for the noisiest probe in the suite — and a value blessed *before*
    floors existed keeps that constant, because the way to earn a tighter floor is to measure one.
    """
    budgets, _gates = _budgets()
    blessed = {"query.metrics.ms.small": {"value": 15.0, "center": 14.8, "noise": 0.6, "scale": 0.3,
                                          "unit": "ms", "budget": "query.metrics.ms.small",
                                          "scaled": True}}
    floor, basis = G.noise_floor(
        G.Measurement("query.metrics.ms.small", "ms", 15.0, "query.metrics.ms.small",
                      samples=[14.4, 14.9, 15.0, 14.7, 14.6]),
        14.8, blessed["query.metrics.ms.small"])
    assert floor < G.NOISE_FLOOR["ms"], (
        f"a verb probe whose own samples scatter by 0.3 ms is still gated by the 5 ms per-unit "
        f"constant ({floor}) even though its blessed floor says 0.6")
    assert "scale" in basis or "blessed" in basis, f"the floor should say where it came from: {basis}"

    deaf = G.Measurement("query.metrics.ms.small", "ms", 15.0, "query.metrics.ms.small",
                         samples=[14.9, 15.0, 15.1])
    assert abs(G.noise_floor(deaf, 15.0, {"value": 15.0})[0] - 5.0) < 1e-9, (
        "a measurement blessed without a floor lost the constant it is supposed to keep: everything "
        "blessed before floors existed must be gated exactly as it was before")

    quiet_run = G.noise_floor(G.Measurement("query.metrics.ms.small", "ms", 15.0,
                                            "query.metrics.ms.small", samples=[15.0, 15.0, 15.0]),
                              15.0, {"noise": 2.5})
    assert abs(quiet_run[0] - 2.5) < 1e-9, (
        "a run that sampled luckily may not resolve better than the floor blessed with the value")
    assert "blessed" in quiet_run[1]

    pty = G.Measurement("query.timeline.ms.small", "ms", 1.8, "query.timeline.ms.small",
                        samples=[1.6, 1.9, 1.7], noise=G.PTY_NOISE_MS)
    assert G.noise_floor(pty, 1.8)[0] == G.PTY_NOISE_MS, (
        "the pty's own resolution is the floor that stands: it is the instrument, not the samples")


def test_the_ratchet_can_see_a_real_fifteen_percent_regression_on_a_verb():
    """The whole point of the ratchet, on the id the ticket named: ±15 % of ~15 ms must be visible."""
    budgets, gates = _budgets()
    blessed = {"query.metrics.ms.small": {"value": 15.0, "center": 14.8, "noise": 0.9, "spread": 0.6,
                                          "unit": "ms", "budget": "query.metrics.ms.small",
                                          "scaled": True}}
    regression = G.Measurement("query.metrics.ms.small", "ms", 17.6, "query.metrics.ms.small",
                               samples=[17.1, 17.3, 17.4, 17.2, 17.6])
    verdicts = G.evaluate([regression], {"measurements": blessed}, budgets, gates, 1.0, strict=True)
    assert any(verdict.status == "regression" for verdict in verdicts), (
        "a 17 % slowdown on a verb the users wait on did not fail the build: the floor is still "
        "wider than the ratchet")


def test_a_change_inside_the_measurement_s_own_noise_is_said_not_decided():
    """Red on one run and green on the next is not a gate. It is a coin toss, and it must be named."""
    budgets, gates = _budgets()
    blessed = {"query.metrics.ms.small": {"value": 15.0, "center": 15.0, "noise": 0.9, "unit": "ms",
                                          "budget": "query.metrics.ms.small", "scaled": True}}
    bimodal = G.Measurement("query.metrics.ms.small", "ms", 21.0, "query.metrics.ms.small",
                            samples=[13.0, 15.0, 21.0, 19.0, 18.5])

    verdicts = G.evaluate([bimodal], {"measurements": blessed}, budgets, gates, 1.0, strict=True)
    assert not any(verdict.failed for verdict in verdicts), (
        "a change smaller than the spread inside the same run went red: that is the coin toss "
        "reco-dkr was filed about")
    assert any(not verdict.resolves for verdict in verdicts), (
        "the gate passed a change it could not classify without saying it could not classify it")
    assert any("cannot classify" in verdict.detail for verdict in verdicts)


def test_a_verb_that_flaps_between_two_modes_is_not_a_regression_on_its_centre():
    """The ticket's own numbers: 14.8, 14.9 and 21.6 ms minutes apart on one tree.

    The median of that run is the blessed centre. The p95 of three samples is a coin toss between the
    two modes, and the ratchet no longer reads it — which is what makes the same tree either red or
    green depending on the hour, and is the flapping the ticket asked to be fixed rather than tolerated.
    """
    budgets, gates = _budgets()
    blessed = {"query.metrics.ms.small": {"value": 15.1, "center": 14.9, "noise": 0.9, "unit": "ms",
                                          "budget": "query.metrics.ms.small", "scaled": True}}
    flapped = G.Measurement("query.metrics.ms.small", "ms", 21.6, "query.metrics.ms.small",
                            samples=[14.8, 14.9, 21.6])
    verdicts = G.evaluate([flapped], {"measurements": blessed}, budgets, gates, 1.0, strict=True)
    assert not any(verdict.failed for verdict in verdicts), (
        "the same tree was judged on which mode its three samples landed in: "
        + "; ".join(v.detail for v in verdicts if v.failed))
    assert verdicts[0].resolves, "a run whose centre did not move is a clean read, not a shrug"


# ---------------------------------------------------------------------------------------------
# The store's denominators (`reco-7yq`): §11.6 budgets the commit tables and the graph lane
# separately, so the split those budgets are computed from has to tell them apart — and an index has
# to be charged to the table it indexes, not to whatever its name happens to start with.
# ---------------------------------------------------------------------------------------------

#: Every page the synthetic store below creates, and the denominator §11.6 says it belongs to.
STORE_PAGES = {
    "git_commit": "spine", "git_commit_file": "spine", "ref_tip": "spine",
    # An index named after neither table, which a name-prefix rule loses: `sqlite_master` knows it is
    # `git_commit`'s, and the spine budget is a claim about `git_commit`.
    "commit_tree": "spine", "sqlite_autoindex_git_commit_1": "spine",
    "graph_snapshot": "graph", "graph_snapshot_base": "graph", "graph_delta": "graph",
    "graph_chunk": "graph", "sqlite_autoindex_graph_chunk_1": "graph",
    "sqlite_autoindex_graph_snapshot_1": "graph",
    "cache": "cache", "cache_input": "cache", "session_state": "cache",
    "sqlite_autoindex_cache_1": "cache", "sqlite_autoindex_session_state_1": "cache",
    "sqlite_autoindex_ref_tip_1": "spine",
    "blob": "other", "sqlite_autoindex_blob_1": "other",
    "episode": "other", "sqlite_autoindex_episode_1": "other",
    "meta": "other", "sqlite_autoindex_meta_1": "other", "sqlite_schema": "other",
}


def _synthetic_store(tmp_path: Path) -> Path:
    """A store-shaped file: §11.6's tables, their indexes, and enough rows to own pages."""
    import sqlite3

    db = tmp_path / "index.sqlite3"
    conn = sqlite3.connect(db)
    conn.executescript(
        "CREATE TABLE git_commit (oid TEXT PRIMARY KEY, subject TEXT, tree TEXT);"
        "CREATE INDEX commit_tree ON git_commit (tree);"
        "CREATE TABLE git_commit_file (oid TEXT, path TEXT);"
        "CREATE TABLE ref_tip (name TEXT PRIMARY KEY, oid TEXT);"
        "CREATE TABLE graph_snapshot (tree_oid TEXT PRIMARY KEY, kind TEXT);"
        "CREATE INDEX graph_snapshot_base ON graph_snapshot (kind);"
        "CREATE TABLE graph_delta (src TEXT, dst TEXT, kind TEXT);"
        "CREATE TABLE graph_chunk (sha256 TEXT PRIMARY KEY, kind TEXT);"
        "CREATE TABLE cache (analyzer TEXT, key TEXT PRIMARY KEY, blob TEXT);"
        "CREATE TABLE cache_input (key TEXT, part TEXT);"
        "CREATE TABLE session_state (key TEXT PRIMARY KEY, value TEXT);"
        "CREATE TABLE blob (sha256 TEXT PRIMARY KEY, bytes INTEGER);"
        "CREATE TABLE episode (oid TEXT PRIMARY KEY);"
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);")
    conn.executemany("INSERT INTO git_commit VALUES (?, ?, ?)",
                     [(f"{i:040}", f"subject {i}", f"{i + 1:040}") for i in range(400)])
    conn.executemany("INSERT INTO graph_delta VALUES (?, ?, ?)",
                     [(f"{i:040}", f"{i + 1:040}", "edit") for i in range(400)])
    conn.executemany("INSERT INTO cache VALUES (?, ?, ?)",
                     [("diff.per_file", f"{i:040}", "x" * 200) for i in range(400)])
    conn.commit()
    conn.close()
    return db


def _synthetic_owners(db: Path) -> list[tuple[str, str]]:
    import sqlite3

    connection = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        return list(connection.execute(
            "SELECT name, tbl_name FROM sqlite_master WHERE type = 'index' AND name IS NOT NULL"))
    finally:
        connection.close()


def test_the_store_split_partitions_the_file_into_the_adrs_denominators(tmp_path):
    """`store.bytes_per_commit_spine` charges the commit tables and nothing else (`reco-7yq`)."""
    db = _synthetic_store(tmp_path)
    split = M.store_split(db)
    assert split["spine"], "the spine came out of the split empty"
    assert split["graph"], "the graph lane came out of the split empty — then it is still in the spine"
    owners = dict(_synthetic_owners(db))
    assert {name: M.store_bucket(name, owners) for name in split["tables"]} == STORE_PAGES, (
        "a `dbstat` page was charged to the wrong denominator")
    for bucket in ("spine", "graph", "cache", "other"):
        expected = sum(size for name, size in split["tables"].items()
                       if STORE_PAGES[name] == bucket)
        assert split[bucket] == expected, f"{bucket} does not sum over its own pages"
    assert split["spine"] + split["graph"] + split["cache"] + split["other"] == split["total"], (
        "the four buckets are not a partition of the file")
    # The defect this is a regression test for, named: the lane used to be *not-cache*, and *not-cache*
    # was called the spine. This assert is the difference between the two byte models.
    assert split["spine"] < split["total"] - split["cache"]


def test_the_graph_lane_is_measured_now_that_it_has_a_budget_of_its_own():
    """A budget on the books and a number nobody measures is a budget nobody fails (§11.7)."""
    coverage = G.coverage()
    for family in ("store.bytes_per_commit_spine", "store.bytes_per_commit_graph"):
        assert coverage[family][0] != "not measured yet", (
            f"{family}: {coverage[family][1]} — §11.6 budgets it, so the gate has to charge it")
    blessed = json.loads(BASELINE.read_text())["measurements"]
    for ident in ("store.bytes_per_commit_spine.small", "store.bytes_per_commit_graph.small"):
        row = blessed.get(ident)
        assert row, f"{ident} is not blessed: the gate measures it and the baseline does not believe it"
        assert row["scaled"] is False, f"{ident}: bytes on disk are not a machine property"
        assert row["value"] > 0, f"{ident}: a blessed zero is a measurement that can never fail"


def test_the_blessed_baseline_carries_the_noise_each_value_was_blessed_with():
    """A blessed number and its noise are one measurement; half of it is a guess about the other half."""
    blessed = json.loads(BASELINE.read_text())["measurements"]
    fail_pct = _budgets()[1]["regression_fail_pct"]
    for ident in ("query.metrics.ms.small", "query.card.ms.cli.small", "warm.reindex.ms.small"):
        row = blessed[ident]
        assert row.get("center") is not None, f"{ident}: blessed without the statistic the ratchet uses"
        assert row.get("noise") is not None, f"{ident}: blessed without the floor it was judged by"
        limit = float(row["center"]) * fail_pct / 100.0
        assert float(row["noise"]) < limit, (
            f"{ident}: its blessed floor ({row['noise']} {row['unit']}) is wider than the ±{fail_pct:g} % "
            f"({limit:.2f} {row['unit']}) that is supposed to gate it, so the ratchet cannot resolve it")


def test_the_gate_reports_the_changes_it_could_not_classify(tmp_path):
    """`make bench-check` must publish what it could not see, not just what it failed on."""
    report = tmp_path / "trend"
    result = subprocess.run(
        [sys.executable, "tools/perf_gate.py", "--check", "--quick", "--binary", str(_binary()),
         "--trend-dir", str(report)],
        cwd=ROOT, capture_output=True, text=True, timeout=600)
    assert result.returncode == 0, result.stdout + result.stderr
    written = json.loads((report / "latest.json").read_text())
    assert "resolution" in written, "the report carries no resolution block"
    assert written["resolution"]["regression_fail_pct"] == _budgets()[1]["regression_fail_pct"]
    assert isinstance(written["resolution"]["changes_this_run_could_not_classify"], list)
    blind = {item["id"]: item for item in written["resolution"]["ids_the_ratchet_cannot_resolve"]}
    assert blind, "the run found no blind spot at all: the pty probes measure 1-5 ms through a "\
        "10 ms instrument and that is a blind spot"
    for ident in ("query.timeline.ms.small", "query.queue.ms.small"):
        assert ident in blind, f"{ident} is measured through the pty and is not listed as unresolvable"
        assert blind[ident]["floor_as_pct_of_baseline"] > 100.0
        assert "resolution" in blind[ident]["noise_basis"].lower()
    assert "query.metrics.ms.small" not in blind, (
        "the verb the ticket was filed about is back to being unresolvable: its blessed floor is "
        "wider than the ±15 % again")


def test_measuring_a_child_does_not_add_the_harness_s_own_sleep_to_the_clock():
    """The bug behind the flapping: a 2 ms poll inside the timed region is 2 ms of the measurement.

    Interleaved against `subprocess.run` on the same child, so a busy runner is a bit slow to both
    sides of the comparison and only the harness's own overhead separates them.
    """
    import statistics
    import subprocess as sp

    sys.path.insert(0, str(ROOT / "tools"))
    import mvp_acceptance as M

    child = [sys.executable, "-c", "pass"]
    plain, timed = [], []
    for _ in range(15):
        started = time.monotonic()
        sp.run(child, stdout=sp.DEVNULL, stderr=sp.DEVNULL)
        plain.append((time.monotonic() - started) * 1000.0)
        timed.append(M.run_timed(child, ROOT)[0])
    gap = statistics.median(timed) - statistics.median(plain)
    assert gap < 1.0, (
        f"the harness adds {gap:.2f} ms to every measurement it takes; the old poll loop added "
        "1.7-2.0 ms and a tail as long as the ratchet")


def test_the_rss_peak_is_sampled_while_the_child_holds_memory():
    """A peak measured from a corpse is zero, and one measured 2 ms late is a tenth short.

    The child reports its own peak out of `/proc/self/status` rather than out of
    `resource.getrusage(...).ru_maxrss`, and that is the whole point of the fixture. Linux does not reset
    an accumulated `ru_maxrss` across `fork`+`execve`, so a child reports
    `max(whatever the forking process had reached, its own peak)` — which is exactly the trap
    `mvp_acceptance._PeakSampler`'s docstring refuses. Run on its own the two numbers agree, and the test
    passes; run as part of the whole suite, where the pytest process has already touched more memory than
    the child ever will, the *truth* is the test runner's own resident set and the sampler is failed for
    measuring the child accurately. `VmHWM` is per address space and is reset by `execve`, so it is the
    same instrument the sampler reads from beside the child, and it is a fact about the child alone.
    """
    import statistics
    import subprocess as sp

    sys.path.insert(0, str(ROOT / "tools"))
    import mvp_acceptance as M

    child = ("import time\n"
             "x = bytearray(60_000_000); y = bytes(x)\n"
             "print(int(next(l for l in open('/proc/self/status') if l.startswith('VmHWM:')).split()[1]),"
             " flush=True)\n"
             "time.sleep(0.02)\n")
    truth = int(sp.run([sys.executable, "-c", child], capture_output=True, text=True,
                       timeout=60).stdout.strip())
    assert truth > 50_000, (
        f"the fixture's own peak is {truth} KiB: it allocated nothing, so there is nothing to catch")
    sampled = statistics.median(M.run_timed([sys.executable, "-c", child], ROOT)[1]
                                for _ in range(5))
    assert sampled >= truth * 0.95, (
        f"the sampler read {sampled:.0f} KiB of a {truth} KiB peak: a peak reached on the way out "
        "of a verb is the peak the RSS budgets are about")


# ---------------------------------------------------------------------------------------------
# The sampler is a thread, and CPython owns some of a thread's names (`reco-yfd`)
# ---------------------------------------------------------------------------------------------
#: `threading.Thread` members CPython binds itself and calls on your behalf, across every interpreter
#: this repository runs. The list is written out rather than read off `dir(threading.Thread)` because
#: the interpreter running this file is not the one that broke: through 3.12 `Thread._stop` is a
#: *method* that `Thread._wait_for_tstate_lock` — the body of every `join()` — calls when the thread
#: finishes, and 3.13 removed it. Bind an `Event` to `self._stop` in a subclass and every join raises
#: `TypeError: 'Event' object is not callable`, which is the sentence that killed the perf gate two
#: seconds into its first `fathom index` on CI's 3.12 while it stayed green on a newer interpreter.
#: A guard that only knows the interpreter it is standing on guards nothing.
THREAD_PRIVATES = (
    "_start", "_stop", "_bootstrap", "_bootstrap_inner", "_wait_for_tstate_lock", "_tstate_lock",
    "_is_stopped", "_started", "_done", "_initialized", "_daemonic", "_ident", "_native_id",
    "_name", "_handle_interrupt", "_invoke_excepthook", "_make_invoke_excepthook", "_delete",
)


def _attributes_bound_by(cls, method: str) -> set[str]:
    """The `self.<name>` bindings made inside `cls.<method>`, read off the source.

    Off the source rather than off an instance, because the question is what the code binds — an
    instance of a *fixed* subclass is a snapshot of one interpreter's answer, and this must hold on
    interpreters that are not the one running the test.
    """
    import ast
    import inspect
    import textwrap

    names: set[str] = set()
    tree = ast.parse(textwrap.dedent(inspect.getsource(getattr(cls, method))))
    for node in ast.walk(tree):
        targets = list(node.targets) if isinstance(node, ast.Assign) else []
        if isinstance(node, ast.AnnAssign):
            targets = [node.target]
        for target in targets:
            if (isinstance(target, ast.Attribute) and isinstance(target.value, ast.Name)
                    and target.value.id == "self"):
                names.add(target.attr)
    return names


def test_the_peak_sampler_binds_no_attribute_threading_itself_owns():
    """The name `self._stop` belongs to `threading.Thread`; the gate's sampler had taken it.

    `_PeakSampler` is a `Thread` subclass and it used to bind `self._stop = threading.Event()`, which
    overwrites CPython's own `Thread._stop` method for every interpreter up to 3.12. `join()` calls
    that method once the thread has ended, so *every* RSS sample raised
    `TypeError: 'Event' object is not callable` and `python3 tools/perf_gate.py --check` died before
    it had indexed anything (`reco-yfd`). The fix is a name that is ours; this is the guard.
    """
    sys.path.insert(0, str(ROOT / "tools"))
    import mvp_acceptance as M

    shadowed = _attributes_bound_by(M._PeakSampler, "__init__") & set(THREAD_PRIVATES)
    assert not shadowed, (
        f"_PeakSampler rebinds {sorted(shadowed)}, which `threading.Thread` uses for its own "
        "lifecycle; on CI's interpreter joining the sampler then raises \"TypeError: 'Event' object "
        "is not callable\" and the perf gate dies before it has indexed anything")


def test_joining_the_sampler_after_a_measurement_returns_a_number():
    """The shape of the crash, from the other side: a measurement must join, not raise.

    `run_timed` stops the sampler by setting its flag and joining it, so on an interpreter where that
    flag has the same name as `Thread._stop` this call never returns a number — it raises. This test
    is the one that fails on the interpreter CI runs (`perf.yml` pins 3.12) and is quiet about on a
    newer one, which is exactly why it is here next to the guard rather than instead of it.
    """
    sys.path.insert(0, str(ROOT / "tools"))
    import mvp_acceptance as M

    wall_ms, peak_kib = M.run_timed([sys.executable, "-c", "pass"], ROOT)
    assert wall_ms > 0.0, f"a child that ran at all measured {wall_ms} ms"
    assert peak_kib >= 0, f"a resident set cannot be negative, and this one is {peak_kib}"


# ---------------------------------------------------------------------------------------------
# The probe's own clock: a frame that lands late is late, not absent (`reco-cwx`)
# ---------------------------------------------------------------------------------------------
def _fake_tui(tmp_path, frames):
    """Write an executable "TUI" that paints `frames` = [(delay_ms, text), …] and then quits.

    It is not the product and it does not pretend to be: it is a clock with a screen, which is the only
    honest way to hand the probe a first frame at a *known* delay. The delays below are all outside
    the reader's ~100 ms settle window, which is the whole shape of `reco-cwx`.
    """
    body = ["#!/usr/bin/env python3",
            "import sys, time",
            "try:",
            "    import tty; tty.setraw(sys.stdin.fileno())",  # the TUI takes the terminal; so does this
            "except Exception:",
            "    pass",
            f"frames = {frames!r}",
            "for delay_ms, text in frames:",
            "    time.sleep(delay_ms / 1000.0)",
            "    sys.stdout.write('\\x1b[2J\\x1b[H' + text)",
            "    sys.stdout.flush()",
            "time.sleep(1.0)"]
    fake = tmp_path / "fake-tui"
    fake.write_text("\n".join(body) + "\n", encoding="utf-8")
    fake.chmod(0o755)
    return fake


def _session(tmp_path, binary, steps_json, script_json):
    """Drive `tools/tui_session.py` over a pty and hand back the timings it wrote."""
    result = subprocess.run(
        [sys.executable, "tools/tui_session.py", "--root", str(tmp_path), "--binary", str(binary),
         "--script", str(script_json), "--steps", str(steps_json)],
        cwd=ROOT, capture_output=True, text=True, timeout=120)
    assert result.returncode == 0, result.stdout + result.stderr
    return json.loads(Path(steps_json).read_text())


def test_a_first_frame_later_than_the_settle_window_is_measured_not_missed(tmp_path):
    """The bug: two quiet ticks before anything arrived was read as "there is no frame".

    `drain` settled on ~100 ms of silence, so a first paint at 300 ms came back `None` and the gate
    reported `cold.queue_visible.ms.small` as *blocked* — a published 3 000 ms budget, unmeasured, on
    every box slower than the one it was blessed on. Quiet must mean "the answer has stopped", never
    "nothing has arrived yet".
    """
    fake = _fake_tui(tmp_path, [(300, "FRAME ONE"), (400, "FRAME TWO")])
    script = tmp_path / "script.json"
    script.write_text(json.dumps({"title": "late paint", "steps": [
        {"keys": "t", "note": "answered late", "settle": 1.0},
        {"keys": "q", "note": "nothing after this", "settle": 0.3},
    ]}), encoding="utf-8")
    steps = tmp_path / "steps.json"
    out = _session(tmp_path, fake, steps, script)

    assert out["first_paint_ms"] is not None, (
        "a first frame 300 ms after spawn was reported as no frame at all: the reader is allowed to "
        "call silence *settled* only once something has been seen")
    assert 250.0 <= out["first_paint_ms"] <= 900.0, (
        f"first paint measured {out['first_paint_ms']} ms against a fake that paints at 300 ms")
    painted = {step["keys"]: step["visible_ms"] for step in out["steps"]}
    assert painted.get("t") is not None, (
        f"a frame 400 ms after the key was written read as 'the key drew nothing': {painted}")
    assert painted["q"] is None, (
        "a key that really does draw nothing must still come back as nothing — the fix is the "
        "censoring, not a floor under every measurement")


def test_a_relative_binary_is_resolved_before_the_pty_child_leaves_the_directory(tmp_path):
    """A `--binary` the shell can see and the child cannot used to exec-fail *onto the pty*.

    The child chdirs into the repository before it execs, so a relative path pointed at nothing; the
    child's own "exec failed: [Errno 2]" text then arrived as terminal output and was timed as a first
    frame about 1 ms after spawn. A dead child measured as the fastest paint in the suite is a number
    that lies.
    """
    fake = _fake_tui(tmp_path, [(50, "FRAME ONE")])
    relative = Path(os.path.relpath(fake, ROOT))
    assert not relative.is_absolute(), "the point of the test is a path the pty child cannot resolve"
    script = tmp_path / "script.json"
    script.write_text(json.dumps({"title": "relative", "steps": [
        {"keys": "", "note": "open", "hold": 0.3},
    ]}), encoding="utf-8")
    steps = tmp_path / "steps.json"
    out = _session(tmp_path, relative, steps, script)
    assert out["first_paint_ms"] is not None and out["first_paint_ms"] >= 30.0, (
        f"the child did not exec: {out}")
    assert "exec failed" not in json.dumps(out), "the child's exec error was recorded as the frame"


def test_the_first_frame_budget_is_measured_rather_than_blocked(tmp_path):
    """The gate, end to end: `cold.queue_visible.ms.small` gets a number, not a shrug."""
    result = subprocess.run(
        [sys.executable, "tools/perf_gate.py", "--check", "--quick", "--binary", str(_binary()),
         "--trend-dir", str(tmp_path)],
        cwd=ROOT, capture_output=True, text=True, timeout=600)
    report = json.loads((tmp_path / "latest.json").read_text())
    rows = {verdict["id"]: verdict for verdict in report.get("verdicts", [])}
    first = rows.get("cold.queue_visible.ms.small")
    assert first is not None, "the gate did not report the first-frame budget at all"
    assert first["status"] != "blocked" and first["measured"] is not None, (
        "the first paint came back blocked, so the 3 000 ms budget guarded nothing this run: "
        f"{first}\n{result.stdout}\n{result.stderr}")
    assert first["measured"] < 3_000.0


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
