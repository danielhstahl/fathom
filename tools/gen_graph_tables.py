#!/usr/bin/env python3
"""Render spike `s3`'s measured tables from the committed report, and fail on drift.

`docs/concepts/graph-storage.md` and ADR-004 §13.7 carry the numbers that closed ADR-004
§14.1. Those numbers come from one run of one prototype, committed as
`benchmarks/reports/s3-graph-snapshots.json`, and a table of numbers copied by hand from a JSON
file is a table that quietly keeps its old numbers the day the prototype changes. So they are
generated, the same way ADR-004's other normative tables are (see `tools/gen_adr_tables.py`):

    python3 tools/gen_graph_tables.py write   # regenerate both documents in place
    python3 tools/gen_graph_tables.py check   # exit 1 if the documents and the report disagree

Blocks are delimited by `<!-- BEGIN GENERATED: name -->` / `<!-- END GENERATED: name -->`, and
each document renders the subset of blocks it carries: ADR-004 §13.7 wants the two decision
tables, the concept document wants all five. A block that *no* document renders is an error,
because that means the generator and the documents disagree about what exists.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s3-graph-snapshots.json"
CONCEPT = ROOT / "docs" / "concepts" / "graph-storage.md"
ADR = ROOT / "docs" / "adr" / "0004-storage.md"

MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)


def mib(value: float) -> str:
    return f"{value / (1024 * 1024):,.1f} MiB"


def gib(value: float) -> str:
    return f"{value / (1024 ** 3):,.2f} GiB"


def cell(strategy: dict, name: str, key: str = "p95") -> str:
    """One latency cell, preferring the materialised-graph number and falling back to SQL."""
    for source in ("query_ms", "direct_sql_ms"):
        got = strategy[source].get(name)
        if got and got.get(key) is not None:
            return f"{got[key]:g}"
    return "—"


def sql_cell(strategy: dict, name: str, key: str = "p95") -> str:
    """The same question answered by SQL against the stored rows, nothing materialised."""
    got = strategy["direct_sql_ms"].get(name)
    return f"{got[key]:g}" if got and got.get(key) is not None else "—"


def workload(report: dict) -> str:
    work, walk = report["workload"], report["walk"]
    return "\n".join([
        f"- Repository: **{work['commits']:,} commits** written by `git fast-import`, "
        f"{work['git_dir_bytes'] / 1e6:,.0f} MB of git objects, read back through "
        f"`git log -r --raw` and `git cat-file --batch` ({walk['blob_read_s']} s for "
        f"{walk['distinct_blobs']:,} distinct blobs).",
        f"- Graph at head: **{work['graph_nodes_at_head']:,} nodes + "
        f"{work['graph_edges_at_head']:,} edges = "
        f"{work['graph_nodes_at_head'] + work['graph_edges_at_head']:,} rows** over "
        f"{work['modules_at_head']:,} live modules, reached in "
        f"{work['commits_measured']:,} measured revisions.",
        f"- Commits touch **{work['avg_changed_files_per_commit']} files** on average and "
        f"move **{work['avg_graph_ops_per_commit']} graph ops**; file ops were "
        f"{work['file_ops']['edit']:,} edits, {work['file_ops']['add']} adds, "
        f"{work['file_ops']['move']} renames, {work['file_ops']['delete']} deletes.",
        f"- Cost of the lane's own reading and extraction, once: history "
        f"{walk['history_read_s']} s, blobs {walk['blob_read_s']} s, one tree listing "
        f"{walk['tree_read_ms']} ms, extraction {walk['extract_ms_total'] / 1000:.1f} s "
        f"for {walk['distinct_blobs']:,} blobs, peak RSS {walk['peak_rss_mib']} MiB.",
        f"- Environment: Python {report['environment']['python']}, SQLite "
        f"{report['environment']['sqlite']}, {report['environment']['platform']}, "
        f"{report['environment']['cpu_count']} cores. Prototype floors — bytes carry to "
        f"`fathom-store`; times are floors, not Rust predictions.",
        "",
    ])


def strategies(report: dict) -> str:
    budget = report["budgets_under_test"]["store.bytes_per_commit_graph"]
    rows = [
        "| Strategy | Cost claim (stated before measuring) | B/revision | × budget | "
        "Projected at 10 000 rev | rows/rev | write p50 ms | cold materialise p95 ms | "
        "warm step p95 ms | Δ adjacent p95 ms | Δ 500 apart p95 ms |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for s in report["strategies"]:
        per = s["bytes_per_revision"]
        projected = s["bytes_projected_at_10k"]
        big = gib(projected) if projected >= 1024 ** 3 else mib(projected)
        rows.append(
            f"| **{s['strategy'].split(':')[0]}** | {s['scaling_claim']} | {per:,} | "
            f"{per / budget:.2f}× | {big}{' (projected)' if s['extrapolated'] else ''} | "
            f"{s['rows_per_revision']:g} | {s['record_ms_p50'] or '—'} | "
            f"{s['materialize_ms_p95'] or '—'} | {s['warm_step_ms_p95'] or '—'} | "
            f"{cell(s, 'delta_adjacent')} | {cell(s, 'delta_distant')} |")
    rows.append("")
    rows.append(
        f"`store.bytes_per_commit_graph` at the time of measurement was **{budget:,} B/revision** "
        f"(§11; it is 32 KiB now, and §14.1 says why). `write` is the cost of storing one revision; "
        f"`cold materialise` is loading a whole revision's graph; `warm step` is the cursor moving "
        f"one commit on a graph already in hand; the Δ columns are `e3.3`'s changed-edges query, "
        f"adjacent and 500 revisions apart. Two kinds of `—`, and they mean opposite things: "
        f"`PER_FILE` writes *nothing* per revision (its blob-keyed row was written when the file "
        f"was first seen), and no delta is measured for `FULL` or `CHUNKED` because the only way "
        f"they can answer one is to materialise *both* revisions and diff them — two of the "
        f"cold-materialise figures in the previous column, which is a finding rather than a gap. "
        f"`CHAIN` has no row here: it is swept per anchor period in the next table, because the "
        f"anchor period is the one knob it has.")
    rows.append("")
    return "\n".join(rows)


def sweep(report: dict) -> str:
    budget = report["budgets_under_test"]["store.bytes_per_commit_graph"]
    rows = [
        "| `graph.chain_full_every` | B/revision | × budget | of which the anchor manifest | "
        "rows/rev | write p50 ms | write p95 ms | cold materialise p95 ms | warm step p95 ms | "
        "`dependents(X)` p95 ms | Δ adjacent p95 ms |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for row in report["chain_sweep"]:
        rows.append(
            f"| N = {row['every']} | {row['bytes_per_revision']:,} | "
            f"{row['bytes_per_revision'] / budget:.2f}× | "
            f"{row['bytes_per_revision'] - row['bytes_per_revision_without_manifest']:,} | "
            f"{row['rows_per_revision']:g} | {row['record_ms_p50']} | {row['record_ms_p95']} | "
            f"{row['materialize_ms_p95']} | {row['warm_step_ms_p95']} | "
            f"{row['dependents_p95_ms']} | {row['delta_adjacent_p95_ms']} |")
    rows.append("")
    rows.append("The two right-hand columns are what an architecture pane and `e3.3` pay; the two "
                "`write` columns are what the lane pays per commit. Read down them: only the bytes "
                "move with N. That is the whole argument for choosing the anchor period on bytes "
                "and not on latency.")
    rows.append("")
    return "\n".join(rows)


def queries(report: dict) -> str:
    names = [item["query"] for item in report["query_battery"]]
    head = ["Strategy"] + [f"`{n}` p50 / p95 ms" for n in names] + \
           ["cold round-trip: materialise + one `dependents`"]
    rows = ["| " + " | ".join(head) + " |", "|---|" + "---|" * (len(names) + 1)]
    for s in report["strategies"]:
        cells = [f"**{s['strategy'].split(':')[0]}**"]
        for name in names:
            value = "—"
            for source in ("query_ms", "direct_sql_ms"):
                got = s[source].get(name)
                if got and got.get("p50") is not None:
                    value = f"{got['p50']:g} / {got['p95']:g}"
                    break
            cells.append(value)
        materialise, deps = s["materialize_ms_p50"], (s["query_ms"].get("dependents") or {}).get("p50")
        cells.append("—" if deps is None or materialise is None else f"{materialise + deps:.1f}")
        rows.append("| " + " | ".join(cells) + " |")
    rows.append("")
    rows.append("These are **warm** in-graph costs: the revision's graph is materialised, which is "
                "the state a pane that is being looked at is in — and the only state in which a "
                "keystroke asks a graph question at all (ADR-006 §7.6's frame budget is spent "
                "against what is already in hand). Every strategy answers `dependents(X)` — the "
                "fan-in case ADR-004 §11 budgets at 20 ms for `medium` — in thousandths of a "
                "millisecond, so that budget is not spent on traversal; it is spent on the last "
                "column, the cold case: materialise the revision, then answer once. That is what "
                "selecting an episode costs, which is why `graph.materialise_revision.ms` exists "
                "as a budget while `query.graph_dependents.ms` stays where `e3.2` put it. For "
                "contrast, answering `dependents(X)` as SQL against the stored rows instead of "
                "against a materialised graph costs "
                f"{sql_cell(report['strategies'][0], 'dependents')} ms p95 on `FULL` — still inside "
                "budget, and the only strategy besides `CHUNKED` that can do it at all: `MERKLE` "
                "has no SQL answer for the reverse direction, because nothing in the trie indexes "
                "it — which is the second reason the trie was not chosen.")
    rows.append("")
    return "\n".join(rows)


def by_name(report: dict) -> dict:
    return {s["strategy"].split(":")[0]: s for s in report["strategies"]}


def decision(report: dict) -> str:
    rec, s = report["recommendation"], by_name(report)
    interactive = rec["budget_interactive_ms"]
    return "\n".join([
        f"- **Full snapshot per revision, unshared — refused.** "
        f"{s['FULL']['bytes_per_revision']:,} B/revision, "
        f"{s['FULL']['bytes_per_revision'] / rec['budget_bytes_per_revision']:,.0f}× the budget in "
        f"force, and the projection is not a rounding error: "
        f"{gib(s['FULL']['bytes_projected_at_10k'])} for the `medium` class alone.",
        f"- **Full snapshot per revision with structural sharing — affordable, and not chosen.** "
        f"{s['MERKLE']['bytes_per_revision']:,} B/revision is inside budget, but a revision's graph "
        f"is reachable only by walking the trie, so `dependents(X)` and `e3.3`'s delta each cost a "
        f"whole materialisation ({cell(s['MERKLE'], 'delta_adjacent')} ms p95 for the adjacent "
        f"delta, against a {interactive} ms interactive budget), and fixing that means a second, "
        f"reverse-keyed index — doubling the write to buy back what the representation just saved.",
        f"- **Content-defined chunking — refused on rows, not bytes.** Chunk payloads are shared by "
        f"content; the per-revision *membership* list is not: "
        f"{s['CHUNKED']['bytes_per_revision']:,} B/revision, {s['CHUNKED']['rows_per_revision']:g} "
        f"rows per revision, almost all of them links.",
        f"- **Chosen: delta rows every revision, a shared content-addressed anchor every "
        f"`graph.chain_full_every = {rec['chain_full_every']}` revisions, on top of the blob-keyed "
        f"per-file structural cache.** {rec['chain_bytes_per_revision']:,} B/revision — "
        f"{rec['headroom_vs_budget']}× headroom against the budget in force — "
        f"{rec['chain_warm_step_p95_ms']} ms p95 for the interactive cursor step, "
        f"{rec['chain_cold_materialise_p95_ms']} ms p95 to materialise an arbitrary revision, and "
        f"`e3.3`'s adjacent delta answered by one indexed query.",
        f"- **`graph.query.max_chain_walk = {rec['chain_query_max_chain_walk']}`**, deliberately the "
        f"same number as the anchor period: a chain longer than the anchor period means an anchor "
        f"went missing, and a missing anchor is ADR-004 §8.5's stated partial, not a slower answer.",
        f"- **Per-revision membership lists are banned outright.** A full re-statement costs "
        f"{s['FULL']['bytes_per_revision'] // s['PER_FILE']['bytes_per_revision']:,}× what the "
        f"per-file substrate costs ({s['PER_FILE']['bytes_per_revision']:,} B/revision, the "
        f"cheapest thing measured, and free to keep because `e1.4` already maintains it). Any "
        f"future \"store the whole graph again\" proposal re-runs this spike before it re-enters "
        f"the plan.",
        "",
    ])


def render(text: str, blocks: dict[str, str]) -> tuple[str, set[str]]:
    """Substitute every block whose markers are present in this file."""
    used: set[str] = set()
    out = text
    for name, body in blocks.items():
        begin, end = f"<!-- BEGIN GENERATED: {name} -->", f"<!-- END GENERATED: {name} -->"
        if begin not in out or end not in out:
            continue
        head, rest = out.split(begin, 1)
        _old, tail = rest.split(end, 1)
        out = head + begin + "\n" + body + end + tail
        used.add(name)
    return out, used


def blocks(report: dict) -> dict[str, str]:
    return {
        "graph_spike_workload": workload(report),
        "graph_spike_strategies": strategies(report),
        "graph_spike_sweep": sweep(report),
        "graph_spike_queries": queries(report),
        "graph_spike_decision": decision(report),
    }


def main(argv: list[str]) -> int:
    mode = argv[0] if argv else "check"
    if mode not in ("check", "write"):
        print(__doc__)
        return 2
    if not REPORT.exists():
        print(f"graph spike: no report at {REPORT.relative_to(ROOT)}; run "
              f"`PYTHONPATH=src python3 -m recommender.store.graphbench --report <path>`")
        return 1
    report = json.loads(REPORT.read_text())
    blocks_ = blocks(report)
    problems: list[str] = []
    seen: set[str] = set()
    for path in (CONCEPT, ADR):
        text = path.read_text()
        rendered, used = render(text, blocks_)
        seen |= used
        if mode == "write":
            if rendered != text:
                path.write_text(rendered)
                print(f"{path.relative_to(ROOT)}: updated")
        elif rendered != text:
            problems.append(f"{path.name}: generated block is out of date")
    unused = sorted(set(blocks_) - seen)
    if unused:
        problems.append("no document renders: " + ", ".join(unused))
    if problems and mode == "check":
        for problem in problems:
            print("graph spike:", problem)
        return 1
    if mode == "check":
        print(f"graph spike: blocks in sync with {REPORT.relative_to(ROOT)} "
              f"({len(seen)} blocks rendered)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
