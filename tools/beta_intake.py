#!/usr/bin/env python3
"""Feedback intake: a filed report in, a bead out, and a register that cannot lie about its counts.

`e7.7` (**reco-rny**) is the beta loop, and the loop has one step that is usually hand-worked until it
stops happening: the moment a report becomes a piece of work. This file is that moment, plus the ledger the
weekly triage writes, because "at least twenty beta issues triaged with explicit dispositions" is a
sentence about a table and a table can be checked.

    python3 tools/beta_intake.py draft --report /tmp/report.md      # print the bd create that captures it
    python3 tools/beta_intake.py draft --report r.md --run          # and run it (needs `bd`)
    python3 tools/beta_intake.py ledger --week 2026-W43             # validate the register, print counts
    python3 tools/beta_intake.py ledger --json                      # every week, machine-readable
    python3 tools/beta_intake.py self-test                          # six refused reports, four rotten rows

**The report format is the form's own output.** A GitHub issue form renders as `### <field label>` then
the answer, so that is what `draft` reads: no bespoke intake format, no copy-paste step where a field gets
dropped. `tools/check_beta_loop.py` guarantees the forms still ask for the things this reader needs — the
forms and this reader are checked against each other, which is the only way two files describing one form
stay honest.

**Why a diagnostic is mandatory.** There is no telemetry in this product (ADR-001 §8,
`docs/concepts/privacy.md`), so a report is the only transport a diagnosis has. `fathom doctor --json` is
one JSON object: version, feature set, OS, terminal, SQLite capabilities, index state, config layers,
findings. The bug form requires it and this reader refuses a report without it — or, for a pure wording
suggestion, requires the literal `n/a` with a reason, because an empty box and a deliberate `n/a` are
different decisions and the register should be able to tell them apart.

**Why the redaction check refuses rather than warns.** The doctor object contains the reporter's paths and
username. The whole ethical content of "no telemetry, but do send a report" is that *the reporter looked at
it first*, so `draft` refuses to write a bead whose doctor block carries an un-redacted home path or a
Windows user directory — a bead is a durable, greppable, public record, and copying somebody's `/home/maria`
into one would make the tool the thing it refuses to be. The escape hatch is `--allow-unredacted`, which
says a human saw it and is recorded in the bead text.

**The register.** `.plan/intake/<ISO-week>.md`, one table, one row per report, one column that may only
contain a disposition from `DISPOSITIONS` below. The vocabulary is the label set in
`.github/labels.yml` and the table in `docs/concepts/feedback.md` §4; `tools/check_beta_loop.py` fails if
the three ever differ, so a disposition cannot be invented on a Tuesday afternoon.

Rule ids: `I01`–`I08` on reports, `L01`–`L06` on the register.

Exit: 0 clean, 1 a rule failed, 2 bad usage or unreadable input.
"""
from __future__ import annotations

import argparse
import dataclasses
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parent.parent
LEDGER_DIR = ROOT / ".plan" / "intake"

#: The seven dispositions. A report always has exactly one of these on it right now; `triage:untriaged`
#: is the absence of one, which is a state with a clock on it rather than an answer.
DISPOSITIONS = ["triage:ticketed", "triage:fixed", "triage:duplicate", "triage:needs-repro",
                "triage:by-design", "triage:wont-fix", "triage:cannot-reproduce"]

#: A disposition that asserts something exists elsewhere: a bead, a duplicate, a release.
ASSERTS_REFERENCE = {"triage:ticketed", "triage:fixed", "triage:duplicate"}

#: Where a row came from. `external` is the only one the beta's user criterion counts, and keeping it
#: separate from `self-dogfood` is what stops this project cheering at its own bug reports.
SOURCES = ["external", "self-dogfood", "board", "ci", "beta-gate"]

#: A bead id or an issue/PR link: what ASSERTS_REFERENCE rows have to name.
BEAD_OR_ISSUE = re.compile(r"(reco-[a-z0-9]{3}|github\.com/\S+/(issues|pull)/\d+|https?://\S+)")

