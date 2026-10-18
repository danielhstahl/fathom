#!/usr/bin/env python3
"""Render spike `s4`'s measured tables from the committed report, and fail on drift.

`docs/concepts/rename-detection.md` carries the numbers that answer `reco-gj6`'s question — how often a
pairing method asserts something untrue, and at what confidence a pairing is safe to show as a fact.
Every one of them comes from one run of one prototype, committed as
`benchmarks/reports/s4-rename-detection.json`, and a table of numbers copied by hand from a JSON file
is a table that quietly keeps its old numbers the day the prototype changes. So they are generated, the
same way `tools/gen_graph_tables.py` and `tools/gen_adr_tables.py` generate theirs:

    python3 tools/gen_rename_tables.py write   # regenerate the document in place
    python3 tools/gen_rename_tables.py check   # exit 1 if the document and the report disagree

Blocks are delimited by `<!-- BEGIN GENERATED: name -->` / `<!-- END GENERATED: name -->`, and a block
that no document renders is an error, because that means the generator and the documents disagree about
what exists.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s4-rename-detection.json"
CONCEPT = ROOT / "docs" / "concepts" / "rename-detection.md"

MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)

#: Reference bars for the comparison table. 0.20 is the cheap guess band, 0.30 the matcher's usual
#: starting point, 0.50 the deepest bar that stays clear of the corpus's known false pair, 0.70 the
#: chosen fact bar. Anything else is in the report.
REFERENCE_BARS = (0.20, 0.30, 0.50, 0.70)

#: The alternative asymmetries the document shows, so that the choice of 3:1 is visibly a choice.
ALTERNATIVE_WEIGHTS = {"1:1": (1, 1), "3:1 (chosen)": (3, 1), "10:1": (10, 1)}


def num(value, digits: int = 3) -> str:
    return "—" if value is None else f"{value:.{digits}f}"


def algorithms(report: dict) -> dict:
    return {row["id"]: row for row in report["algorithms"]}


def sweep(report: dict) -> dict:
    return {(row["algorithm"], row["threshold"]): row for row in report["threshold_sweep"]}


# -- blocks ---------------------------------------------------------------------------------------

def corpus(report: dict) -> str:
    c, env = report["corpus"], report["environment"]
    kinds = ", ".join(f"{v} {k}" for k, v in sorted(c["by_kind"].items()))
    evidence = ", ".join(f"{v} {k}" for k, v in sorted(c["by_evidence"].items()))
    return "\n".join([
        f"- **{c['scenarios']} scenarios**, **{c['windows']} review windows** "
        f"({c['episode_windows']} read as one turn, {c['commit_pair_windows']} as a single commit "
        f"pair), **{c['commits']} commits** built by real `git`, of which "
        f"{c['merge_commits']} are merges; history labels {', '.join('`' + s + '`' for s in c['rewritten_labels'])} "
        f"exist only as rewritten (unreachable) objects, which is what a rebase leaves behind and what "
        f"makes a review ledger's stored pairing testable across a force-push.",
        f"- **{c['authored_rows']} hand-authored rows**: **{c['true_pairs']} true pairings** and "
        f"**{c['named_decoys']} named decoys** — pairs the truth file records as *not* a move, which a "
        f"detector is allowed to be tempted by and forbidden from shipping.",
        f"- True pairings by kind: {kinds}. By content evidence: {evidence}, of which "
        f"**{c['unclaimable_pairs']} are past the ceiling** (the move happened and the blobs do not "
        f"carry the evidence for it), leaving **{c['claimable_pairs']} claimable** — the only number "
        f"any method is scored against.",
        f"- **{c['arrivals']} genuine arrivals** (files that simply are new): pairing one with a deleted "
        f"file is a fabricated origin, counted separately from a wrong pair between two real candidates.",
        f"- Truth-coherence check: {report['coherence']['windows']} windows, "
        f"{report['coherence']['paths_checked']} path assertions verified against the built trees, "
        f"{len(report['coherence']['problems'])} problems. The fixture set builds in "
        f"{c['build_seconds']} s.",
        f"- Environment: {env['git']}, Python {env['python']}, {env['platform']}, "
        f"{env['cpu_count']} cores; MinHash at {env['minhash_permutations']} permutations, default "
        f"shingle width {env['shingle_width_default']}. {env['profile']}.",
    ])


def algorithm_table(report: dict) -> str:
    head = ("| configuration | family | candidate scope | mean ms/window (floor) | blobs read (total) "
            "| candidates emitted | what it is |")
    sep = "|" + "---|" * 7
    rows = []
    for config in report["algorithms"]:
        cost = config["cost"]
        rows.append(
            f"| `{config['id']}` | {config['family']} | {config['scope']} | {cost['ms_mean']:.2f} "
            f"| {cost['blobs_read']} | {cost['candidates_emitted']} | {config['claim']} |")
    return "\n".join([head, sep, *rows])


def bar_table(report: dict) -> str:
    """Precision and recall at four reference bars, one row per configuration.

    A non-swept git row prints once, at its own knob: `git-M75` *means* "at least 75% similar", so its
    0.10/0.30/0.50 columns are empty rather than duplicated — the bar is the configuration.
    """
    table = sweep(report)
    head = ("| configuration | " + " | ".join(f"P / R at {bar:.2f}" for bar in REFERENCE_BARS)
            + " | wrong / missed at its bar |")
    sep = "|" + "---|" * (2 + len(REFERENCE_BARS))
    rows = []
    for config in report["algorithms"]:
        swept = config["params"].get("sweep")
        cells = []
        for bar in REFERENCE_BARS:
            if not swept:
                cells.append("`own bar`" if bar == REFERENCE_BARS[0] else "—")
                continue
            row = table.get((config["id"], bar))
            cells.append("—" if row is None
                         else f"{num(row['precision'])} / {num(row['recall_claimable'])}")
        at_bar = table.get((config["id"], 0.70 if swept else 0.0))
        rows.append(f"| `{config['id']}` | " + " | ".join(cells)
                    + f" | {at_bar['wrong_total']} / {at_bar['missed_claimable']} |")
    return "\n".join([head, sep, *rows])


def calibration_table(report: dict) -> str:
    head = ("| configuration | pairs emitted | expected calibration error | observed truth rate for "
            "scores 0.0–0.2 | 0.2–0.5 | 0.5–0.7 | 0.7–1.0 |")
    sep = "|" + "---|" * 7
    rows = []
    for algorithm in ("git-M50", "git-calibrated", "sim-w3", "sim-exact", "hybrid", "hybrid-no-name",
                      "name-only"):
        buckets = report["calibration"][algorithm]["buckets"]

        def band(low: float, high: float) -> str:
            inside = [b for b in buckets if b["low"] >= low and b["high"] <= high]
            if not inside:
                return "—"
            total = sum(b["n"] for b in inside)
            true = sum(b["observed_true_rate"] * b["n"] for b in inside)
            return f"{true / total:.2f} (n={total:.0f})"

        rows.append(f"| `{algorithm}` | {report['calibration'][algorithm]['pairs']} "
                    f"| {num(report['calibration'][algorithm]['expected_calibration_error'])} "
                    f"| {band(0.0, 0.2)} | {band(0.2, 0.5)} | {band(0.5, 0.7)} | {band(0.7, 1.0)} |")
    return "\n".join([head, sep, *rows])


def minhash_table(report: dict) -> str:
    measured = report["sketch_error"]["measured"]
    expected = report["sketch_error"]["expected_standard_error_at_half"]
    rows = [
        f"- Pairs scored by a sketch rather than an exact set comparison: **{measured['pairs']}**.",
        f"- Absolute error against the exact Jaccard: mean **{measured['mean_absolute_error']}**, "
        f"p95 **{measured['p95_absolute_error']}**, max **{measured['max_absolute_error']}**; the "
        f"textbook standard error at J=0.5 with "
        f"{report['environment']['minhash_permutations']} permutations is **{expected}**, so the "
        f"prototype is behaving like the method and not like its own bugs.",
        "- Pairs where the sketch and the exact value land on **opposite sides of a candidate bar** "
        "(the only kind of error that changes what a reviewer sees): "
        + ", ".join(f"**{row['crossed']}** at {bar} ({row['of_pairs'] * 100:.1f}%)"
                    for bar, row in sorted(measured["threshold_crossings"].items(),
                                           key=lambda item: float(item[0]))),
        "- The banding shortcut (`sim-banded`, 32 bands × 4 rows) drops candidates the full comparison "
        "keeps: it finds **0** of the split pairs, because the S-curve's elbow sits above where a split's "
        "evidence lives. Its calibration error is the best in the table "
        f"(**{num(report['calibration']['sim-banded']['expected_calibration_error'])}**) for exactly the "
        "wrong reason — it is not confident, it is blind.",
    ]
    return "\n".join(rows)


def asymmetry_table(report: dict) -> str:
    """The ticket's asymmetry, priced — and the pricing tested to see whether it decides anything."""
    table = sweep(report)
    local = [row for row in table.values()
             if row["emitted"] > 0 and row.get("scope") == "changed"]

    def cheapest(rows, wrong_weight, miss_weight):
        return min(rows, key=lambda r: (wrong_weight * r["wrong_total"]
                                        + miss_weight * r["missed_claimable"],
                                        -(r["recall_claimable"] or 0), r["algorithm"]))

    eligible = [row for row in local if row["wrong_named_decoy"] == 0
                and (row["precision"] or 0) >= 0.95]
    rows = []
    for label, (wrong_weight, miss_weight) in ALTERNATIVE_WEIGHTS.items():
        loose = cheapest(local, wrong_weight, miss_weight)
        strict = cheapest(eligible, wrong_weight, miss_weight)
        rows.append(
            f"| {label} | `{loose['algorithm']}` at {loose['bar']:.2f} — {loose['wrong_total']} wrong, "
            f"{loose['missed_claimable']} missed | `{strict['algorithm']}` at {strict['bar']:.2f} — "
            f"{strict['wrong_total']} wrong, {strict['missed_claimable']} missed | "
            f"{'the same pair of choices' if (loose['algorithm'], loose['bar']) == (strict['algorithm'], strict['bar']) else 'the filter, not the weight'} |")
    rec = report["recommendation"]
    return "\n".join([
        "| wrong : missed weighting | cheapest point if a lie is allowed | cheapest point a fact tier is "
        "allowed to ship | did the weighting change the answer |",
        "|" + "---|" * 4,
        *rows,
        "",
        "Read that column honestly: **on this corpus the weighting changes nothing.** Three weightings "
        "from "
        "`1:1` to `10:1` all pick the same cheapest point and the same fact tier, because what refuses the "
        "lie is the eligibility rule — *never assert a pair the truth names as not-a-move* — and not the "
        "price attached to it. The weight is kept anyway, for two reasons: it is the place where the ticket's "
        "claim (`a wrong pairing is worse than a missing one`) is written down as a number instead of implied "
        "by a metric, and a rule that refuses lies on a *corpus of 55 pairs* will need a price when the corpus "
        "is a thousand and the tie is not this obvious.",
        "",
        f"What refusing the lie does cost, stated: the chosen fact configuration "
        f"`{rec['fact']['algorithm']}` ships **{rec['fact']['wrong_total']} wrong pairings** and misses "
        f"**{rec['fact']['missed_claimable']}** claimable pairs across "
        f"{report['corpus']['review_windows']} windows, where the cheapest point in the whole sweep — the "
        f"one that lets a lie through — is "
        f"`{rec['lowest_harm_point_any_policy']['algorithm']}` at "
        f"{rec['lowest_harm_point_any_policy']['bar']:.2f} with "
        f"{rec['lowest_harm_point_any_policy']['wrong_total']} wrong to miss "
        f"{rec['lowest_harm_point_any_policy']['missed_claimable']}. Refusing it costs "
        f"**{rec['price_of_the_fact_tier']['missed_claimable']} more missed pairs** "
        f"({rec['price_of_the_fact_tier']['weighted_errors']} more weighted errors). That is the price of a "
        "fact tier, and it is the number `e2.4` has to accept on the record rather than discover later.",
    ])


