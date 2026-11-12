#!/usr/bin/env python3
"""Measure what the index-time complexity pass (`e8.4`) costs, and gate the answer.

ADR-007 §9.7 decided that complexity is counted during the index run, in the pass that already has the
bytes in hand, rather than later by an analyzer re-reading the contents through the cache. The ticket that
closed it owes a number for that decision, and this tool produces it: the **same binary**, the same
repository, two configurations —

| arm | configuration | what it measures |
|---|---|---|
| `declined` | `extract.complexity.max_run_ms = 0` | the index with the complexity pass reading nothing at all |
| `shipped` | no `[extract.complexity]` keys at all, i.e. the shipped defaults | the index as it ships |

`declined` is not an older build of fathom with the pass deleted. It is this build, with the pass asked to
decline everything, which is the comparison that isolates the thing the ticket is about: it holds the
binary, the store schema, the columns being written and the reason-coding fixed, and varies only whether
file *contents* are read. Every other way of drawing the before/after line — this commit against its
parent, two checkouts, two machines — also varies the spine, the grammars and the machine, and then
attributes all of it to the pass.

Two numbers per arm, both taken the way `e7.2` took them: cold wall clock from just before the fork to the
child's exit, and `VmHWM` sampled while the child lives (`tools/mvp_acceptance.run_measured`, which is
where the reasoning about why a peak read from a corpse is zero lives). And one extra per arm: the run's
own `complexity_fill` object, because a cost measurement that cannot show *what the pass did* — rows,
bytes inflated, refusals, which ceiling stopped it — is a delta with no explanation attached.

The gate

    python3 tools/complexity_cost.py --repo REPO --check

re-measures and fails when the overhead exceeds what `e8.4` declared, which is written into the committed
report at `gate` so the number lives with the measurement rather than in a workflow file. The declaration
is deliberately in **relative** terms — a percentage of index time and an absolute of peak RSS — because a
cold index's absolute cost belongs to the machine and the repository, and a gate that forgets that goes red
on a busy runner rather than on a regression.

    python3 tools/complexity_cost.py --repo . --reps 2 \
        --corpus-note "this repository: the checkout the workflow is running" \
        --out benchmarks/reports/e8-4-index-time-complexity.json
    python3 tools/complexity_cost.py --repo . --reps 2 \
        --corpus-note "this repository: the checkout the workflow is running" \
        --report benchmarks/reports/e8-4-index-time-complexity.json --check

    python3 tools/complexity_cost.py --audit \
        --report benchmarks/reports/e8-4-index-time-complexity.json

`--check` re-measures and then asks whether the committed report describes *this* experiment — same
repository, same named corpus, same two arm configurations, same gate — and it does not compare wall clocks,
because two cold indexes on two machines are two machines and a gate that fires on a busy runner is a gate
that gets muted. `--audit` measures nothing at all: it re-derives a report's headline from that report's own
arm medians, which is the half that can run in a test suite on a machine that cannot spare four minutes to a
cold index.

This tool never clones: like `tools/perf_realrepo.py`, it measures a corpus somebody put somewhere, because
a benchmark that re-clones has a corpus that changes on its own. It **does** write two things into the
repository it measures — it removes the store and the payload cache to make each run cold, and it swaps
`fathom.toml` for the duration of a run to set the arm, restoring the config file afterwards byte for byte
(including on a crash, which is what a `finally` is for; a `SIGKILL` in the middle of a run leaves the arm's
config behind, and that is the one thing to know before pointing it at something you care about). It deletes
only `index.sqlite3*` and `objects/`, never the rest of `.fathom/`, because some of what lives there is
tracked. Point it at a checkout you own.

Exit: 0 measured and inside the gate (or `--out`, which only writes), 1 measured and over the gate or the
committed report does not reproduce, 2 the repository or the binary is not what the tool needs.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import mvp_acceptance as M  # noqa: E402  (run_measured: the wall clock and the VmHWM sampler)

#: The committed report, and the ticket whose cost it declares.
REPORT = ROOT / "benchmarks" / "reports" / "e8-4-index-time-complexity.json"
TICKET = "reco-l9bz.4"

#: What the two arms write into `fathom.toml`. `declined`'s zero is the spelling the config reference
#: documents: `0 >= 0` declines the first row, so the pass runs, fills the columns with reasons, and
#: inflates nothing. `shipped` writes no table at all — the defaults, not a second copy of them.
ARM_CONFIG = {
    "declined": "[extract.complexity]\nmax_run_ms = 0\n",
    "shipped": "",
}

#: The order the arms are measured and reported in. Declined first, so a failure in the second arm still
#: leaves a first arm on disk in `--out`.
ARM_ORDER = ("declined", "shipped")

#: What `e8.4` declared its pass is allowed to cost, in the report's `gate` block. The reasoning belongs
#: here because this is the file that produces the number it is judged on:
#:
#: * **index time, +40 %.** The pass is one inflate and one parse per *source* file row. A run whose
#:   contents are mostly vendored or binary pays almost nothing extra — on the 5 501-commit `aws-sdk-go`
#:   history, where 46 785 of 66 885 file rows are not source at all, the overhead was +12.8 % — and a run
#:   over a history that is mostly hand-written source pays for the parse it is now doing: +15.7 % on this
#:   repository, whose own graph pass is by then the larger share of the run. 40 %
#:   is the second case with room for a noisy runner: on the two repositories this ticket measured the
#:   overhead landed under it while the pass was still reading real content, and the pass cannot be made to
#:   cost more than its own ceilings allow.
#: * **peak RSS, +256 MiB.** The pass holds one blob at a time — `extract.complexity.max_file_bytes`, 2 MiB
#:   by default — so its own footprint is a couple of megabytes. What it adds to peak RSS is the allocator
#:   keeping the largest blob it inflated and the parse trees built from it: a per-file quantity that does
#:   not grow with history. Measured across the two corpora and repeated runs of each, that delta ranged from
#:   −4.5 MiB (yes: down, on a run that read 94 MB) to +131.8 MiB, which is allocator behaviour rather than a
#:   cost centre, so the allowance sits above the observed spread instead of on top of it — a gate that fires
#:   on a runner's malloc is a gate people mute, and this one is here to catch the pass starting to hold
#:   whole histories.
GATE = {"max_index_time_over_pct": 40.0, "max_peak_rss_over_mib": 256.0}

#: How much a re-measurement may differ from the committed report before `--check` says the report is
#: lying rather than that the machine is busy. Generous on purpose: this is a check that the *cost story*
#: still holds, not a byte-for-byte reproduction of a wall clock.
REPRODUCE_PCT = 50.0

#: A `[extract.complexity]` table already in the repository's own config, which the arm would collide with.
TABLE_HEADER = re.compile(r"^\s*\[extract\.complexity\]\s*$")


class Measured(Exception):
    """A measurement that cannot be made, with the sentence that says why."""


def git(repo: Path, *args: str) -> str:
    """A `git` read for the report's own facts. Empty string when the repo will not answer."""
    proc = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True)
    return proc.stdout.strip() if proc.returncode == 0 else ""