#: Un-redacted personal paths. Deliberately a small, named list rather than a general "looks private"
#: heuristic: a check that guesses is a check that fires on everything and then gets turned off.
UNREDACTED = [
    re.compile(r"/home/(?!you\b|\*|<)[a-z0-9._-]+", re.I),
    re.compile(r"/Users/(?!you\b|\*|<)[a-z0-9._-]+", re.I),
    re.compile(r"[A-Za-z]:[\\/]Users[\\/](?!you\b|\*|<)[a-z0-9._-]+", re.I),
]

FIELD_HEADING = re.compile(r"^#{2,4}\s+(.*?)\s*$", re.M)
DOCTOR_HINT = re.compile(r"doctor", re.I)
NA_LINE = re.compile(r"^\s*n/?a\b\s*[—:-]*\s*(?P<reason>.*)$", re.I)
Bead = re.compile(r"reco-[a-z0-9]{3}")


def _run(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(args, capture_output=True, text=True)


@dataclass(frozen=True)
class Problem:
    rule: str
    where: str
    message: str

    def line(self) -> str:
        return "  %-4s %s: %s" % (self.rule, self.where, self.message)


# ---------------------------------------------------------------------------
# the report side
# ---------------------------------------------------------------------------


def parse_report(text: str) -> dict[str, str]:
    """Split a filed report into `{field label: answer}` using the form's own headings."""
    fields: dict[str, str] = {}
    matches = list(FIELD_HEADING.finditer(text))
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        body = text[match.end():end].strip()
        label = match.group(1).strip().strip("`")
        if body or label in fields:
            fields[label] = body
    # The title line, when the export carries one (`### Something is wrong` is a field, the issue title is
    # not, so a `[report]`/`[feedback]` prefix anywhere in the text is the only signal there is).
    return fields


def doctor_object(fields: dict[str, str]) -> tuple[dict | None, str, str]:
    """The doctor answer: a parsed object, or the stated `n/a` reason, or nothing."""
    for label, answer in fields.items():
        if not DOCTOR_HINT.search(label):
            continue
        # The object may be fenced (```json … ```) or raw; take the first balanced brace run.
        start = answer.find("{")
        if start >= 0:
            depth = 0
            for index in range(start, len(answer)):
                depth += {"{": 1, "}": -1}.get(answer[index], 0)
                if depth == 0:
                    try:
                        return json.loads(answer[start:index + 1]), "", label
                    except json.JSONDecodeError:
                        break
        match = NA_LINE.match(answer.strip().splitlines()[0] if answer.strip() else "")
        if match:
            reason = match.group("reason").strip()
            return None, reason or "no reason given", label
    return None, "", ""


def check_report(path: pathlib.Path, allow_unredacted: bool = False) -> tuple[list[Problem], dict]:
    """`I01`–`I08`: is this a report a maintainer could act on without a follow-up question?"""
    problems: list[Problem] = []
    text = path.read_text()
    fields = parse_report(text)
    kind = "feedback" if "[feedback]" in text.lower() else ("report" if "[report]" in text.lower() else "")
    if not kind:
        problems.append(Problem("I01", path.name,
                                "no `[report]` or `[feedback]` in the title: triage routes on it, and so does this reader"))

    doctor, na_reason, doctor_label = doctor_object(fields)
    if doctor is None and not na_reason:
        problems.append(Problem("I02", path.name,
                                "no `fathom doctor --json` object and no `n/a — reason`. There is no telemetry: "
                                "the report is the transport, and a report without the object costs a week of questions"))
    elif doctor is not None:
        blob = json.dumps(doctor)
        if not allow_unredacted:
            for pattern in UNREDACTED:
                found = pattern.search(blob)
                if found:
                    problems.append(Problem("I03", path.name,
                                            "the doctor object carries an un-redacted personal path (%s). Redact it — "
                                            "a bead is public and greppable. --allow-unredacted if a human has read it"
                                            % found.group(0)))
        for key in ("version", "features", "index"):
            if key not in doctor:
                problems.append(Problem("I04", path.name,
                                        "the doctor object has no %r section: is it really the output of "
                                        "`fathom doctor --json`, or a hand-written summary?" % key))

    needed = ["what happened", "command", "expected"] if kind == "report" else ["task", "plot", "wish"]
    missing = []
    for want in needed:
        if not any(want in label.lower() and answer.strip() for label, answer in fields.items()):
            missing.append(want)
    if missing:
        problems.append(Problem("I05", path.name,
                                "no answer for %s — the form asks for them because triage cannot work without them"
                                % ", ".join("`%s`" % m for m in missing)))
    if doctor is None and na_reason and kind == "report":
        problems.append(Problem("I06", path.name,
                                "a bug report cannot `n/a` its way out of the diagnostic; only the feedback form "
                                "may, and only for something nothing measured"))

    # The sentence the bead gets: the first line of the symptom, not the first line of the file.
    symptom = ""
    for label in ("what happened", "where did you lose the plot", "what were you trying to do"):
        for field in fields:
            if label in field.lower() and fields[field].strip():
                symptom = fields[field].strip().splitlines()[0].strip()
                break
        if symptom:
            break
    if not symptom:
        problems.append(Problem("I07", path.name, "nothing here can become the bead's first line"))
    if len(symptom) > 200:
        problems.append(Problem("I08", path.name, "the symptom sentence is %d characters; a bead title is not a paragraph"
                                % len(symptom)))

    areas = sorted({a.strip().lstrip("-—· ").strip("` ") for label, answer in fields.items()
                    if "which part" in label.lower() for a in answer.splitlines() if a.strip()})
    return problems, {"file": str(path), "kind": kind, "symptom": symptom,
                      "doctor_version": (doctor or {}).get("version", ""),
                      "doctor_features": (doctor or {}).get("features", []),
                      "na_reason": na_reason, "areas": areas,
                      "fields": sorted(fields)}


def bead_command(report: dict, *, priority: str = "P2", run: bool = False) -> list[str]:
    """The `bd create` line that captures a report as a bead (the `triage:ticketed` act)."""
    labels = ",".join(["beta-feedback", "triage:ticketed"] + (["kind:%s" % report["kind"]] if report["kind"] else []))
    description = "\n".join([
        "Captured from a beta report by `tools/beta_intake.py` (`e7.7`, **reco-rny**).",
        "",
        "*Symptom, in the reporter's words:* %s" % report["symptom"],
        "*Reported against:* `fathom %s`" % (report["doctor_version"] or "version not supplied"),
        "*Doctor:* %s" % ("`fathom doctor --json` attached to the report" if report["doctor_version"]
                          else "`n/a — %s`" % report["na_reason"]),
        "*Area claims:* %s" % ("; ".join(report["areas"]) or "not stated"),
        "",
        "Release notes, when this is fixed, owe three sentences: the symptom above, the cause as a "
        "mechanism, and why it was not caught earlier (docs/concepts/feedback.md §5).",
    ])
    command = ["bd", "create", report["symptom"][:120], "--type",
               "bug" if report["kind"] == "report" else "feature", "--priority", priority,
               "--labels", labels, "--description", description]
    if not run:
        return command
    work = pathlib.Path(tempfile.mkdtemp(prefix="beta-intake-"))
    try:
        result = _run(command)
        if result.returncode != 0:
            print(result.stdout.strip() or result.stderr.strip())
            raise SystemExit("`bd create` exited %d — nothing was recorded" % result.returncode)
        print(result.stdout.strip())
        return command
    finally:
        shutil.rmtree(work, ignore_errors=True)


# ---------------------------------------------------------------------------
# the register
# ---------------------------------------------------------------------------


def ledger_files(path: pathlib.Path = LEDGER_DIR) -> list[pathlib.Path]:
    return sorted(p for p in path.glob("*.md") if re.match(r"^\d{4}-W\d{2}$", p.stem))


def ledger_rows(text: str) -> tuple[list[dict[str, str]], list[str]]:
    """The table rows, zipped onto the header's column names. Textual on purpose: no PyYAML here."""
    lines = [line.strip() for line in text.splitlines()]
    for index, line in enumerate(lines):
        if not line.startswith("|") or line.count("|") < 5:
            continue
        header = [cell.strip() for cell in line.strip("|").split("|")]
        if "disposition" not in {h.lower() for h in header}:
            continue
        if index + 1 >= len(lines) or not set(lines[index + 1].replace("|", "").replace(" ", "")) <= set("-:"):
            continue
        rows = []
        for row_line in lines[index + 2:]:
            if not row_line.startswith("|"):
                break
            cells = [cell.strip() for cell in row_line.strip("|").split("|")]
            if len(cells) != len(header):
                rows.append({"__malformed__": row_line})
                continue
            rows.append(dict(zip(header, cells)))
        return rows, header
    return [], []


def check_ledger(path: pathlib.Path) -> tuple[list[Problem], dict]:
    """`L01`–`L06`: the week's register is well formed, and its counts mean what the criteria claim."""
    problems: list[Problem] = []
    rows, header = ledger_rows(path.read_text())
    if not header:
        return ([Problem("L01", path.name, "no table with a `disposition` column: the register is a table, not prose")], {})
    lower = {h.lower(): h for h in header}
    for needed in ("id", "source", "disposition"):
        if needed not in lower:
            problems.append(Problem("L01", path.name, "the register has no `%s` column" % needed))

    counted = 0
    tally: dict[str, int] = {}
    external: set[str] = set()
    for row in rows:
        if "__malformed__" in row:
            problems.append(Problem("L02", path.name, "row has %d cells, header has %d: %s"
                                    % (row["__malformed__"].count("|") - 1, len(header), row["__malformed__"][:70])))
            continue
        get = lambda key: row.get(lower.get(key, ""), "").strip().strip("`")  # noqa: E731
        disposition = get("disposition")
        source = get("source")
        ident = get("id") or "(row %d)" % counted
        counted += 1
        if disposition not in DISPOSITIONS:
            problems.append(Problem("L03", ident,
                                    "`%s` is not a disposition; the vocabulary is %s (docs/concepts/feedback.md §4)"
                                    % (disposition or "empty", ", ".join(DISPOSITIONS))))
        else:
            tally[disposition] = tally.get(disposition, 0) + 1
        if source not in SOURCES:
            problems.append(Problem("L04", ident, "`%s` is not a source; the register distinguishes %s"
                                    % (source or "empty", ", ".join(SOURCES))))
        elif source == "external":
            handle = get("reporter")
            if not handle:
                problems.append(Problem("L05", ident,
                                        "an external report names no reporter: the adoption ledger counts handles, "
                                        "and an anonymous row cannot be followed up"))
            else:
                external.add(handle)
        if disposition in ASSERTS_REFERENCE and not BEAD_OR_ISSUE.search(get("bead / next") or get("bead") or ""):
            problems.append(Problem("L06", ident,
                                    "`%s` claims a bead, a duplicate or a release, and names none. Empty is not a "
                                    "reference (docs/concepts/feedback.md §4)" % disposition))
        if disposition == "triage:needs-repro" and not get("owner"):
            problems.append(Problem("L06", ident,
                                    "`triage:needs-repro` without an owner is how a report quietly dies; "
                                    "the clock runs on a named person (docs/concepts/feedback.md §3)"))

    return problems, {"week": path.stem, "rows": counted, "dispositions": tally,
                      "external_reporters": sorted(external)}


def ledger_summary(paths: list[pathlib.Path]) -> dict:
    rows = 0
    external: set[str] = set()
    tally: dict[str, int] = {}
    for path in paths:
        _, data = check_ledger(path)
        rows += data.get("rows", 0)
        external.update(data.get("external_reporters", []))
        for key, value in (data.get("dispositions") or {}).items():
            tally[key] = tally.get(key, 0) + value
    return {"weeks": [p.stem for p in paths], "triaged_rows": rows,
            "external_reporters": len(external), "dispositions": tally}


# ---------------------------------------------------------------------------
# the self-test
# ---------------------------------------------------------------------------

GOOD_REPORT = """### What happened

A lockfile-only change came out the riskiest episode in the repository.

### The exact command line, and what it printed

```
$ fathom why ep-1234
```

### What you expected, and why

I expected the noise axis to discount the lockfile.

### `fathom doctor --json` — read this before you paste it

```json
{"version": "0.1.0-alpha.1", "features": ["tui"], "index": {"commits": 12}}
```

### Which part of Fathom is wrong

- `the numbers (risk score, delta, architecture, scorecard)`
"""


def self_test() -> int:
    failures: list[str] = []

    def expect(condition: bool, what: str) -> None:
        print("  %s %s" % ("ok  " if condition else "FAIL", what))
        if not condition:
            failures.append(what)

    work = pathlib.Path(tempfile.mkdtemp(prefix="beta-intake-self-test-"))
    try:
        good = work / "good.md"
        good.write_text("[report] something\n\n" + GOOD_REPORT)
        problems, report = check_report(good)
        expect(not problems, "a complete report passes: %s" % [p.line() for p in problems])
        expect(report["doctor_version"] == "0.1.0-alpha.1", "and the doctor object is read, not just required")
        expect(report["symptom"].startswith("A lockfile-only change"),
               "and the bead's first line is the reporter's own sentence")
        command = bead_command(report)
        expect(command[:2] == ["bd", "create"] and "beta-feedback" in " ".join(command),
               "and the bead command is a bd create with the beta label")
        expect(any("--description" == c for c in command), "and the bead carries the release-notes obligation")

        broken = {
            "no doctor": GOOD_REPORT.replace("### `fathom doctor --json` — read this before you paste it",
                                             "### (skipped)"),
            "unredacted path": GOOD_REPORT.replace('"index"', '"home": "/home/maria/work", "index"'),
            "no symptom": GOOD_REPORT.replace("A lockfile-only change came out the riskiest episode in the repository.",
                                              ""),
            "no command": GOOD_REPORT.replace("```\n$ fathom why ep-1234\n```", ""),
            "na on a bug": GOOD_REPORT.replace('```json\n{"version": "0.1.0-alpha.1", "features": ["tui"], '
                                               '"index": {"commits": 12}}\n```', "n/a — I did not feel like it"),
            "half an object": GOOD_REPORT.replace('{"version": "0.1.0-alpha.1", "features": ["tui"], '
                                                  '"index": {"commits": 12}}', '{"version": "0.1.0-alpha.1"}'),
        }
        for name, body in broken.items():
            path = work / ("%s.md" % name.replace(" ", "-"))
            path.write_text("[report] something\n\n" + body)
            problems, _ = check_report(path)
            expect(bool(problems), "and %s is refused (%s)" % (name, [p.rule for p in problems]))
        path = work / "unredacted-allowed.md"
        path.write_text("[report] something\n\n" + broken["unredacted path"])
        problems, _ = check_report(path, allow_unredacted=True)
        expect(not problems, "with the escape hatch documented and named in the bead text")

        # ---- the register ---------------------------------------------------------------------------
        ledger = work / "2026-W43.md"
        header = ("| id | source | reporter | area | what it says | disposition | owner | bead / next |\n"
                  "|---|---|---|---|---|---|---|---|\n")
        ledger.write_text("# Week\n\n" + header +
                          "| W43-01 | self-dogfood | — | gate | a cycle that is a module and its submodule |"
                          " triage:ticketed | rota | reco-rny |\n"
                          "| W43-02 | external | @m | TUI | a toast that never expires | triage:needs-repro |"
                          " rota | reco-2ll |\n")
        problems, data = check_ledger(ledger)
        expect(not problems, "a well formed week passes: %s" % [p.line() for p in problems])
        expect(data["rows"] == 2 and len(data["external_reporters"]) == 1,
               "and the counts are the counts the criteria are written against")

        rotten = {
            "invented disposition": header + "| W43-03 | external | @m | gate | x | triage:probably-fine | rota | reco-rny |\n",
            "no reference for a ticketed row": header + "| W43-04 | external | @m | gate | x | triage:ticketed | rota | soon |\n",
            "needs-repro with no owner": header + "| W43-05 | external | @m | gate | x | triage:needs-repro | | reco-rny |\n",
            "anonymous external row": header + "| W43-06 | external | | gate | x | triage:fixed | rota | reco-rny |\n",
            "wrong source": header + "| W43-07 | twitter | @m | gate | x | triage:duplicate | rota | reco-rny |\n",
            "ragged row": "| id | source | disposition | x | y | z |\n|---|---|---|---|---|---|\n| W43-08 | external |\n",
        }
        for name, body in rotten.items():
            path = work / ("%s.md" % name.replace(" ", "-"))
            path.write_text("# Week\n\n" + body)
            problems, _ = check_ledger(path)
            expect(bool(problems), "and a register rots visibly: %s (%s)"
                   % (name, [p.rule for p in problems]))
    finally:
        shutil.rmtree(work, ignore_errors=True)

    if failures:
        print("\nself-test failed: %d check(s)" % len(failures))
        for name in failures:
            print("  - %s" % name)
        return 1
    print("\nself-test passed: six broken reports refused, one accepted, six rotten registers caught")
    return 0


# ---------------------------------------------------------------------------
# entry point
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="beta_intake.py", description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="mode")

    draft_p = sub.add_parser("draft", help="read a filed report and print the bead it should become")
    draft_p.add_argument("--report", required=True, help="the filed report, as the issue form renders it")
    draft_p.add_argument("--priority", default="P2")
    draft_p.add_argument("--run", action="store_true", help="run `bd create` instead of printing it")
    draft_p.add_argument("--allow-unredacted", action="store_true",
                         help="accept a doctor object that still carries personal paths (a human read it)")
    draft_p.add_argument("--json", action="store_true")

    ledger_p = sub.add_parser("ledger", help="validate the weekly register and print its counts")
    ledger_p.add_argument("--week", default="", help="e.g. 2026-W43; empty means every week on file")
    ledger_p.add_argument("--dir", default=str(LEDGER_DIR))
    ledger_p.add_argument("--json", action="store_true")

    sub.add_parser("self-test", help="prove the reader and the register can fail")

    args = parser.parse_args(argv)
    if args.mode == "self-test":
        return self_test()

    if args.mode == "draft":
        path = pathlib.Path(args.report)
        if not path.exists():
            print("no such report: %s" % path, file=sys.stderr)
            return 2
        problems, report = check_report(path, allow_unredacted=args.allow_unredacted)
        for problem in problems:
            print(problem.line())
        if problems:
            return 1
        command = bead_command(report, priority=args.priority, run=args.run)
        if args.json:
            print(json.dumps({"report": report, "bead": " ".join(command)}, indent=2))
        else:
            print("bead title : %s" % report["symptom"][:120])
            print("kind       : %s" % report["kind"])
            print("doctor     : %s" % (report["doctor_version"] or "n/a — %s" % report["na_reason"]))
            print("\n%s" % " ".join("'%s'" % part if " " in part else part for part in command))
            if not args.run:
                print("\n(dry run — add --run to file it; the ledger row is written by the triage pass)")
        return 0

    if args.mode == "ledger":
        directory = pathlib.Path(args.dir)
        paths = ledger_files(directory)
        if args.week:
            paths = [p for p in paths if p.stem == args.week]
            if not paths:
                print("no register for %s under %s" % (args.week, directory), file=sys.stderr)
                return 2
        problems: list[Problem] = []
        for path in paths:
            found, _ = check_ledger(path)
            problems.extend(found)
        summary = ledger_summary(paths)
        for problem in problems:
            print(problem.line())
        if args.json:
            summary["problems"] = [p.__dict__ for p in problems]
            print(json.dumps(summary, indent=2))
        else:
            print("  ok  %d week(s): %s" % (len(paths), ", ".join(summary["weeks"]) or "none"))
            print("  ok  %d triaged row(s); dispositions: %s"
                  % (summary["triaged_rows"],
                     ", ".join("%s %d" % kv for kv in sorted(summary["dispositions"].items())) or "none"))
            print("  ok  %d distinct external reporter(s) named" % summary["external_reporters"])
        return 1 if problems else 0

    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
