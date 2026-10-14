#!/usr/bin/env python3
"""Render ADR-004's normative tables from the code that owns them, and fail on drift.

`docs/adr/0004-storage.md` contains five *generated* blocks: the durability classes, the
`.fathom/` layout, the invalidation matrix, the SQL schema and the perf budgets. They are
generated from `src/recommender/store/` and `benchmarks/budgets.json` for the same reason
`docs/adr/README.md`'s table is generated (ADR-005 §12): two copies of a contract always
drift, and the drift is discovered somewhere expensive.

    python3 tools/gen_adr_tables.py write   # regenerate the ADR in place
    python3 tools/gen_adr_tables.py check   # exit 1 if the ADR and the code disagree

Blocks are delimited by `<!-- BEGIN GENERATED: name -->` / `<!-- END GENERATED: name -->`
so everything a human wrote stays exactly where it is.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from recommender.store.cache import INVALIDATION_MATRIX, SCOPE_KINDS  # noqa: E402
from recommender.store.schema import (DURABILITY, GITIGNORE_STANZA,  # noqa: E402
                                      STORE_LAYOUT, TABLE_CLASS, ddl_text)

ADR = ROOT / "docs" / "adr" / "0004-storage.md"
BUDGETS = ROOT / "benchmarks" / "budgets.json"

MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->",
                  re.S)


def _esc(text: str) -> str:
    return str(text).replace("|", "\\|")


def durability() -> str:
    rows = ["| Class | Name | What it is | Storage | fsync policy | Loss means | Prunable |",
            "|---|---|---|---|---|---|---|"]
    for key, d in DURABILITY.items():
        rows.append("| **%s** | %s | %s | %s | %s | %s | %s |" % (
            key, d["name"], _esc(d["definition"]), _esc(d["storage"]), _esc(d["fsync"]),
            _esc(d["loss"]), _esc(d["prunable"])))
    rows.append("")
    rows.append("| Table | Class | Table | Class | Table | Class |")
    rows.append("|---|---|---|---|---|---|")
    items = sorted(TABLE_CLASS.items())
    for i in range(0, len(items), 3):
        chunk = items[i:i + 3]
        cells = []
        for name, cls in chunk:
            cells.append("`%s`" % name)
            cells.append("**%s**" % cls)
        while len(cells) < 6:
            cells += ["", ""]
        rows.append("| " + " | ".join(cells) + " |")
    return "\n".join(rows)


def layout() -> str:
    rows = ["| Path under the repo | Class | In git? | Written by | What it is / what losing it costs |",
            "|---|---|---|---|---|"]
    for p in STORE_LAYOUT:
        rows.append("| `%s` | **%s** | %s | %s | %s |" % (
            p.path, p.klass, "committed" if p.in_git else "ignored", p.writer, _esc(p.note)))
    rows.append("")
    rows.append("Required `.gitignore` stanza (§12):")
    rows.append("")
    rows.append("```gitignore")
    rows.append(GITIGNORE_STANZA.rstrip())
    rows.append("```")
    return "\n".join(rows)


def matrix() -> str:
    rows = ["| Cause | Input that changes | Key field that moves | What gets recomputed | What is *not* touched |",
            "|---|---|---|---|---|"]
    for cause, inp, field, scope, not_touched in INVALIDATION_MATRIX:
        rows.append("| %s | %s | `%s` | %s | %s |" % (
            _esc(cause), _esc(inp), field.strip("`"), _esc(scope), _esc(not_touched)))
    rows.append("")
    rows.append("Cache scope kinds (§5.3): " + ", ".join("`%s`" % s for s in SCOPE_KINDS) + ".")
    return "\n".join(rows)


def ddl() -> str:
    return "```sql\n%s\n```" % ddl_text().strip()


def budgets() -> str:
    data = json.loads(BUDGETS.read_text())
    classes = list(data["size_classes"])
    by_family: dict[str, dict] = {}
    for b in data["budgets"]:
        by_family.setdefault(b["family"], {})[b["size_class"]] = b
    header = ["| Budget id | Unit | Statistic | " + " | ".join(classes) + " | Rationale |",
              "|---|---|---|" + "---|" * len(classes) + "---|"]
    out: list[str] = []
    for family in sorted(by_family):
        rows = list(header)
        per = by_family[family]
        first = per[classes[0]]
        rows.append("| `%s` | %s | %s | " % (family, first["unit"], first["statistic"])
                    + " | ".join(_fmt(per[c]["budget"]) for c in classes)
                    + " | %s |" % _esc(first["rationale"]))
        out.append("\n".join(rows))
    body = "\n\n".join(out)
    gates = data.get("gates", {})
    if gates:
        body += "\n\nCI gate rules (`e0.11` reads these, it does not restate them):\n\n"
        body += "```json\n%s\n```" % json.dumps(gates, indent=1, sort_keys=True)
    return body


def _fmt(v) -> str:
    if isinstance(v, float) and v == int(v):
        v = int(v)
    return "{:,}".format(v) if isinstance(v, int) and abs(v) >= 1000 else str(v)


RENDERERS = {"durability": durability, "layout": layout, "matrix": matrix, "ddl": ddl,
             "budgets": budgets}


def render(text: str) -> tuple[str, list[str]]:
    problems = []
    for name, renderer in RENDERERS.items():
        begin, end = "<!-- BEGIN GENERATED: %s -->" % name, "<!-- END GENERATED: %s -->" % name
        if begin not in text or end not in text:
            problems.append("ADR is missing generated block %r" % name)
            continue
        body = renderer()
        new = "%s\n%s\n%s" % (begin, body, end)
        text = re.sub(re.escape(begin) + r".*?" + re.escape(end), lambda _m, n=new: n,
                      text, flags=re.S)
    return text, problems


def main(argv: list[str]) -> int:
    mode = argv[1] if len(argv) > 1 else "check"
    if not ADR.exists():
        print("docs/adr/0004-storage.md does not exist")
        return 1
    original = ADR.read_text()
    updated, problems = render(original)
    if mode == "write":
        ADR.write_text(updated)
        print("rendered %s: %s" % (ADR.relative_to(ROOT), ", ".join(RENDERERS)))
        for p in problems:
            print("warning: " + p)
        return 1 if problems else 0
    if problems:
        for p in problems:
            print(p)
        return 1
    if updated != original:
        print("%s has drifted from the schema/cache/budget sources;"
              " run `python3 tools/gen_adr_tables.py write`" % ADR.relative_to(ROOT))
        return 1
    print("ADR-004 tables OK: %d generated blocks in sync" % len(RENDERERS))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(list(sys.argv)))
