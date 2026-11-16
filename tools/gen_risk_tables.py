#!/usr/bin/env python3
"""Render the `features-3` axis table from the Rust `SPECS` it is a copy of, and fail on drift.

`docs/concepts/risk.md` carries one row per risk axis: the axis, its built-in weight, the question it
answers, the unit and ruler that fill it, and the row or analysis the measurement came from. Every one of
those five columns is a literal in `crates/fathom-analyze/src/risk/feature.rs`'s `pub const SPECS`, and
the page has always said so — "this table is generated, not typed" — while carrying no generated block and
no generator, which is the worst of the three states a table can be in: a claim of freshness with nothing
behind it. (`docs/reference/doc-map.md` §4.1 M1 found it while censusing the risk family, and named the
choice: write the generator, or stop saying the table is generated.)

    python3 tools/gen_risk_tables.py write   # regenerate the page from the Rust source
    python3 tools/gen_risk_tables.py check   # exit 1 if the page and the code disagree

The source is read as text, not compiled: this is the same rule `tools/gen_docs.py` applies to
`features.rs` and `exit.rs`, so the check runs in the cheap no-Rust job and a documentation-only branch
can still run it. Reading it as text is also why the parser is strict — it refuses a `SPECS` array whose
declared length it cannot reproduce, so a Rust edit that changes the shape of a `Spec` fails here loudly
instead of quietly dropping an axis row out of the page.

Blocks are delimited by `<!-- BEGIN GENERATED: name -->` / `<!-- END GENERATED: name -->`, and a block
that no document renders is an error, because that means the generator and the documents disagree about
what exists.
"""
from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FEATURE_RS = ROOT / "crates" / "fathom-analyze" / "src" / "risk" / "feature.rs"
RISK_PAGE = ROOT / "docs" / "concepts" / "risk.md"

MARK = re.compile(r"<!-- BEGIN GENERATED: ([\w.\-]+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)

SPECS_OPEN = re.compile(r"pub const SPECS: \[Spec; (\d+)\] = \[")
FIELD = re.compile(r'^\s*(id|asks|unit|ruler|default|source|owner):\s*(?:"((?:[^"\\]|\\.)*)"|([0-9.]+))\s*,?\s*$')
MODEL_CONST = re.compile(r'pub const MODEL: &str = "([^"]+)"')

#: The columns the page prints, in order: `asks`, then `unit; ruler`, then `source (`owner`)`.
COLUMNS = ("Axis", "Weight", "The question it answers", "Its unit, and the ruler that fills it",
           "The row or analysis it is measured from (and who owns the input)")


def parse_specs(text: str) -> list[dict[str, str]]:
    """Every `Spec { … }` in `SPECS`, in declaration order.

    Declaration order is load-bearing, not incidental: it is the order `fathom why` prints when
    contributions tie, and the order the page's rows are written in.
    """
    open_match = SPECS_OPEN.search(text)
    if not open_match:
        raise SystemExit("risk tables: no `pub const SPECS: [Spec; N] = [` in "
                         f"{FEATURE_RS.relative_to(ROOT)} — the page and the code have stopped "
                         "describing the same shape; fix this generator rather than the page")
    declared = int(open_match.group(1))
    body = text[open_match.end():]
    stop = body.index("\n];")
    body = body[:stop]

    specs: list[dict[str, str]] = []
    current: dict[str, str] | None = None
    for line in body.splitlines():
        if line.strip().startswith("Spec {"):
            current = {}
            continue
        if line.strip().startswith("},"):
            if current is not None:
                specs.append(current)
            current = None
            continue
        field = FIELD.match(line)
        if field and current is not None:
            current[field.group(1)] = field.group(2) if field.group(2) is not None else field.group(3)
    if len(specs) != declared:
        raise SystemExit(f"risk tables: `SPECS` declares {declared} specs but this tool read "
                         f"{len(specs)} — a field this parser does not know about has arrived in "
                         f"{FEATURE_RS.relative_to(ROOT)}; teach it there rather than typing the row")
    missing = [(s.get("id", "?"), k) for s in specs for k in
               ("id", "asks", "unit", "ruler", "default", "source", "owner") if k not in s]
    if missing:
        raise SystemExit("risk tables: spec(s) with no value read: "
                         + ", ".join(f"{spec_id}.{key}" for spec_id, key in missing))
    return specs


def model_name(text: str) -> str:
    found = MODEL_CONST.search(text)
    if not found:
        raise SystemExit("risk tables: no `pub const MODEL` in "
                         f"{FEATURE_RS.relative_to(ROOT)}")
    return found.group(1)


def table(header: tuple[str, ...], rows: list[tuple[str, ...]]) -> str:
    out = ["| " + " | ".join(header) + " |",
           "|" + "|".join("---" for _ in header) + "|"]
    out += ["| " + " | ".join(c.replace("|", "\\|") for c in row) + " |" for row in rows]
    return "\n".join(out)


def blocks(text: str) -> dict[str, str]:
    specs = parse_specs(text)
    rows = [(f"`{s['id']}`",
             f"{float(s['default']):g}",
             s["asks"],
             f"{s['unit']}; {s['ruler']}",
             f"{s['source']} (`{s['owner']}`)") for s in specs]
    total = sum(float(s["default"]) for s in specs)
    if abs(total - 1.0) > 1e-9:
        # The page tells the reader the weights sum to 1.00 and that the sum is also the denominator.
        # That sentence is a claim about this array, so it is asserted here rather than hoped.
        raise SystemExit(f"risk tables: the weights in `SPECS` sum to {total:.4f}, not 1.00 — the "
                         "page's `weight denominator` sentence is now wrong; change the page in the "
                         "same commit as the code")
    return {"features_axes": table(COLUMNS, rows) + "\n"}


def render(text: str, blocks_: dict[str, str]) -> tuple[str, set[str]]:
    """Substitute every block whose markers are present in this file."""
    used: set[str] = set()
    out = text
    for name, body in blocks_.items():
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
    if not FEATURE_RS.exists():
        print(f"risk tables: no {FEATURE_RS.relative_to(ROOT)}")
        return 1
    source = FEATURE_RS.read_text(encoding="utf-8")
    blocks_ = blocks(source)
    model = model_name(source)
    problems: list[str] = []
    seen: set[str] = set()
    for path in (RISK_PAGE,):
        if not path.exists():
            problems.append(f"{path.relative_to(ROOT)} is missing")
            continue
        text = path.read_text(encoding="utf-8")
        rendered, used = render(text, blocks_)
        seen |= used
        if mode == "write":
            if rendered != text:
                path.write_text(rendered)
                print(f"{path.relative_to(ROOT)}: updated")
        elif rendered != text:
            problems.append(f"{path.name}: the generated axis table is out of date with "
                            "`risk::feature::SPECS` — run `python3 tools/gen_risk_tables.py write`")
    unused = sorted(set(blocks_) - seen)
    if unused:
        problems.append("no document renders: " + ", ".join(unused))
    if problems and mode == "check":
        for problem in problems:
            print("risk tables:", problem)
        return 1
    if mode == "check":
        print(f"risk tables: {len(seen)} block(s) in sync with "
              f"`{model}` / risk::feature::SPECS "
              f"({len(parse_specs(source))} axes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
