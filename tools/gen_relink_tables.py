#!/usr/bin/env python3
"""Render spike `s6`'s measured tables from the committed report, and fail on drift.

`docs/concepts/episode-relink.md` and ADR-003 §19 carry the numbers that closed ADR-003 §13.1 — the
missed-relink and false-relink rates of the shipped ladder, the price of every candidate fuzzy rung, and
the gate results. Every one of them comes from one run of one harness, committed as
`benchmarks/reports/s6-relink-accuracy.json`, and a table of numbers copied by hand out of a JSON file is
a table that quietly keeps its old numbers the day the corpus changes. So they are generated, the same way
`s2`'s are (`tools/gen_segment_tables.py`):

    python3 tools/gen_relink_tables.py write   # regenerate both documents in place
    python3 tools/gen_relink_tables.py check   # exit 1 if the documents and the report disagree

Blocks are delimited by `<!-- BEGIN GENERATED: name -->` / `<!-- END GENERATED: name -->`. A block no
document renders is an error, because that means the generator and the documents disagree about what
exists. Nothing here computes anything: every cell is read out of the report, and
`cargo run -p fathom-analyze --features sqlite-bundled --example s6_spike -- --check` is what proves the
report still matches the corpus.
"""
from __future__ import annotations

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s6-relink-accuracy.json"
CONCEPT = ROOT / "docs" / "concepts" / "episode-relink.md"
ADR = ROOT / "docs" / "adr" / "0003-episodes.md"

# The ladders whose rows the documents quote by hand; the rest are census columns.
HEADLINES = ("shipped", "fz-subject@0.45", "recency-any")


def fmt(value: float | None, places: int = 3) -> str:
    return "—" if value is None else f"{value:.{places}f}"


def ladder(report: dict, lid: str) -> dict:
    for row in report["results"]:
        if row["id"] == lid:
            return row
    raise KeyError(f"the report has no ladder {lid!r}")


def pooled(report: dict, lid: str, board: str) -> dict:
    return ladder(report, lid)["boards"][board]["scopes"]["pooled"]


def families(report: dict, lid: str, board: str) -> dict[str, dict]:
    return ladder(report, lid)["boards"][board]["families"]


def corpus(report: dict) -> str:
    c = report["corpus"]
    lines = [
        "| | |",
        "|---|---|",
        f"| Turn histories the events are laid on | {c['turn_histories']} "
        f"({c['annotated_turns']} annotated turns of `s2`'s corpus) |",
        f"| Rewrite events scored | **{c['rewrite_events']}** |",
        f"| Rewrite families | {len(c['families'])} ({', '.join(sorted(c['families']))}) |",
        f"| Corpus files | {', '.join(f'`{f}`' for f in c['corpus_files'])} |",
        f"| Ladders scored per event | {len(report['ladders'])} |",
        "| Boards per ladder | `signed-off` (every episode dispositioned) and `sparse` (every third) |",
        f"| Store pin | {report['store_pin']['events_checked']} events checked against "
        f"`fathom_store::episode`'s own relink pass; agrees: "
        f"**{'yes' if report['store_pin']['agrees'] else 'NO'}** |",
    ]
    return "\n".join(lines)


def ladders(report: dict) -> str:
    lines = [
        "| Ladder | Tiers, in the order RL1 tries them | What it is for |",
        "|---|---|---|",
    ]
    for row in report["ladders"]:
        lines.append(
            f"| `{row['id']}` | {' › '.join(f'`{t}`' for t in row['tiers'])} | {row['rationale']} |"
        )
    return "\n".join(lines)