def facts(repo: Path) -> dict:
    """What is being measured, so the report can be identified later without guessing."""
    commits = git(repo, "rev-list", "--count", "HEAD")
    files = git(repo, "ls-files").splitlines()
    head = git(repo, "rev-parse", "HEAD")
    shallow = (repo / ".git" / "shallow").exists()
    return {
        "path": str(repo),
        "name": repo.name,
        "head": head,
        "head_subject": git(repo, "log", "-1", "--format=%s")[:120],
        "commits": int(commits) if commits.isdigit() else None,
        "tracked_files": len(files),
        "shallow_clone": shallow,
        "git_dir_mib": round(sum(f.stat().st_size for f in (repo / ".git").rglob("*")
                                  if f.is_file()) / 1048576, 1),
    }


def strip_complexity_table(text: str) -> str:
    """`fathom.toml` with any `[extract.complexity]` table removed, everything else untouched.

    The arms have to differ in exactly one thing, so a repository that already set its own ceilings has
    them lifted for the measurement rather than shadowed by an appended duplicate table — which TOML
    refuses to parse, and which `fathom` would report as "config ignored" while quietly measuring the
    defaults. Removing the section is done line-wise against the parsed document, so a `#` comment that
    mentions the table name is not mistaken for a header.
    """
    try:
        parsed = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        # Not parseable, so there is no `[extract.complexity]` table in it either; the arm's own text will
        # be parsed by `fathom`, which will report the real problem in its own words.
        return text
    if "extract" not in parsed or "complexity" not in parsed.get("extract", {}):
        return text
    out: list[str] = []
    dropping = False
    for line in text.splitlines():
        if line.lstrip().startswith("[") and not line.lstrip().startswith("#"):
            dropping = bool(TABLE_HEADER.match(line))
        if not dropping:
            out.append(line)
    return "\n".join(out) + ("\n" if out else "")