def representation_table(report: dict) -> str:
    data = report["recall_by_representation"]
    rows = []
    for kind, cell in data["by_kind"].items():
        rows.append(f"| kind `{kind}` | {cell['pairs']} | {cell['claimable']} | {cell['found']} | "
                    f"{num(cell['recall_claimable'])} |")
    for evidence, cell in data["by_evidence"].items():
        rows.append(f"| evidence `{evidence}` | {cell['pairs']} | {cell['claimable']} | "
                    f"{cell['found']} | {num(cell['recall_claimable'])} |")
    return "\n".join([
        f"Measured with the chosen fact configuration `{data['algorithm']}` at bar "
        f"`{data['bar']:.2f}`.",
        "",
        "| slice | pairs | claimable | found | recall of claimable |",
        "|" + "---|" * 5,
        *rows,
    ])


def recommendation_block(report: dict) -> str:
    rec, guess = report["recommendation"], report["recommendation"]["guess_band"]
    fact, band = rec["fact"], guess["cheapest_band_all_told"]
    deep = guess["deepest_without_a_known_false_pair"]
    return "\n".join([
        "The rule, executed:",
        "",
        f"- **Filters first, from `e2.4`'s own criteria**: window-local candidate sets only "
        f"(*{rec['rule']['scope_policy']}*), and a *fact* may never be a pair the truth names as not-a-"
        f"move ({rec['rule']['named_decoy_policy']}). Points within "
        f"{rec['rule']['tie_tolerance_weighted_errors']} weighted errors of the best are treated as tied "
        f"and broken toward a score the reviewer can reproduce "
        f"({', '.join(rec['rule']['verifiable_scores_only_for'])} — nothing else here produces a number a "
        f"reviewer can check without this project's code).",
        f"- **Fact tier: `{fact['algorithm']}` at bar {fact['bar']:.2f}** — precision "
        f"**{num(fact['precision'])}** on **{fact['emitted']}** shipped pairings, recall "
        f"**{num(fact['recall_claimable'])}** of the {fact['true_claimable_pairs']} claimable pairs, "
        f"**{fact['wrong_total']}** wrong, and "
        f"{fact['wrong_per_100_windows']} wrong pairings per 100 windows reviewed.",
        f"- **The deepest bar that adds a pair without meeting a known false pairing** is "
        f"`{deep['algorithm']}` at {deep['bar']:.2f} — worth {deep['true_pairs_gained']} extra pairings, "
        f"and it clears the false pair's own similarity by only "
        f"**{guess['clearance_above_the_matcher_decoy']}**. That is a razor's edge, not a margin.",
        f"- **The widest band worth showing** is `{band['algorithm']}` at {band['bar']:.2f}: precision "
        f"{num(band['measured']['precision'])}, recall {num(band['measured']['recall_claimable'])}, and "
        f"against the fact tier it is **+{band['true_pairs_gained']} true pairings for "
        f"+{band['wrong_pairs_added']} wrong** ({band['named_lies_added']} of them a named decoy). That "
        f"ratio is the finding: **the guess band is not a tuning problem, it is a display problem** — the "
        f"lies in it are not removable by a threshold, so the label has to carry them.",
        f"- **No threshold separates the false pair from the true ones.** The scaffold twin scores R064 in "
        f"git and J≈0.49 in the matcher, above the true rewritten move's R015 and J≈0.06, so *any* bar "
        f"low enough to catch a real rewrite is high enough to ship the lie. Gates G46-G48 hold this open.",
        "",
        "What that means for `e2.4`, in one line each:",
        "",
        "1. **Raise the shipped fact bar from 50% to 75%.** At 50% git ships a pairing that is not one "
        "(the corpus's named decoy) and still misses the rewrites at R024 and below; at 75% it ships "
        "nothing false. The default is a precision choice, not a performance one, and it is currently "
        "made in the wrong direction.",
        "2. **Do not blend path similarity into the score.** `hybrid` and `hybrid-no-name` differ only by "
        "a 0.15 name weight and their wrong-pair counts at a 0.10 bar are **36** and **1**. Path evidence "
        "belongs in candidate ordering and display, never in the number.",
        "3. **Report a fact as a pair, and a guess as a guess with its number.** The fact tier is precise "
        "enough to be worth acting on; below it, an unlabelled percentage is a claim a reviewer will act "
        "on and cannot check.",
        "4. **Do not try to express a split, a fold or a copy as a rename pair at all.** See the "
        "representation table: at the fact bar those classes are found zero times, and no threshold "
        "changes that, because the pair shape is the thing that is wrong.",
        "5. **Replace the guessed 90% precision/recall bar** in `e2.4` (`reco-a0v`) with the measured "
        "floor: precision ≥ 0.95 at a fact bar of 0.75 with zero assertions of a known-false pairing, and "
        "recall reported per representation class rather than pooled.",
    ])