def boards(report: dict) -> str:
    lines = [
        "| Ladder | Board | Carries owed | …with a verdict behind them | Caught | Missed | Missed rate | "
        "…no tier could | …twin churn | …tier had it | Written | False relinks | False rate of written |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for lid in HEADLINES:
        for board in ("signed-off", "sparse"):
            p = pooled(report, lid, board)
            lines.append(
                f"| `{lid}` | `{board}` | {p['owed']} | {p['owed_with_a_verdict_behind_it']} | "
                f"{p['caught']} | {p['missed']} | {fmt(p['missed_rate'])} | "
                f"{p['missed_where_no_tier_could']} | {p['missed_where_a_twin_kept_the_digest']} | "
                f"{p['missed_where_a_tier_had_the_evidence']} | {p['written']} | {p['false_relinks']} | "
                f"{fmt(p['false_rate_of_written'])} |"
            )
    return "\n".join(lines)


FAMILY_ORDER = (
    "amend-no-edit", "rebase-picks", "amend-adds-file", "amend-moves-files", "reword", "reword-batch",
    "reword-to-shared-subject", "squash-kept-subject", "revert-then-redo", "force-merge-regroup",
    "split-commit", "squash-new-subject", "reword-and-move", "reset-author", "cherry-twin",
    "backport-renamed", "split-regroup", "branch-rebase", "branch-rebase-heavy", "dropped-turn",
    "dropped-and-reworded",
)


def families_table(report: dict) -> str:
    fams = families(report, "shipped", "signed-off")
    lines = [
        "`shipped`, on the `signed-off` board, per family. `owed` is the annotated carries; `floor` is the "
        "ones the annotation itself marks unreachable; `defect` is the ones the ladder owed itself.",
        "",
        "| Family | Owed | Caught | Missed | floor | defect | twin | Written | False | Crowd from one "
        "donor landing twice |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    order = [f for f in FAMILY_ORDER if f in fams] + sorted(set(fams) - set(FAMILY_ORDER))
    for name in order:
        v = fams[name]
        lines.append(
            f"| `{name}` | {v['owed']} | {v['caught']} | {v['missed']} | "
            f"{v['missed_where_no_tier_could']} | {v['missed_where_a_tier_had_the_evidence']} | "
            f"{v['missed_where_a_twin_kept_the_digest']} | {v['written']} | {v['false_relinks']} | "
            f"{v['false_from_a_donor_that_landed_more_than_once']} |"
        )
    return "\n".join(lines)


def false_table(report: dict) -> str:
    lines = [
        "| Ladder | Board | Written | False relinks | False rate of written | …from a donor that landed "
        "more than once | …landing on an episode that had to arrive unread | Silent annotations violated |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for lid in ("t1-only", "t2-only", "t3-only", "shipped", "fz-subject@0.45", "fz-subject-unique@0.45",
                "recency-any"):
        for board in ("signed-off", "sparse"):
            p = pooled(report, lid, board)
            lines.append(
                f"| `{lid}` | `{board}` | {p['written']} | {p['false_relinks']} | "
                f"{fmt(p['false_rate_of_written'])} | {p['false_from_a_donor_that_landed_more_than_once']} | "
                f"{p['false_on_must_arrive_unread']} | {p['must_stay_silent']} |"
            )
    return "\n".join(lines)


def marginal(report: dict) -> str:
    rows = report["fuzzy_marginal_price"]
    lines = [
        "Each row is one candidate rung minus the shipped ladder: what the rung added, on that board. The "
        "price §6.6 sets is better than 3 carries per false relink.",
        "",
        "| Rung | Board | Extra carries bought | Extra false relinks | Net at 3:1 |",
        "|---|---|---|---|---|",
    ]
    for row in rows:
        net = row["net_at_3_to_1"]
        lines.append(
            f"| `{row['ladder']}` | `{row['board']}` | {row['bought_carries']} | "
            f"{row['bought_false_relinks']} | {'+' if net >= 0 else ''}{net} |"
        )
    return "\n".join(lines)


def gates(report: dict) -> str:
    lines = [
        "| Gate | Claim | Cell | Measured | Expected | As expected |",
        "|---|---|---|---|---|---|",
    ]
    for g in report["gates"]:
        measured = g.get("measured")
        shown = "—" if measured is None else (f"{measured:g}" if isinstance(measured, float) else str(measured))
        claim = str(g["claim"]).replace("|", "\\|")
        lines.append(
            f"| `{g['id']}` | {claim} | `{g['ladder']}` · `{g['board']}` · `{g['scope']}` · "
            f"`{g['metric']}` | {shown} | `{g['expected']}` | "
            f"{'yes' if g['as_expected'] else '**NO**'} |"
        )
    failed = [g["id"] for g in report["gates"] if not g["pass"]]
    lines.append("")
    lines.append(
        f"{len(report['gates'])} gates, {len(failed)} of them declared failing and therefore findings: "
        + ", ".join(f"`{g}`" for g in failed) + "."
    )
    return "\n".join(lines)


def blocks(report: dict) -> dict[str, str]:
    return {
        "s6_corpus": corpus(report),
        "s6_ladders": ladders(report),
        "s6_boards": boards(report),
        "s6_families": families_table(report),
        "s6_false": false_table(report),
        "s6_marginal": marginal(report),
        "s6_gates": gates(report),
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
        out = head + begin + "\n" + body + "\n" + end + tail
        used.add(name)
    return out, used


def main(argv: list[str]) -> int:
    mode = argv[0] if argv else "check"
    if mode not in ("check", "write"):
        print(__doc__)
        return 2
    if not REPORT.exists():
        print("relink spike: no report at "
              f"{REPORT.relative_to(ROOT)}; run `cargo run -p fathom-analyze --features sqlite-bundled "
              "--example s6_spike`")
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
        problems.append(f"generated blocks no document renders: {', '.join(unused)}")
    if problems:
        for problem in problems:
            print(f"relink spike: {problem}")
        print("relink spike: run `python3 tools/gen_relink_tables.py write`")
        return 1
    print(f"relink spike: every generated table matches {REPORT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