class Arm:
    """One configuration, measured over N cold runs."""

    def __init__(self, name: str, repo: Path, binary: str, reps: int) -> None:
        self.name = name
        self.repo = repo
        self.binary = binary
        self.reps = reps
        self.config = repo / "fathom.toml"
        self.backup: bytes | None = None
        self.backed_up = False

    def __enter__(self) -> "Arm":
        original = self.config.read_bytes() if self.config.exists() else None
        self.backup = original
        base = strip_complexity_table((original or b"").decode("utf-8"))
        text = base + ("\n" if base and not base.endswith("\n") else "") + ARM_CONFIG[self.name]
        # Both arms get the repository's own config, minus any `[extract.complexity]` table in it, plus the
        # arm's own table or nothing. Deleting the file for the `shipped` arm instead — which is what this
        # first did, in the name of "the defaults" — is a confound with a table of its own: a repository's
        # `fathom.toml` says things about the graph, the layers and the caches, so the two arms would then
        # differ in every one of those and the delta would be about the config file rather than the pass.
        self.config.write_text(text)
        self.backed_up = True
        return self

    def __exit__(self, *_exc: object) -> bool:
        if not self.backed_up:
            return False
        if self.backup is None:
            self.config.unlink(missing_ok=True)
        else:
            self.config.write_bytes(self.backup)
        self.backed_up = False
        return False

    def measure_once(self) -> tuple[float, int, dict]:
        """One cold run: (wall ms, peak RSS KiB, the run's own JSON report).

        The JSON is read back from the child's own stdout — a handle passed into
        `mvp_acceptance.run_measured`, which times the child while it writes — because the pass's numbers
        have to come from a *cold* run: a second, warm run would report an empty ledger, and the report
        would say the pass read nothing on an index that had just read 122 MiB.
        """
        # Only what the index owns. `.fathom/` also holds things a repository may *track* — `fathom` itself
        # tracks `.fathom/reviews/ledger.md`, and a `rm -rf .fathom` here deleted it out from under `git` on
        # this tool's first run — so a cold start removes the store file, its sidecars and the payload cache,
        # and leaves the rest of the directory alone.
        fathom_dir = self.repo / ".fathom"
        for candidate in fathom_dir.glob("index.sqlite3*"):
            candidate.unlink(missing_ok=True)
        shutil.rmtree(fathom_dir / "objects", ignore_errors=True)
        with tempfile.TemporaryFile() as handle:
            wall_ms, peak_kib, code = M.run_measured(
                [self.binary, "index", "--json"], self.repo, stdout=handle)
            if code != 0:
                raise Measured(
                    f"`{self.name}` arm: `fathom index --json` exited {code}; the pass may have refused a "
                    f"store it was asked to fill (see the run's own output)")
            handle.seek(0)
            text = handle.read().decode("utf-8")
        return wall_ms, peak_kib, (json.loads(text) if text.strip() else {})


