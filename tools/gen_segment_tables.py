#!/usr/bin/env python3
"""Render spike `s2`'s measured tables from the committed report, and fail on drift.

`docs/concepts/episode-segmentation.md` and ADR-003 §18 carry the numbers that closed ADR-003 §4.4 and
§13.7 — the boundary precision and recall of the candidate rule sets, the `episode.time_gap_minutes`
sweep, the signal census, and the gate results. Every one of them comes from one run of one harness,
committed as `benchmarks/reports/s2-segmentation-accuracy.json`, and a table of numbers copied by hand
out of a JSON file is a table that quietly keeps its old numbers the day the corpus changes. So they are
generated, the same way ADR-003's other normative tables are (see `tools/gen_adr_tables.py`):

    python3 tools/gen_segment_tables.py write   # regenerate both documents in place
    python3 tools/gen_segment_tables.py check   # exit 1 if the documents and the report disagree

Blocks are delimited by `<!-- BEGIN GENERATED: name -->` / `<!-- END GENERATED: name -->`. The concept
document renders all six blocks; ADR-003 §18 renders the two it quotes — the strategy table and the
gate table. A block no document renders is an error, because that means the generator and the documents
disagree about what exists. Nothing here computes anything: every cell is read out of the report, and
`cargo run -p fathom-analyze --example s2_spike -- --check` is what proves the report still matches the
corpus.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s2-segmentation-accuracy.json"
CONCEPT = ROOT / "docs" / "concepts" / "episode-segmentation.md"
ADR = ROOT / "docs" / "adr" / "0003-episodes.md"

STRATA = ("real", "fixture", "authored")
FILES = {
    "real": "`fathom-self.json`",
    "fixture": "`fixture-*.json` (13 scenarios)",
    "authored": "`authored-*.json` (8 histories)",
}
CLOCKS = {
    "real": "real author timestamps",
    "fixture": "synthetic (`seq / 1000` seconds) — not a clock",
    "authored": "hand-chosen from the real turnaround distribution",
}


def fmt(value: float, places: int = 3) -> str:
    return f"{value:.{places}f}"


def strategy(report: dict, sid: str) -> dict:
    for row in report["strategies"]:
        if row["id"] == sid:
            return row
    raise KeyError(f"the report has no strategy {sid!r}")


def corpus(report: dict) -> str:
    totals = report["corpus"]["totals"]
    by = totals["by_provenance"]
    lines = [
        "| stratum | files | turns (commits) | pathologies | commits with turn evidence | clocks |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for provenance in STRATA:
        row = by[provenance]
        lines.append(
            f"| `{provenance}` | {FILES[provenance]} | {row['turns']} ({row['commits']}) "
            f"| {row['pathologies']} | {row['commits_with_turn_evidence']} | {CLOCKS[provenance]} |"
        )
    lines.append(
        f"| **pooled** | {totals['histories']} histories | **{totals['turns']} ({totals['commits']})** "
        f"| **{totals['pathologies']}** | {sum(v['commits_with_turn_evidence'] for v in by.values())} | — |"
    )
    sizes = totals["turn_sizes"]
    turns = totals["turns"]

    lines.append("")
    lines.append(
        "Turn shapes, which decide what any boundary rule can do: "
        + ", ".join(
            f"{count / turns:.1%} of turns are {n} commit" + ("" if n == "1" else "s")
            for n, count in ((k, sizes[k]) for k in sorted(sizes, key=lambda k: int(k.split()[0])))
        )
        + "; by author kind the corpus is "
        + ", ".join(f"{v} {k}" for k, v in sorted(totals["turn_kinds"].items()))
        + " turns — which is why the `author` rule is nearly inert (§6)."
    )
    return "\n".join(lines) + "\n"


def strategies(report: dict) -> str:
    lines = [
        "| strategy | rungs (left → right = highest → lowest priority) | P | R | **F1** | real F1 "
        "| pair F1 | turns split | turns absorbed | pathologies hidden |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for row in report["strategies"]:
        pooled, real = row["results"]["pooled"], row["results"]["real"]
        mark = "**" if row["id"] in ("D-shipped", "F-hook-qualified") else ""
        rungs = row["rungs"] if isinstance(row["rungs"], str) else " › ".join(row["rungs"])
        lines.append(
            f"| {mark}`{row['id']}`{mark} | {rungs or '*(no rule fires: never split)*'} "
            f"| {fmt(pooled['boundary_precision'])} | {fmt(pooled['boundary_recall'])} "
            f"| {mark}{fmt(pooled['boundary_f1'])}{mark} | {fmt(real['boundary_f1'])} "
            f"| {fmt(pooled['pair_f1'])} | {pooled['turns_split']}/{pooled['turns']} "
            f"| {pooled['turns_absorbed']}/{pooled['turns']} "
            f"| {pooled['pathologies_hidden_total']}/{pooled['pathologies_annotated']} |"
        )
    lines.append("")
    lines.append(
        "`D-shipped` is ADR-003 §4.2 as built (`e1.3`); `F-hook-qualified` is the recommendation. Pooled is "
        "the whole corpus; `real F1` is Fathom's own git history alone; `pair F1` is co-membership, the "
        "measure `e1.3` gates the corpus at. Harness-identity check against `fathom_ingest::segment::"
        f"segment()`: **{report['production_equality']['shipped_strategy_matches_segment']}** on all "
        f"{report['production_equality']['histories_compared']} histories "
        f"({len(report['production_equality']['differences'])} mismatched)."
    )
    return "\n".join(lines) + "\n"


def hides(report: dict) -> str:
    scored = {kind: row["scored_via"] for kind, row in report["pathology_kinds"].items()}
    lines = [
        "| strategy | hidden by split | hidden by merge | inherent to the definition | beyond "
        "segmentation | the hidden ones |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for sid in ("D-shipped", "E-token-above-gap", "F-hook-qualified"):
        row = strategy(report, sid)
        pooled = row["results"]["pooled"]
        named = "; ".join(
            f"`{h['history']}/{h['turn']}` ({h['kind']}, {scored.get(h['kind'], 'split')})"
            for h in row["hides"][:6]
        ) + ("; …" if len(row["hides"]) > 6 else "") or "none"
        lines.append(
            f"| `{sid}` | {pooled['pathologies_hidden_by_split']} "
            f"| {pooled['pathologies_hidden_by_merge']} "
            f"| {pooled['pathologies_inherent_to_definition']} "
            f"| {pooled['pathologies_beyond_segmentation']} | {named} |"
        )
    lines.append("")
    lines.append(
        "The report's `hides` list names every one of these with the commit, the turn, the paths and the "
        "annotator's reason. The two the recommendation still hides on the **real** stratum are both "
        "unclaimed commits landing a minute after a released turn: "
        "`e4a03667` (*contracts.yml: grep the keyprobe's real machine keys in the e0.6 step*) and "
        "`60d63449` (*chore: gitignore `.transcripts/` — session logs land in the working tree and are not "
        "artefacts*). No bead, no trailer, no ticket token, no clock signal: every strategy in this table "
        "reads both as part of the turn before them, and no rule that reads git alone can be written to do "
        "otherwise."
    )
    return "\n".join(lines) + "\n"


def gap_sweep(report: dict) -> str:
    shown = ("A-time-author", "D-shipped", "E-token-above-gap")
    grid: dict[int, dict[str, dict]] = {}
    for row in report["gap_sweep"]:
        if row["strategy"] in shown:
            grid.setdefault(row["gap_minutes"], {})[row["strategy"]] = row
    gaps = sorted(grid)
    argmax = {}
    for sid in shown:
        argmax[sid] = max(gaps, key=lambda g: grid[g][sid]["pooled"]["boundary_f1"])

    lines = [
        "| gap (min) | A: time+author pooled F1 | D-shipped pooled F1 | D-shipped real F1 "
        "| D-shipped pathologies hidden | D-shipped turns split | E (recommended) pooled F1 |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for gap in gaps:
        cells = []
        for sid, scope, key in (
            ("A-time-author", "pooled", "boundary_f1"),
            ("D-shipped", "pooled", "boundary_f1"),
            ("D-shipped", "real", "boundary_f1"),
            ("D-shipped", "pooled", "pathologies_hidden_total"),
            ("D-shipped", "pooled", "turns_split"),
            ("E-token-above-gap", "pooled", "boundary_f1"),
        ):
            value = grid[gap][sid][scope][key]
            text = fmt(value) if key.endswith("_f1") else str(value)
            if scope == "pooled" and key.endswith("_f1") and argmax[sid] == gap:
                text = f"**{text}**"
            cells.append(text)
        label = f"{gap} **← published default**" if gap == 30 else str(gap)
        lines.append(f"| {label} | " + " | ".join(cells) + " |")
    lines.append("")
    lines.append(
        "Bolded: each strategy's own pooled argmax — A at "
        + f"{argmax['A-time-author']} min, the shipped cascade at {argmax['D-shipped']} min, the "
        f"recommendation at {argmax['E-token-above-gap']} min. The published default is 30."
    )
    return "\n".join(lines) + "\n"


def census(report: dict) -> str:
    order = [
        ("hook(harness)", "harness states turn membership (the `e6.1` counterfactual)"),
        ("hook(commit)", "**`hook` as shipped** — raw `turn_id` compared unqualified, both sides required"),
        ("hook(commit,session-qualified)", "`hook` with ids qualified as `session:turn` (§9.1, D4's fix), both sides required"),
        ("trailer(Agent-Turn)", "`Agent-Turn:` trailer as §9.1's transport spells it: qualified, **and a trailer on one side alone reads as a boundary**"),
        ("trailer(Fathom-Episode)", "`Fathom-Episode:` trailer — §4.2 rung 3"),
        ("branch", "`branch` — §4.2 rung 4"),
        ("author", "`author` — §4.2 rung 5"),
        ("ticket-token", "ticket token in the subject — the rule §18.2 D1 adds"),
        ("time-gap", "`time-gap` at the published 30 — rung 6"),
        ("message-shape", "`message-shape` — rung 7"),
    ]
    pooled, real = report["signal_census"]["pooled"], report["signal_census"]["real"]
    lines = [
        "| signal, judged alone | true boundaries found, pooled | false boundaries fired, pooled "
        "| true boundaries found, real stratum |",
        "| --- | --- | --- | --- |",
    ]
    def totals(row: dict) -> tuple[int, int]:
        return (
            row["true_boundaries_found"] + row["true_boundaries_missed"],
            row["false_boundaries_fired"] + row["false_boundaries_withheld"],
        )

    for key, label in order:
        p, r = pooled[key], real[key]
        true_pool, false_pool = totals(p)
        real_true, _ = totals(r)
        lines.append(
            f"| {label} | {p['true_boundaries_found']} / {true_pool} "
            f"| {p['false_boundaries_fired']} / {false_pool} "
            f"| {r['true_boundaries_found']} / {real_true} |"
        )
    lines.append("")
    lines.append(
        "Judged alone means: this rule, no others, at the published gap, against "
        f"{totals(pooled['author'])[0]} true boundaries and {totals(pooled['author'])[1]} within-turn "
        f"adjacencies pooled — {totals(real['author'])[0]} and {totals(real['author'])[1]} of them on the "
        "real stratum. Three things to read off it. (1) Nothing fires a false boundary on turn evidence: "
        "every `0 / 60` in the middle column is TB6 holding. (2) The trailer row reads more boundaries than "
        "the `hook` rows because it is the only one that treats a trailer on *one* side as a boundary; that "
        "is sound exactly as long as the harness declares every agent commit, and unsound the moment it "
        "doesn't — which is why D5 files it as a vocabulary decision and not a free win. (3) A signal's "
        "share here is not its contribution to the cascade, where a higher-priority rule may already have "
        "answered; this is a census of what each rule *could* ever have known."
    )
    return "\n".join(lines) + "\n"


def gates(report: dict) -> str:
    lines = ["| gate | claim | measured | verdict |", "| --- | --- | --- | --- |"]
    for row in report["gates"]:
        # Lowercase the first word for table prose, unless it is an acronym: "ADR-003 rung 3" must not
        # become "aDR-003 rung 3".
        claim = row["claim"]
        first = claim.split(" ", 1)[0]
        if not (first.isupper() or first.rstrip(".").isupper()):
            claim = claim[0].lower() + claim[1:]
        measured = row["measured"]
        # A rate is a rate; a count is a count. `pathologies_hidden_total: 3.000` reads like a rounded
        # ratio, which is exactly the confusion the column exists to avoid.
        rate = any(token in row.get("metric", "") for token in ("f1", "precision", "recall"))
        if row["kind"] == "gap_argmax":
            text = f"{row['detail']['best_gap_minutes']} min (peak {measured:.3f})"
        elif rate:
            text = f"{measured:.3f}"
        elif isinstance(measured, (int, float)):
            text = f"{measured:g}"
        else:
            text = str(measured)
        verdict = "✅" if row["pass"] else "❌ as claimed"
        if not row["as_expected"]:
            verdict = "⚠️ **deviates from its claim**"
        lines.append(f"| `{row['id']}` | {claim} | {text} | {verdict} |")
    lines.append("")
    failing = [row["id"] for row in report["gates"] if not row["pass"]]
    lines.append(
        f"{len(failing)} of {len(report['gates'])} gates fail, each declared `expected: fail` before the "
        f"run: " + ", ".join(f"`{gid}`" for gid in failing) + ". Every gate came out as claimed "
        f"(`gates_all_as_expected: {str(report['gates_all_as_expected']).lower()}`)."
    )
    return "\n".join(lines) + "\n"


def blocks(report: dict) -> dict[str, str]:
    return {
        "s2_corpus": corpus(report),
        "s2_strategies": strategies(report),
        "s2_hides": hides(report),
        "s2_gap_sweep": gap_sweep(report),
        "s2_census": census(report),
        "s2_gates": gates(report),
    }


def render(text: str, available: dict[str, str]) -> tuple[str, set[str]]:
    """Substitute every block whose markers are present in this file."""
    used: set[str] = set()
    out = text
    for name, body in available.items():
        begin, end = f"<!-- BEGIN GENERATED: {name} -->", f"<!-- END GENERATED: {name} -->"
        if begin not in out or end not in out:
            continue
        head, rest = out.split(begin, 1)
        _old, tail = rest.split(end, 1)
        out = head + begin + "\n" + body + end + tail
        used.add(name)
    return out, used


def main(argv: list[str]) -> int:
    mode = argv[0] if argv else "check"
    if mode not in ("check", "write"):
        print(__doc__)
        return 2
    if not REPORT.exists():
        print("segmentation spike: no report at "
              f"{REPORT.relative_to(ROOT)}; run `cargo run -p fathom-analyze --example s2_spike`")
        return 1
    available = blocks(json.loads(REPORT.read_text()))
    problems: list[str] = []
    seen: set[str] = set()
    for path in (CONCEPT, ADR):
        text = path.read_text()
        rendered, used = render(text, available)
        seen |= used
        if mode == "write":
            if rendered != text:
                path.write_text(rendered)
                print(f"{path.relative_to(ROOT)}: updated")
        elif rendered != text:
            problems.append(f"{path.name}: generated block is out of date")
    unused = sorted(set(available) - seen)
    if unused:
        problems.append("no document renders: " + ", ".join(unused))
    if problems and mode == "check":
        for problem in problems:
            print("segmentation spike:", problem)
        return 1
    if mode == "check":
        print(f"segmentation spike: blocks in sync with {REPORT.relative_to(ROOT)} "
              f"({len(seen)} blocks rendered)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
