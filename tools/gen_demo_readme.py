#!/usr/bin/env python3
"""Render the tables in `docs/demo/README.md` from the artefacts the run actually produced.

The repository's rule about generated documentation is blunt: a table of numbers copied by hand is a
table of numbers that will quietly go wrong. The walkthrough's timings and the budget comparison are
exactly such tables, so they are rendered from `docs/demo/*.steps.json` and
`benchmarks/reports/e1-12-mvp-acceptance.json` and nothing else.

    python3 tools/gen_demo_readme.py write    # rewrite the generated regions
    python3 tools/gen_demo_readme.py check    # fail if the file and the artefacts disagree

The regions are delimited by HTML comments in the markdown, so the prose around them is hand-written
and stays hand-written.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
README = ROOT / "docs" / "demo" / "README.md"
STEPS = ROOT / "docs" / "demo" / "mvp-500-commit-review.steps.json"
DEMO = ROOT / "docs" / "demo"
REPORT = ROOT / "benchmarks" / "reports" / "e1-12-mvp-acceptance.json"

MARK = "GENERATED: {name}"
BEGIN = f"<!-- BEGIN {MARK} -->"
END = f"<!-- END {MARK} -->"


def cell(text: str, *, html: bool = False) -> str:
    """One markdown table cell: no pipes, no newlines.

    `html=True` is for prose cells, where `<C-n>` would otherwise be swallowed as an unknown tag;
    code cells are already inside backticks and must keep their angle brackets verbatim.
    """
    out = str(text).replace("|", "\\|").replace("\n", " ").strip()
    if html:
        out = out.replace("<", "&lt;").replace(">", "&gt;")
    return out


def render_script() -> str:
    data = json.loads(STEPS.read_text(encoding="utf-8"))
    script = json.loads((DEMO / "mvp-500-commit-review.json").read_text(encoding="utf-8"))["steps"]
    keystrokes = sum(1 for step in script if step.get("keys"))
    handoffs = sum(1 for step in script if "shell" in step)
    lines = [
        f"{keystrokes} keystrokes and {handoffs} hand-offs to another process, {data['total_s']} s "
        f"end to end. `visible` is the interval " 
        f"between the keystroke being written and the screen grid changing; the run's first frame took "
        f"{data['first_paint_ms']} ms and its worst visible change was {data['visible_worst_ms']} ms "
        f"(`g`, the chord timeout). Steps that ran in another process print their output underneath.",
        "",
        "| # | at | key | visible | what the reviewer was doing |",
        "|---:|---:|---|---:|---|",
    ]
    outputs = {}
    for entry in data.get("shell_output", []):
        outputs.setdefault(entry["step"], []).append(entry)
    for step in data["steps"]:
        keys = step["keys"] if step.get("keys") else ""
        if not keys and any("shell" in s for s in []):
            keys = ""
        visible = "—" if step.get("visible_ms") is None else f"{step['visible_ms']} ms"
        shown = keys if keys else ("*(another process)*" if "shell" in step.get("note", "").lower() else "—")
        lines.append(f"| {step['step']} | {step['at_s']} s | `{cell(shown)}` | {visible} "
                     f"| {cell(step['note'], html=True)} |")
        for out in outputs.get(step["step"], []):
            stream = "stderr" if "stderr" in out else "stdout"
            text = out[stream].replace("\n", " ⏎ ")
            lines.append(f"| | | | | *output ({stream}):* `{cell(text[:300])}` |")
    lines.append("")
    return "\n".join(lines)


def render_budgets() -> str:
    report = json.loads(REPORT.read_text(encoding="utf-8"))
    env = report["environment"]
    lines = [
        f"Corpus: {report['corpus']['commits']} commits, size class **{report['corpus']['size_class']}**. "
        f"Binary profile `{env['profile']}`, `{env['git']}`, {env['cpu_count']} cpus, {env['platform']}. "
        f"{report['samples_note']}.",
        "",
        "| measurement | budget | budget value | measured | verdict |",
        "|---|---|---:|---:|---|",
    ]
    for row in report["measured"]:
        measured = "—" if row["measured"] is None else f"{row['measured']:,}"
        budget = "—" if row["budget"] is None else f"{row['budget']:,} {row['unit']}"
        if row["blocked_by"]:
            verdict = f"blocked — {row['blocked_by']}"
        elif row["met"]:
            verdict = "met"
        else:
            verdict = "**over budget**"
        lines.append(f"| {cell(row['measurement'])} | `{cell(row['budget_id'])}` | {budget} "
                     f"| {measured} | {cell(verdict)} |")
    lines.append("")
    lines.append("Declared but not measurable at all from this build: "
                 + "; ".join(report["not_measured"]) + ".")
    lines.append("")
    return "\n".join(lines)


def splice(text: str, name: str, body: str) -> str:
    pattern = re.compile(re.compile(BEGIN.format(name=name)).pattern + r".*?"
                         + re.compile(END.format(name=name)).pattern, re.S)
    if not pattern.search(text):
        raise SystemExit(f"docs/demo/README.md: no {BEGIN.format(name=name)} marker")
    replacement = (BEGIN.format(name=name) + "\n\n" + body + "\n" + END.format(name=name))
    return pattern.sub(lambda _: replacement, text, count=1)


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "check"
    if mode not in ("write", "check"):
        raise SystemExit("usage: gen_demo_readme.py [write|check]")
    text = README.read_text(encoding="utf-8")
    rendered = text
    for name, renderer in (("script", render_script), ("budgets", render_budgets)):
        rendered = splice(rendered, name, renderer())
    if mode == "write":
        README.write_text(rendered, encoding="utf-8")
        print(f"docs/demo/README.md: regenerated from {STEPS.name} and {REPORT.name}")
        return 0
    if rendered != text:
        print("docs/demo/README.md is out of date with the artefacts; run "
              "`python3 tools/gen_demo_readme.py write`")
        return 1
    print(f"docs/demo/README.md matches {STEPS.name} and {REPORT.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