def host() -> dict:
    """The machine the numbers came from, because a cold index belongs to its runner."""
    return {
        "system": platform.system(),
        "release": platform.release(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "cpu_count": os.cpu_count(),
        "memtotal_gib": _memtotal_gib(),
    }


def _memtotal_gib() -> float | None:
    try:
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                return round(int(line.split()[1]) / 1048576, 1)
    except OSError:
        pass
    return None


def measure(repo: Path, binary: str, reps: int) -> dict:
    """Both arms, interleaved, and the overhead between them.

    **The arms alternate, and that is the measurement.** A cold index reads the whole packfile, so the first
    arm of a pair runs against a cold page cache and the second runs against a warm one: measured always in
    the same order, the second arm is handed a faster machine and the delta the report is built on is partly
    an ordering artifact. `e8.4` caught it this way — one run reported the pass making a 5 501-commit index
    *faster* by 11.9 %, which is not a thing a parse does. Each rep runs the pair in the opposite order, the
    overhead is the median of the per-rep ratios, and a machine that drifts during a measurement shows up as
    scatter in `per_rep_pct` rather than as a sign error in the headline number.
    """
    repo = repo.resolve()
    if not (repo / ".git").exists():
        raise Measured(f"{repo} is not a repository")
    if not Path(binary).exists():
        raise Measured(f"{binary} does not exist; build it with `cargo build --release -p fathom-cli`")
    version = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()

    samples: dict[str, list[tuple[float, int]]] = {name: [] for name in ARM_ORDER}
    reports: dict[str, dict] = {}
    # Every run of an arm has to look like that arm's configuration, not merely the last one. This is the
    # only thing standing between a swapped configuration — the two arms measuring each other's config — and
    # a report with a confident, wrong delta in it, and it costs nothing: the declined arm's own ledger says
    # whether it read anything.
    proofs: dict[str, list[bool]] = {name: [] for name in ARM_ORDER}
    ratios: list[float] = []
    for rep in range(reps):
        order = list(ARM_ORDER) if rep % 2 == 0 else list(reversed(ARM_ORDER))
        for name in order:
            with Arm(name, repo, binary, 1) as arm:
                wall_ms, peak_kib, report = arm.measure_once()
            passed = report.get("complexity_fill") or {}
            if name == "declined":
                proofs[name].append(bool(passed) and passed.get("bytes_read") == 0
                                    and passed.get("declined") is not None)
            else:
                proofs[name].append(bool(passed) and (passed.get("bytes_read") or 0) > 0)
            samples[name].append((wall_ms, peak_kib))
            reports[name] = report
        base = samples["declined"][-1][0] or 1.0
        ratios.append((samples["shipped"][-1][0] - base) / base * 100.0)

    arms: dict[str, dict] = {}
    for name in ARM_ORDER:
        walls = [wall for wall, _peak in samples[name]]
        peaks = [peak for _wall, peak in samples[name]]
        report = reports[name]
        arms[name] = {
            "wall_ms_samples": [round(w, 1) for w in walls],
            "wall_ms_median": round(statistics.median(walls), 1),
            "wall_ms_store_side": report.get("elapsed_ms"),
            "phases_ms": report.get("phases_ms"),
            "peak_rss_mib": round(max(peaks) / 1024.0, 1),
            "pass": report.get("complexity_fill") or {},
            # The arm's whole identity: a report that does not say which config produced it cannot be
            # re-measured, only re-guessed.
            "config": ARM_CONFIG[name],
            "proved": all(proofs[name]),
        }
    declined, shipped = arms["declined"], arms["shipped"]
    over_ms = shipped["wall_ms_median"] - declined["wall_ms_median"]
    over_pct = statistics.median(ratios)
    over_rss = round(shipped["peak_rss_mib"] - declined["peak_rss_mib"], 1)
    passed = {"declined": all(proofs["declined"]), "shipped": all(proofs["shipped"])}
    return {
        "ticket": TICKET,
        "measured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "binary": {"path": str(binary), "version": version},
        "host": host(),
        "repository": facts(repo),
        "reps": reps,
        "arms": arms,
        "overhead": {
            "index_time_pct": round(over_pct, 1),
            "index_time_ms": round(over_ms, 1),
            "per_rep_pct": [round(ratio, 1) for ratio in ratios],
            "peak_rss_mib": over_rss,
            "declined_proved": passed["declined"],
            "shipped_proved": passed["shipped"],
        },
        "gate": dict(GATE),
    }


def check(result: dict, committed: dict) -> list[str]:
    """The gate, and the reproduction check. A list of failures, empty when it passes."""
    problems: list[str] = []
    gate = committed.get("gate") or dict(GATE)
    overhead = result["overhead"]
    # Either arm declining to do its job is a broken measurement, not a cheap index.
    if not overhead["declined_proved"]:
        problems.append("the `declined` arm did not read nothing: the arms are not the pair this tool claims "
                        "to measure, so no overhead number from it means anything")
    if not overhead["shipped_proved"]:
        problems.append("the `shipped` arm read no file contents, so there is no pass cost to gate")
    limit_pct = gate.get("max_index_time_over_pct", GATE["max_index_time_over_pct"])
    if overhead["index_time_pct"] > limit_pct:
        problems.append(f"the complexity pass added {overhead['index_time_pct']:.1f} % of index time "
                        f"(budget ≤ {limit_pct} %), declared by {TICKET}")
    limit_mib = gate.get("max_peak_rss_over_mib", GATE["max_peak_rss_over_mib"])
    if overhead["peak_rss_mib"] > limit_mib:
        problems.append(f"the complexity pass added {overhead['peak_rss_mib']:.1f} MiB of peak RSS "
                        f"(budget ≤ {limit_mib} MiB), declared by {TICKET}")
    # The committed report is the story; this is the part of the story that says it is still the same story.
    before = committed.get("repository") or {}
    now = result["repository"]
    if before.get("name") and before["name"] != now["name"]:
        problems.append(f"the committed report measured `{before['name']}` and this is `{now['name']}`; "
                        "point --repo at the repository the report declares, or rewrite the report")
    # Commit drift is allowed to be ordinary: a report is not re-measured on every commit, and a gate that
    # demanded one would be muted within a week. A *large* drift is not ordinary — it means the corpus
    # being measured is not the corpus being gated against.
    old_commits, new_commits = before.get("commits"), now["commits"]
    if old_commits and new_commits and abs(new_commits - old_commits) / old_commits * 100.0 > 25.0:
        problems.append(f"the committed report measured {old_commits} commits and this repository has "
                        f"{new_commits}; the corpus moved, so re-measure and commit the report")
    # And a named corpus has to stay the named corpus. This tool never clones, which is the only reason its
    # numbers are comparable across machines at all — so the report carries the sentence describing what was
    # cloned, and `--check` refuses a run against a corpus that quietly became something else.
    if before.get("corpus") != now.get("corpus"):
        problems.append(f"the committed report names its corpus as {before.get('corpus')!r} and this run was "
                        f"told {now.get('corpus')!r}; a corpus that changed underneath the report is a "
                        "benchmark that changed without saying so")
    # What `--check` is for: proving the committed report describes *this* experiment, so its numbers are
    # the ones the gate is about. It deliberately does not compare the wall clocks. Two cold indexes on two
    # machines are two different machines, and a gate that fires on a busy runner is a gate that gets muted
    # inside a week; `e8-2`'s harness says the same thing about timings for the same reason.
    for name in ARM_ORDER:
        was = (committed.get("arms") or {}).get(name) or {}
        if "config" in was and was["config"] != ARM_CONFIG[name]:
            problems.append(f"the `{name}` arm's configuration changed since the committed report was "
                            "measured; re-measure and commit the report, because the delta between two "
                            "different experiments is not a measurement of anything")
    if committed.get("gate") and committed["gate"] != result["gate"]:
        problems.append(f"the committed report's gate {committed['gate']} is not the gate this tool carries "
                        f"({result['gate']}); the allowance is a claim in the tool, not in the report")
    # And the fresh measurement's own numbers have to be self-consistent, which is the cheapest way to
    # notice a report generated by a run whose arms did something other than what they were told.
    problems.extend(audit(result, label="this run"))
    return problems


def audit(report: dict, *, label: str = "the committed report") -> list[str]:
    """A report's own arithmetic, re-derived from its own medians. No measurement, no build.

    This is the half of the gate that can run anywhere, and it is here because a report whose headline does
    not follow from its own numbers is worse than no report: it is a number with authority over people who
    cannot check it. The pytest contract test runs it on both committed reports on every commit.
    """
    problems: list[str] = []
    arms = report.get("arms") or {}
    if set(arms) != set(ARM_ORDER):
        return [f"{label} does not carry both arms ({sorted(arms)})"]
    over = report.get("overhead") or {}
    declined, shipped = arms["declined"], arms["shipped"]
    for name, arm in arms.items():
        passed = arm.get("pass") or {}
        if not passed:
            problems.append(f"{label}: the `{name}` arm recorded no complexity pass at all")
            continue
        for key in ("rows", "measured", "bytes_read", "ms", "declined", "ceilings"):
            if key not in passed:
                problems.append(f"{label}: the `{name}` arm's pass block is missing `{key}`")
        ceiling = (passed.get("ceilings") or {}).get("max_run_ms")
        if name == "declined" and ceiling not in (0, None):
            problems.append(f"{label}: the `declined` arm was not declined — its `max_run_ms` is {ceiling}")
        if name == "declined" and (passed.get("bytes_read") or 0) != 0:
            problems.append(f"{label}: the `declined` arm read {(passed.get('bytes_read') or 0):,} bytes, "
                            "which is not the arm this tool claims to measure")
        if name == "shipped" and not arm.get("proved", True):
            problems.append(f"{label}: the `shipped` arm read nothing, so there was no pass cost to measure")
    if declined.get("wall_ms_median") and shipped.get("wall_ms_median"):
        again = (shipped["wall_ms_median"] - declined["wall_ms_median"])
        if abs(again - (over.get("index_time_ms") or 0.0)) > 0.11:
            problems.append(f"{label}: `index_time_ms` says {over.get('index_time_ms')} and the arm medians "
                            f"say {again:.1f}")
    if not over.get("declined_proved") or not over.get("shipped_proved"):
        problems.append(f"{label}: one of the arms did not do the job it was measured for")
    gate = report.get("gate") or GATE
    if (over.get("index_time_pct") or 0.0) > gate["max_index_time_over_pct"]:
        problems.append(f"{label}: +{over.get('index_time_pct')} % of index time is over the declared "
                        f"{gate['max_index_time_over_pct']} %")
    if (over.get("peak_rss_mib") or 0.0) > gate["max_peak_rss_over_mib"]:
        problems.append(f"{label}: {over.get('peak_rss_mib')} MiB added to peak RSS is over the declared "
                        f"{gate['max_peak_rss_over_mib']} MiB")
    return problems


    return problems


def render(result: dict) -> str:
    """The same story in lines, for the CI log and for a person."""
    repo = result["repository"]
    print(f"{repo['name']}: {repo['commits']:,} commits, {repo['tracked_files']:,} tracked files "
          f"{'(shallow clone)' if repo['shallow_clone'] else ''}")
    for name in ARM_ORDER:
        arm = result["arms"][name]
        passed = arm["pass"] or {}
        print(f"  {name:<9} {arm['wall_ms_median']:>10,.1f} ms   peak {arm['peak_rss_mib']:>7,.1f} MiB"
              f"   pass: {passed.get('rows', 0):,} rows, "
              f"{(passed.get('bytes_read') or 0) / 1048576:,.1f} MiB read, "
              f"{passed.get('ms', 0):,} ms"
              + (f", stopped by {passed['declined']}" if passed.get("declined") else ""))
    over = result["overhead"]
    gate = result["gate"]
    print(f"  overhead  +{over['index_time_pct']:.1f} % index time "
          f"(budget ≤ {gate['max_index_time_over_pct']:.0f} %), "
          f"+{over['peak_rss_mib']:.1f} MiB peak RSS (budget ≤ {gate['max_peak_rss_over_mib']:.0f} MiB)")


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", default=".", help="the repository to measure (it gets a cold store and a "
                                               "swapped `fathom.toml` for the duration)")
    ap.add_argument("--bin", default=str(ROOT / "target" / "release" / "fathom"),
                    help="the `fathom` binary to measure (default: target/release/fathom)")
    ap.add_argument("--reps", type=int, default=2, help="cold runs per arm (default 2; median wall, worst RSS)")
    ap.add_argument("--out", default=None, help="write this report (the committed one, normally)")
    ap.add_argument("--report", default=str(REPORT), help="the committed report `--check` judges against")
    ap.add_argument("--check", action="store_true", help="measure, then judge against the committed report")
    ap.add_argument("--corpus-note", default=None,
                    help="how the corpus was obtained, recorded verbatim in the report — required honesty "
                         "for any repository this tool did not clone itself and therefore cannot name")
    ap.add_argument("--audit", action="store_true", help="re-derive the committed report's own arithmetic "
                                                        "and say whether it hangs together; no measurement, "
                                                        "no build")
    args = ap.parse_args(argv[1:])

    if args.audit:
        committed = json.loads(Path(args.report).read_text())
        problems = audit(committed)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        if not problems:
            over = committed["overhead"]
            print(f"  {committed['repository']['name']}: {committed['repository']['commits']:,} commits, "
                  f"+{over['index_time_pct']} % index time, {over['peak_rss_mib']:+.1f} MiB peak RSS — the "
                  "report's own numbers agree with it")
        return 1 if problems else 0

    try:
        result = measure(Path(args.repo), args.bin, args.reps)
        if args.corpus_note:
            result["repository"]["corpus"] = args.corpus_note
    except (Measured, FileNotFoundError, json.JSONDecodeError) as err:
        print(f"complexity cost: {err}", file=sys.stderr)
        return 2
    render(result)

    if args.out:
        Path(args.out).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
        print(f"  wrote {args.out}")
    if not args.check:
        return 0
    committed_path = Path(args.report)
    if not committed_path.exists():
        print(f"complexity cost: {committed_path} does not exist; there is nothing to gate against",
              file=sys.stderr)
        return 1
    committed = json.loads(committed_path.read_text())
    problems = check(result, committed)
    for problem in problems:
        print(f"complexity cost: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"  gate: inside what {TICKET} declared, and the committed report reproduces")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