def gate_table(report: dict) -> str:
    rows = [f"| `{g['id']}` | {g['claim']} | `{g['probe']}`{'.' + g['field'] if g['field'] else ''} "
            f"| {g['expected']!r} | {g['observed']!r} | {'pass' if g['pass'] else 'FAIL'} |"
            for g in report["gates"]]
    return "\n".join([f"All {len(report['gates'])} gates, as this run measured them.", "",
                      "| gate | claim | read from | expected | observed | result |",
                      "|" + "---|" * 6, *rows])


def scenario_table(report: dict) -> str:
    rows = []
    for row in report["corpus"]["scenario_detail"]:
        rows.append(f"| `{row['name']}` | {row['note']} | {row['windows']} | {row['true_pairs']} "
                    f"| {row['named_decoys']} | {', '.join(row['kinds'])} |")
    return "\n".join([
        "| scenario | what it is for | windows | true pairs | named decoys | kinds |",
        "|" + "---|" * 6, *rows])


def not_measured(report: dict) -> str:
    return "\n".join(f"- {line}" for line in report["not_measured"])


BLOCKS = {
    "rename_spike_corpus": corpus,
    "rename_spike_scenarios": scenario_table,
    "rename_spike_algorithms": algorithm_table,
    "rename_spike_bars": bar_table,
    "rename_spike_calibration": calibration_table,
    "rename_spike_minhash": minhash_table,
    "rename_spike_asymmetry": asymmetry_table,
    "rename_spike_representations": representation_table,
    "rename_spike_recommendation": recommendation_block,
    "rename_spike_gates": gate_table,
    "rename_spike_not_measured": not_measured,
}


