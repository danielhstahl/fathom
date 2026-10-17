#!/usr/bin/env python3
"""Render spike `s1`'s measured tables from the committed report, and fail on drift.

`docs/concepts/structure-extraction.md` carries the accuracy numbers that decided which
grammars `e3.1` (`reco-zoz`) may ship and what each language's fallback is. Those numbers come
from one run of a throwaway harness, committed as
`benchmarks/reports/s1-tree-sitter-accuracy.json`, and a table of numbers copied by hand from a
JSON file is a table that quietly keeps its old numbers the day the extractors change. So they are
generated, the same way ADR-004's tables are (see `tools/gen_adr_tables.py`):

    python3 tools/gen_structure_tables.py write   # regenerate the document in place
    python3 tools/gen_structure_tables.py check   # exit 1 if the document and the report disagree

Blocks are delimited by `<!-- BEGIN GENERATED: name -->` / `<!-- END GENERATED: name -->`. The
cost block is refreshed by `write` but exempt from `check`: throughput on a shared container is not
a contract. A
block that the document carries but this script cannot fill, or fills but the document dropped, is
an error: that is the generator and the document disagreeing about what exists.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPORT = ROOT / "benchmarks" / "reports" / "s1-tree-sitter-accuracy.json"
THRESHOLDS = ROOT / "crates" / "fathom-analyze" / "spike" / "s1" / "thresholds.json"
DOC = ROOT / "docs" / "concepts" / "structure-extraction.md"

MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)

ORDER = ["rust", "python", "typescript", "go", "ruby", "java"]

# The condition a reader has to carry, keyed by the blind-spot category the harness recorded.
# Deliberately hand-written: the report can say *that* a category lost declarations, only a
# sentence can say what an extractor is therefore not allowed to conclude.
CONDITIONS = {
    "macro_definition": "macro-generated items are invisible; a symbol absent from the parse is "
                        "not a symbol absent from the crate",
    "macro_generated_item": "macro-generated items are invisible",
    "decorated_class": "class bodies rewritten by a decorator are only partly visible",
    "generated_method": "synthetic methods (`__init__`, accessors) are not in the tree",
    "dataclass_generated": "synthetic methods (`__init__`, accessors) are not in the tree",
    "monkeypatch": "assignment-as-definition is invisible unless the value looks like a function",
    "monkeypatch_or_data": "assignment-as-definition is invisible unless the value looks like a "
                           "function",
    "define_method_dynamic": "methods whose names are computed at runtime cannot be listed",
    "class_eval": "a file's declaration list is not the set of methods the type ends up with",
    "struct_new": "a class produced by a library call is visible only as the call",
    "define_method": "methods whose names are computed at runtime cannot be listed",
    "record_accessor": "accessors the compiler synthesises are not in the tree",
    "constructor": "constructors are not symbols; that is the rule, not a miss",
    "nested_function": "function-local declarations are not symbols; that is the rule, not a miss",
    "associated_type_binding": "an associated type binding reads as a declaration, not a reference",
}


def pct(value: float) -> str:
    return f"{value * 100:.1f}"


def lang_row(lang: dict) -> list[str]:
    rel = lang["relations"]
    return [
        lang["language"],
        pct(rel["definitions"]["precision"]),
        pct(rel["definitions"]["recall"]),
        pct(rel["exports"]["precision"]),
        pct(rel["exports"]["recall"]),
        pct(rel["imports"]["precision"]),
        pct(rel["imports"]["recall"]),
    ]


def conditions_for(lang: dict, irreducible: dict) -> str:
    kinds = [spot["kind"] for spot in lang["blind_spots"]]
    listed = irreducible.get(lang["language"], [])
    parts = [CONDITIONS[k] for k in kinds if k in listed and k in CONDITIONS]
    missed = sorted(set(lang["missing_definitions"] + lang["missing_exports"] + lang["missing_edges"]))
    if not parts:
        if missed:
            names = "; ".join(m.split(" • ")[-1] for m in missed[:3])
            parts = [f"{len(missed)} missed row(s): {names}"]
        else:
            parts = ["none recorded"]
    return "; ".join(parts) if parts else "none recorded"


def verdict_table(report: dict, thresholds: dict) -> str:
    irreducible = thresholds.get("irreducible_blind_spots", {})
    rows = [
        "| Language | Verdict | Definitions P / R | Exports P / R | Imports P / R | Conditions carried |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    by = {lang["language"]: lang for lang in report["languages"]}
    for name in ORDER:
        lang = by[name]
        cells = lang_row(lang)
        condition = conditions_for(lang, irreducible)
        rows.append(
            f"| **{cells[0]}** | `{report['verdicts'][name]}` | {cells[1]} / {cells[2]} | "
            f"{cells[3]} / {cells[4]} | {cells[5]} / {cells[6]} | {condition} |"
        )
    return "\n".join(rows)


def corpus_table(report: dict) -> str:
    rows = [
        "| Language | Files | Bytes | Truth rows: definitions | exports | import edges |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    by = {lang["language"]: lang for lang in report["languages"]}
    for name in ORDER:
        c = by[name]["corpus"]
        rows.append(
            f"| {name} | {c['files']} | {c['bytes']:,} | {c['definitions']} | {c['exports']} | "
            f"{c['import_edges']} |"
        )
    total = sum(by[n]["corpus"]["bytes"] for n in ORDER)
    rows.append(f"| **total** | {sum(by[n]['corpus']['files'] for n in ORDER)} | {total:,} | "
                f"{sum(by[n]['corpus']['definitions'] for n in ORDER)} | "
                f"{sum(by[n]['corpus']['exports'] for n in ORDER)} | "
                f"{sum(by[n]['corpus']['import_edges'] for n in ORDER)} |")
    return "\n".join(rows)


def relation_table(report: dict) -> str:
    rows = [
        "| Language | Relation | Found | Correct | Missed | Spurious | Precision | Recall | F1 |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    by = {lang["language"]: lang for lang in report["languages"]}
    for name in ORDER:
        for relation in ("definitions", "exports", "imports"):
            s = by[name]["relations"][relation]
            rows.append(
                f"| {name} | {relation} | {s['true_positive'] + s['false_positive']} | "
                f"{s['true_positive']} | {s['missed']} | {s['false_positive']} | "
                f"{pct(s['precision'])}% | {pct(s['recall'])}% | {pct(s['f1'])}% |"
            )
    return "\n".join(rows)


def tag_table(report: dict) -> str:
    rows = [
        "| Language | Construct tag | Expected | Found | Recall |",
        "| --- | --- | --- | --- | --- |",
    ]
    by = {lang["language"]: lang for lang in report["languages"]}
    for name in ORDER:
        lost = [t for t in by[name]["tags"] if t["recall"] < 1.0]
        if not lost:
            rows.append(f"| {name} | *every annotated construct found* | — | — | 100% |")
            continue
        for t in sorted(lost, key=lambda t: (t["recall"], t["tag"])):
            rows.append(f"| {name} | `{t['tag']}` | {t['expected']} | {t['found']} | "
                        f"{pct(t['recall'])}% |")
    return "\n".join(rows)


def blind_table(report: dict, thresholds: dict) -> str:
    irreducible = thresholds.get("irreducible_blind_spots", {})
    rows = [
        "| Language | Recorded blind spot | Occurrences | Irreducible? |",
        "| --- | --- | --- | --- |",
    ]
    by = {lang["language"]: lang for lang in report["languages"]}
    for name in ORDER:
        spots = by[name]["blind_spots"]
        if not spots:
            rows.append(f"| {name} | *none recorded* | 0 | — |")
            continue
        for spot in spots:
            kind = spot["kind"]
            verdict = "yes" if kind in irreducible.get(name, []) else "no — extractor gap"
            rows.append(f"| {name} | `{kind}` | {spot['count']} | {verdict} |")
    return "\n".join(rows)


def cost_table(report: dict) -> str:
    rows = [
        "| Language | Corpus | Parse MiB/s | Extract MiB/s | p50 µs/file | p95 µs/file | ΔRSS KiB |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for bench in report["throughput"]:
        name = bench["language"]
        for corpus in ("fixture", "replica", "repository"):
            b = bench.get(corpus)
            if not b:
                continue
            rows.append(
                f"| {name} | {corpus} ({b['files']} files) | {b['parse_mib_per_second']:.2f} | "
                f"{b['extract_mib_per_second']:.2f} | {b['per_file_us_p50']:.0f} | "
                f"{b['per_file_us_p95']:.0f} | {b['rss_delta_kib']:,} |"
            )
    return "\n".join(rows)


def version_table(report: dict) -> str:
    rows = [
        "| Crate | Version | Grammar ABI | Grammar checksum (sha256, first 12) |",
        "| --- | --- | --- | --- |",
    ]
    by = {g["language"]: g for g in report["grammars"]}
    for name in ORDER:
        g = by[name]
        rows.append(f"| `{g['crate']}` | {g['version']} | {g['grammar_abi']} | "
                    f"`{g['checksum'][:12]}` |")
    return "\n".join(rows)


# Blocks `check` does not police. Throughput jitters run to run on any shared machine, and a
# documentation gate that fails because a container was busy is a gate that gets deleted. `write`
# still refreshes them; they are simply informational.
UNCHECKED = {"s1_cost"}

BLOCKS = {
    "s1_verdict": verdict_table,
    "s1_corpus": lambda r, t: corpus_table(r),
    "s1_relations": lambda r, t: relation_table(r),
    "s1_tags": lambda r, t: tag_table(r),
    "s1_blind_spots": blind_table,
    "s1_cost": lambda r, t: cost_table(r),
    "s1_versions": lambda r, t: version_table(r),
}


def render(name: str, report: dict, thresholds: dict) -> str:
    return BLOCKS[name](report, thresholds)


def substitute(text: str, report: dict, thresholds: dict, *, check: bool) -> tuple[str, set[str]]:
    used: set[str] = set()

    def repl(match: re.Match[str]) -> str:
        name = match.group(1)
        if name not in BLOCKS:
            raise SystemExit(f"{DOC}: block '{name}' has no generator")
        used.add(name)
        if check and name in UNCHECKED:
            return match.group(0)
        return f"<!-- BEGIN GENERATED: {name} -->\n{render(name, report, thresholds)}\n" \
               f"<!-- END GENERATED: {name} -->"

    return MARK.sub(repl, text), used


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "check"
    report = json.loads(REPORT.read_text())
    thresholds = json.loads(THRESHOLDS.read_text())
    text = DOC.read_text()
    new, used = substitute(text, report, thresholds, check=(mode == "check"))
    missing = sorted(set(BLOCKS) - used)
    if missing:
        print(f"{DOC}: no document renders these blocks: {', '.join(missing)}", file=sys.stderr)
        return 1
    if mode == "write":
        if new != text:
            DOC.write_text(new)
            print(f"wrote {DOC.relative_to(ROOT)}")
        else:
            print(f"{DOC.relative_to(ROOT)} already current")
        return 0
    if new != text:
        print(f"{DOC} disagrees with {REPORT.relative_to(ROOT)}; run\n"
              f"  python3 tools/gen_structure_tables.py write", file=sys.stderr)
        for i, (a, b) in enumerate(zip(text.splitlines(), new.splitlines())):
            if a != b:
                print(f"  line {i + 1}\n    have:    {a}\n    expected: {b}", file=sys.stderr)
                break
        return 1
    print(f"{DOC.relative_to(ROOT)} matches the report")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
