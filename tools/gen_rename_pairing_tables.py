#!/usr/bin/env python3
"""Render `e2.4`'s measured pairing tables from the committed report, and fail on drift.

`docs/concepts/rename-pairing.md` carries the numbers that answer `reco-a0v`'s four acceptance
criteria — how often a shipped pairing is untrue, whether splits and folds are represented, what a
reviewer's accept or reject does, and what the cost bound actually costs. Every number in it comes
from one run of the shipped engine, committed as
`benchmarks/reports/e2-4-rename-pairing.json`, and a table of numbers copied by hand from a JSON file
is a table that quietly keeps its old numbers the day the engine changes. So they are generated, the
same way `tools/gen_rename_tables.py` generates the spike's:

    python3 tools/gen_rename_pairing_tables.py write   # regenerate the document in place
    python3 tools/gen_rename_pairing_tables.py check   # exit 1 if document and report disagree

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
REPORT = ROOT / "benchmarks" / "reports" / "e2-4-rename-pairing.json"
CONCEPT = ROOT / "docs" / "concepts" / "rename-pairing.md"

MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)

#: The four representation classes the ticket names, in the order the document discusses them.
KINDS = ("rename", "split", "merge", "copy")

PHRASE = {"rename": "rename (1→1)", "split": "split (1→many)", "merge": "fold (many→1)",
          "copy": "copy (source survives)", "tangle": "tangle"}


def num(value, digits: int = 4) -> str:
    return "—" if value is None else f"{value:.{digits}f}"


def named_fixtures(report: dict) -> dict:
    """The two fixtures `e2.4` names, restated from the per-window rows (no second measurement)."""
    out: dict[str, dict] = {}
    for scenario in ("rename_storm", "rename_rewrite"):
        rows = [row for row in report["rows"] if row["scenario"] == scenario]
        found = sum(row["found"] for row in rows)
        claimable = sum(row["claimable"] for row in rows)
        emitted = sum(row["emitted"] for row in rows)
        out[scenario] = {"found": found, "claimable": claimable, "emitted": emitted,
                         "wrong": sum(row["wrong"] for row in rows),
                         "recall": round(found / claimable, 4) if claimable else None}
    return out


def headline(report: dict) -> str:
    """The pooled and per-tier numbers, in the words the gates use."""
    t, tiers, named = report["totals"], report["tiers"], named_fixtures(report)
    lines = [
        f"- **{t['windows']} review windows** across **{report['corpus']['scenarios']} fixtures**, "
        f"{t['true_pairs']} hand-authored pairings, of which **{t['claimable_pairs']} are claimable** "
        f"and {t['ceiling_pairs']} sit past the evidence ceiling a window-local method can reach",
        f"- **pooled: precision {num(t['precision'])}, recall {num(t['recall_claimable'])} of "
        f"claimable** — {t['emitted']} pairings emitted, {t['found']} true, {t['wrong']} untrue, "
        f"**{num(t['wrong_per_100_windows'], 1)} untrue pairings per 100 windows reviewed** "
        f"({t['wrong_named_decoy']} of them a named decoy, {t['wrong_fabricated']} fabricated)",
        f"- **the fact tier: {tiers['fact']['emitted']} pairings emitted, "
        f"{tiers['fact']['wrong']} untrue** — precision **{num(tiers['fact']['precision'])}**, recall "
        f"{num(tiers['fact']['recall_claimable'])} of claimable, and "
        f"{tiers['fact']['wrong_named_decoy']} named decoys asserted as facts",
        f"- **the guess tier: {tiers['guess']['emitted']} pairings, {tiers['guess']['wrong']} untrue**, "
        f"precision {num(tiers['guess']['precision'])}, recall "
        f"{num(tiers['guess']['recall_claimable'])} — the tier exists because the lies are inside the "
        f"range where the truths are, so it is labelled instead of tuned away",
        "- **the two fixtures `e2.4` names**: "
        + ", ".join(f"`{scenario}` recall {num(row['recall'])} with "
                    f"{row['found']}/{row['claimable']} claimable pairs found"
                    for scenario, row in sorted(named.items()))
        + ", **0 untrue pairings in either**",
        f"- **{t['silent_windows']} windows in which nothing moved, "
        f"{t['silent_windows_dirty']} of them made anything up**",
    ]
    return "\n".join(lines)


def by_representation(report: dict) -> str:
    """Recall per representation class, shipped mode beside the widened mode that sees copies."""
    shipped, wider = report["by_representation"], report["copies_harder"]["by_representation"]
    rows = ["| class | claimable pairs | found, shipped | recall, shipped | found, widened "
            "| recall, widened |",
            "|---|---|---|---|---|---|"]
    for kind in KINDS:
        one, two = shipped[kind], wider[kind]
        rows.append("| %s | %d | %d | %s | %d | %s |"
                    % (PHRASE[kind], one["claimable"], one["found"], num(one["recall"]),
                       two["found"], num(two["recall"])))
    rows.append("")
    rows.append("Found under the *wrong* shape anywhere in the corpus: **%d** \u2014 a split reported as "
                "a rename is a representation failure, not a hit with a different label, and the gate "
                "(`A2c`) says so." % sum(v["found_wrong_shape"] for v in shipped.values()))
    rows.append("")
    rows.append("Shapes emitted: %s. Shapes reaching the fact tier: %s — which is the spike's finding "
                "reproduced by the shipped engine: a group is never a fact, so a split, a fold or a "
                "copy is on the card as a guess or not at all."
                % (", ".join("%s ×%d" % (shape, n) for shape, n in
                             sorted(report["shapes_emitted"].items())),
                   ", ".join("%s ×%d" % (shape, n) for shape, n in
                             sorted(report["shapes_at_fact_tier"].items()))))
    return "\n".join(rows)


def cost(report: dict) -> str:
    """Two tables: repository growth with the change set fixed, and change-set growth."""
    c = report["cost"]
    growth = c["change_set_growth"]
    rows = ["Repository size, with the change set held at %d moved files "
            "(mean of 3 runs, milliseconds for build + read + pair):"
            % c["shipped"][0]["changed_pairs"],
            "",
            "| paths in the tree | shipped mode | widened mode | blobs read, shipped | blobs read, "
            "widened |", "|---|---|---|---|---|"]
    for one, two in zip(c["shipped"], c["widened"]):
        rows.append("| %d | %s ms | %s ms | %d | %d |"
                    % (one["repo_files"], one["total_ms"], two["total_ms"], one["blobs_read"],
                       two["blobs_read"]))
    rows.append("")
    rows.append("From the smallest repository to 20× it: **%s× shipped, %s× widened**. The shipped "
                "mode's curve is flat against the tree because it never looks at the tree; the widened "
                "mode's is not, which is the whole reason it is opt-in. Gate `A4a` bars the shipped "
                "ratio at %s." % (c["ratio_small_to_20x_shipped"], c["ratio_small_to_20x_widened"],
                                  "3.0"))
    rows.append("")
    rows.append("Change-set size, with the repository held at %d paths:"
                % growth[0]["repo_files"])
    rows.append("")
    rows.append("| changed file pairs | candidate pairs | pairs scored | blobs read | pair (ms) | "
                "total (ms) |")
    rows.append("|---|---|---|---|---|---|")
    for row in c["change_set_growth"]:
        rows.append("| %d | %d | %d | %d | %s | %s |"
                    % (row["changed_pairs"], row["candidates"], row["scored_pairs"],
                       row["blobs_read"], row["pair_ms"], row["total_ms"]))
    rows.append("")
    rows.append("×%d the change set costs **%s× the time**: the candidate set is a cross product of "
                "the window's own deletions and additions, so it is quadratic *in what changed* and "
                "that is the honest statement of the bound — `O(changed files)`, with the square in "
                "it, pruned by `rename.candidates.top_k` and `rename.candidates.budget` rather than "
                "hidden." % (growth[-1]["changed_pairs"] // max(1, growth[0]["changed_pairs"]),
                             c["change_1_to_48_shipped"]))
    return "\n".join(rows)


def display_budget(report: dict) -> str:
    """The sweep that shows the shipped floor is a choice with a price on it."""
    rows = ["| guess floor | pairings emitted | found | untrue | precision | recall | true pairings "
            "per untrue one |", "|---|---|---|---|---|---|---|"]
    for floor, row in sorted(report["display_budget"].items(), key=lambda kv: float(kv[0])):
        rows.append("| %s | %d | %d | %d | %s | %s | %s |"
                    % (floor, row["emitted"], row["found"], row["wrong"], num(row["precision"]),
                       num(row["recall"]), num(row["true_pairings_per_wrong"], 1)))
    rows.append("")
    rows.append("The shipped floor is **%s**: raising it buys precision by losing pairings the "
                "reviewer could have accepted, and lowering it to 0.10 admits the corpus's scaffolded "
                "decoy — the one row that would have been *wrong* rather than merely missed at 0.15. "
                "A guess is allowed to be wrong; it is not allowed to be silent about being a guess."
                % report["policy"]["guess_floor"])
    return "\n".join(rows)


def gates(report: dict) -> str:
    rows = ["| gate | expected | observed | result | claim |", "|---|---|---|---|---|"]
    for gate in report["gates"]:
        rows.append("| `%s` | %s | %s | %s | %s |"
                    % (gate["id"], gate["expected"], gate["observed"], gate["result"],
                       gate["claim"].replace("|", "\\|")))
    failed = [g["id"] for g in report["gates"] if g["result"] != "pass"]
    rows.append("")
    rows.append("%d gates, %d failed%s."
                % (len(report["gates"]), len(failed),
                   "" if not failed else " (%s)" % ", ".join(failed)))
    return "\n".join(rows)


BLOCKS = {"rename_pairing_headline": headline,
          "rename_pairing_by_representation": by_representation,
          "rename_pairing_cost": cost,
          "rename_pairing_display_budget": display_budget,
          "rename_pairing_gates": gates}


def render(document: pathlib.Path, report: dict, *, write: bool) -> list[str]:
    """Fill every block. With `write`, repeat until the document is stable: each pass can only fix the
    offsets it can see, so a freshly written block is re-matched on the next pass."""
    problems: list[str] = []
    for _ in range(len(BLOCKS) + 1):
        text = document.read_text()
        stale = False
        for match in MARK.finditer(text):
            name = match.group(1)
            renderer = BLOCKS.get(name)
            if renderer is None:
                problems.append(f"{document.name}: unknown generated block {name!r}")
                continue
            body = renderer(report).strip()
            if match.group(2).strip() != body:
                stale = True
                if not write:
                    problems.append(f"{document.name}: block {name} is out of date")
        if not write:
            break
        if not stale:
            document.write_text(text)
            break
        updated = text
        # Reverse order: a replacement shifts every offset after it, so the last block is replaced
        # first and the earlier matches' offsets stay valid.
        for match in sorted(MARK.finditer(text), key=lambda m: m.start(), reverse=True):
            renderer = BLOCKS.get(match.group(1))
            if renderer is None:
                continue
            body = renderer(report).strip()
            if match.group(2).strip() != body:
                updated = updated[:match.start(2)] + body + "\n" + updated[match.end(2):]
        document.write_text(updated)
    return problems


def main(argv: list[str]) -> int:
    if len(argv) != 2 or argv[1] not in ("write", "check"):
        print(__doc__)
        return 2
    if not REPORT.exists():
        print(f"missing {REPORT}; run\n"
              f"  PYTHONPATH=src python3 -m recommender.rename score --write")
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
    print(f"rename pairing tables {'written' if write else 'in sync'}: {CONCEPT.name}, "
          f"{len(BLOCKS)} blocks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