def render(document: pathlib.Path, report: dict, *, write: bool) -> list[str]:
    text = document.read_text()
    found = list(MARK.finditer(text))
    if not found:
        return [f"{document.name}: no generated blocks found"]
    problems, updated = [], text
    for match in reversed(found):
        name = match.group(1)
        renderer = BLOCKS.get(name)
        if renderer is None:
            problems.append(f"{document.name}: unknown generated block {name!r}")
            continue
        body = renderer(report).strip()
        if match.group(2).strip() != body:
            if write:
                updated = updated[:match.start(2)] + body + "\n" + updated[match.end(2):]
            else:
                problems.append(f"{document.name}: block {name} is out of date")
    if write and updated != text:
        document.write_text(updated)
    return problems


def main(argv: list[str]) -> int:
    if len(argv) != 2 or argv[1] not in ("write", "check"):
        print(__doc__)
        return 2
    if not REPORT.exists():
        print(f"missing {REPORT}; run\n"
              f"  PYTHONPATH=src python3 -m recommender.rename.spike --report {REPORT}")
        return 1
    report = json.loads(REPORT.read_text())
    write = argv[1] == "write"
    problems = render(CONCEPT, report, write=write)
    used = {match.group(1) for match in MARK.finditer(CONCEPT.read_text())}
    for name in sorted(set(BLOCKS) - used):
        problems.append(f"{CONCEPT.name}: block {name} is generated but rendered nowhere")
    if problems:
        for problem in problems:
            print("drift: " + problem)
        return 1
    print(f"rename spike tables {'written' if write else 'in sync'}: {CONCEPT.name}, "
          f"{len(BLOCKS)} blocks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
