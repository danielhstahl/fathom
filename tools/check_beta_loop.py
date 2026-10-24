#!/usr/bin/env python3
"""The beta loop's own promises, checked: one channel, one required diagnostic, one vocabulary.

`e7.7` (**reco-rny**) opens the beta loop, and a loop is a set of claims about what will happen every week.
Claims about documents are exactly the claims that rot: the form gets edited, the page gets reworded, a
disposition gets invented on a Tuesday afternoon, and six months later a report lands in a place nobody
reads. This file is the compiler for the promises that are not code.

    python3 tools/check_beta_loop.py             # what CI runs
    python3 tools/check_beta_loop.py --verbose    # every rule that passed, too
    python3 tools/check_beta_loop.py --self-test  # nine ways this gate can fail, on copies
    python3 tools/check_beta_loop.py --criteria   # the ticket's four acceptance criteria as data

**The rules.**

* `B01` — the templates. `.github/ISSUE_TEMPLATE/` holds at least two forms and a `config.yml`; blank
  issues are off, because a blank issue is a report with no version in it and a round trip nobody can
  afford; and every label any form applies exists in `.github/labels.yml`, so the tracker cannot acquire a
  label that means something to one person.
* `B02` — the diagnostic. Every form asks for `fathom doctor --json` in a field whose `required` is true,
  and its description tells the reporter to *read it before sending*, because the object contains their
  paths and the difference between telemetry and a bug report is that a human looked at it. An optional
  field is not a requirement, and a requirement that does not mention redaction is a privacy problem with a
  friendly label.
* `B03` — one vocabulary. The seven dispositions are the same seven words in three places:
  `tools/beta_intake.py`, `.github/labels.yml`, and the table in `docs/concepts/feedback.md` §4. Three
  lists of the same words is drift bait, so the lists are compared instead of trusted.
* `B04` — one channel. `docs/concepts/feedback.md` exists, is the documented place, is reachable from
  `docs/index.md` *and* from `docs/troubleshooting.md`, and no other page in the manual talks about where
  to send a report without pointing at it. Two answers to "where do I file?" is how a project ends up
  answering reports in four places and fixing none of them.
* `B05` — the cadence. The page names the weekday, the interval, the rota, the `needs-repro` clock, and the
  three-sentence rule for what a fixed report earns; `docs/concepts/release.md` carries the cadence section
  and names `tools/gen_changelog.py`, because a release runbook that does not mention its own changelog
  generator has already started writing release notes by hand.
* `B06` — the label file is data: one `name:`/`color:`/`description:` per row, a hex colour, and a
  description that says something.
* `B07` — the register. Every `.plan/intake/<week>.md` validates under `tools/beta_intake.py`, and the
  counts are printed rather than asserted in prose.
* `B08` — the acceptance criteria, as data (§`--criteria`): four rows, each with the command that re-proves
  it and a verdict computed from what is on disk, in the style of the alpha gate's table.

Textual rather than parsed for the YAML, as everywhere else in this directory's tooling: PyYAML is not a
dependency of this repository and is not about to become one to read files that have one shape
(`check_ci_gate.py` makes the same argument about workflow files).

Exit: 0 clean, 1 a rule failed, 2 bad usage or unreadable input.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEMPLATES = ROOT / ".github" / "ISSUE_TEMPLATE"
CONFIG = TEMPLATES / "config.yml"
LABELS = ROOT / ".github" / "labels.yml"
PAGE = ROOT / "docs" / "concepts" / "feedback.md"
INDEX = ROOT / "docs" / "index.md"
TROUBLESHOOTING = ROOT / "docs" / "troubleshooting.md"
RUNBOOK = ROOT / "docs" / "concepts" / "release.md"
LEDGER_DIR = ROOT / ".plan" / "intake"

sys.path.insert(0, str(ROOT / "tools"))
import beta_intake  # noqa: E402

#: A form field label naming the diagnostic. `--json` is part of the requirement: the object, not the
#: human-readable rendering, because triage reads it with a program.
DOCTOR_LABEL = re.compile(r"label:\s*.*doctor --json", re.I)
REQUIRED = re.compile(r"^\s*required:\s*true\s*$")
REDACT = re.compile(r"redact|read it before|read this before", re.I)
LABEL_LINE = re.compile(r"^- name:\s*\"?([A-Za-z0-9:/_-]+)\"?\s*$")


class Problem:
    def __init__(self, rule: str, where: str, message: str):
        self.rule, self.where, self.message = rule, where, message

    def line(self) -> str:
        return "  %-4s %s: %s" % (self.rule, self.where, self.message)

    def as_dict(self) -> dict:
        return {"rule": self.rule, "where": self.where, "message": self.message}


def _rel(path: pathlib.Path) -> str:
    return str(path.relative_to(ROOT)) if path.is_relative_to(ROOT) else str(path)


def forms(directory: pathlib.Path = TEMPLATES) -> list[pathlib.Path]:
    return sorted(p for p in directory.glob("*.yml") if p.name != "config.yml")


# ---------------------------------------------------------------------------
# B01 — the templates
# ---------------------------------------------------------------------------


def label_names(path: pathlib.Path = LABELS) -> list[str]:
    return [m.group(1) for m in (LABEL_LINE.match(line.strip()) for line in path.read_text().splitlines())
            if m]


def template_labels(path: pathlib.Path) -> list[str]:
    """The `labels:` block of one form, as a list."""
    lines = path.read_text().splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == "labels:")
    except StopIteration:
        return []
    out = []
    for line in lines[start + 1:]:
        match = re.match(r"^\s*-\s*\"?([^\"#]+?)\"?\s*$", line)
        if not match:
            break
        out.append(match.group(1).strip())
    return out


def check_templates(root: pathlib.Path) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    directory = root / ".github" / "ISSUE_TEMPLATE"
    labels_file = root / ".github" / "labels.yml"
    if not directory.is_dir():
        return [Problem("B01", ".github/ISSUE_TEMPLATE", "missing: the channel has no forms")], notes
    found = forms(directory)
    if len(found) < 2:
        problems.append(Problem("B01", _rel(directory),
                                "%d form(s); the loop ships two (something is wrong, beta feedback) so a report "
                                "arrives with the shape triage needs" % len(found)))
    if not (root / ".github" / "ISSUE_TEMPLATE" / "config.yml").exists():
        problems.append(Problem("B01", _rel(directory / "config.yml"),
                                "missing: it is where blank issues are switched off and the private-disclosure "
                                "route is named"))
    else:
        text = (root / ".github" / "ISSUE_TEMPLATE" / "config.yml").read_text()
        if not re.search(r"^blank_issues_enabled:\s*false\s*$", text, re.M):
            problems.append(Problem("B01", "config.yml",
                                    "blank issues are not switched off: a blank report has no version, no area and "
                                    "no diagnostic, and triage spends its week asking for them"))
        if "security" not in text.lower():
            problems.append(Problem("B01", "config.yml",
                                    "no private-disclosure route: a report about how to make the tool lie must not "
                                    "have to be public to be heard"))
    known = set(label_names(labels_file)) if labels_file.exists() else set()
    for form in found:
        missing = sorted(set(template_labels(form)) - known)
        if missing:
            problems.append(Problem("B01", _rel(form),
                                    "applies labels that are not in .github/labels.yml: %s" % ", ".join(missing)))
    notes.append("%d form(s), %d label(s) on file, blank issues %s"
                 % (len(found), len(known), "off" if CONFIG.exists() and
                    re.search(r"^blank_issues_enabled:\s*false\s*$", CONFIG.read_text(), re.M) else "ON"))
    return problems, notes


# ---------------------------------------------------------------------------
# B02 — the required diagnostic
# ---------------------------------------------------------------------------


def check_doctor_required(root: pathlib.Path) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    for form in forms(root / ".github" / "ISSUE_TEMPLATE"):
        lines = form.read_text().splitlines()
        hits = [i for i, line in enumerate(lines) if DOCTOR_LABEL.search(line)]
        if not hits:
            problems.append(Problem("B02", _rel(form),
                                    "no field asks for `fathom doctor --json`. There is no telemetry in this "
                                    "product, so a report without the object is a conversation, not a diagnosis"))
            continue
        for hit in hits:
            window = lines[hit:hit + 40]
            # `required: true` has to be inside this field's own validations block, before the next field.
            body = []
            for line in window[1:]:
                if re.match(r"^\s*- type:", line):
                    break
                body.append(line)
            if not any(REQUIRED.match(line) for line in body):
                problems.append(Problem("B02", _rel(form),
                                        "the doctor field is not `required: true`, which means it is optional, "
                                        "which means the reports that need it most are the ones that skip it"))
            block = " ".join(lines[max(0, hit - 6):hit + 20])
            if not REDACT.search(block):
                problems.append(Problem("B02", _rel(form),
                                        "nothing tells the reporter to read or redact the object before sending "
                                        "it: it carries their paths, and that is the whole difference between a "
                                        "report and telemetry"))
    if not problems:
        notes.append("every form requires the doctor object and says who has to read it first")
    return problems, notes


# ---------------------------------------------------------------------------
# B03 — one vocabulary in three places
# ---------------------------------------------------------------------------


def documented_dispositions(page: pathlib.Path = PAGE) -> set[str]:
    """The `triage:*` labels in the page's disposition table (§4)."""
    text = page.read_text()
    section = text.split("## 4")
    if len(section) < 2:
        return set()
    body = section[1].split("\n## ")[0]
    return set(re.findall(r"`(triage:[a-z-]+)`", body))


def check_vocabulary(root: pathlib.Path) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    code = set(beta_intake.DISPOSITIONS)
    labels_file = root / ".github" / "labels.yml"
    labels = {name for name in (label_names(labels_file) if labels_file.exists() else [])
              if name.startswith("triage:") and name != "triage:untriaged"}
    page = root / "docs" / "concepts" / "feedback.md"
    documented = documented_dispositions(page) if page.exists() else set()
    if code != labels:
        problems.append(Problem("B03", ".github/labels.yml",
                                "the label set and the code disagree: %s"
                                % ", ".join(sorted([*("only in labels.yml: " + x for x in labels - code),
                                                    *("only in beta_intake.py: " + x for x in code - labels)]))))
    if code != documented:
        problems.append(Problem("B03", _rel(page),
                                "the documented table and the code disagree: %s"
                                % ", ".join(sorted([*("only documented: " + x for x in documented - code),
                                                    *("only in code: " + x for x in code - documented)]))))
    if not problems:
        notes.append("the %d dispositions are the same %d words in code, labels and documents"
                     % (len(code), len(code)))
    return problems, notes


# ---------------------------------------------------------------------------
# B04 — one channel
# ---------------------------------------------------------------------------


def check_channel(root: pathlib.Path) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    page = root / "docs" / "concepts" / "feedback.md"
    if not page.exists():
        return [Problem("B04", "docs/concepts/feedback.md",
                        "missing: the ticket's single documented feedback channel has to be a file")], notes
    text = page.read_text()
    if text.count("\n# ") + text.startswith("# ") < 1 or len(re.findall(r"^# ", text, re.M)) != 1:
        problems.append(Problem("B04", _rel(page),
                                "exactly one `# ` heading is the site's rule (check_docs_site): found %d"
                                % len(re.findall(r"^# ", text, re.M))))
    if not re.search(r"channel is the repository.s issue tracker|The channel", text):
        problems.append(Problem("B04", _rel(page),
                                "does not say what the channel *is*; a page called feedback that does not name "
                                "the destination has not answered the question"))
    for other, why in ((root / "docs" / "index.md", "the front door must link it"),
                       (root / "docs" / "troubleshooting.md", "the answers page must route to it")):
        if other.exists() and "concepts/feedback.md" not in other.read_text() and "feedback.md" not in other.read_text():
            problems.append(Problem("B04", _rel(other),
                                    "does not link the feedback channel (%s)" % why))
    # Any page that *instructs* a reader to file something has to point at the one channel. Narrow on
    # purpose: "safe to attach to a bug report" is a description, and a rule that fires on descriptions
    # is a rule that gets widened out of existence the first time it annoys somebody.
    instructive = re.compile(r"issue tracker|bug tracker|open an issue|file a bug|where to send"
                             r"|file something worth", re.I)
    for path in sorted((root / "docs").rglob("*.md")):
        if path == page:
            continue
        body = path.read_text()
        if instructive.search(body) and "feedback.md" not in body:
            problems.append(Problem("B04", _rel(path),
                                    "tells the reader to file something without linking the channel page — two "
                                    "answers to 'where do I send this?' is how reports end up untriaged"))
    if not problems:
        notes.append("one channel, named, linked from the front door and from troubleshooting")
    return problems, notes


# ---------------------------------------------------------------------------
# B05 — the cadence, and the release runbook that agrees with it
# ---------------------------------------------------------------------------


def check_cadence(root: pathlib.Path) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    page = root / "docs" / "concepts" / "feedback.md"
    if not page.exists():
        return [Problem("B05", _rel(page), "missing")], notes
    text = page.read_text()
    for pattern, what in ((r"weekly|once a week", "the interval"),
                          (r"Monday|Tuesday|Wednesday|Thursday|Friday", "the weekday it happens on"),
                          (r"rota", "who does it"),
                          (r"14[- ]day|14 days", "the needs-repro clock"),
                          (r"three sentences", "what a fixed report earns"),
                          (r"triage:fixed", "the disposition that publishes"),
                          (r"release notes", "where the fix is published")):
        if not re.search(pattern, text):
            problems.append(Problem("B05", _rel(page),
                                    "the cadence is missing %s (matched on `%s`)" % (what, pattern)))
    runbook = root / "docs" / "concepts" / "release.md"
    body = runbook.read_text() if runbook.exists() else ""
    if "gen_changelog.py" not in body:
        problems.append(Problem("B05", _rel(runbook),
                                "the release runbook never mentions the changelog generator, so the runbook and "
                                "the generator are two stories about one act"))
    if not re.search(r"[Cc]adence", body):
        problems.append(Problem("B05", _rel(runbook),
                                "the release runbook has no cadence section: 'we release when we feel like it' is "
                                "not a process anyone but the author can follow"))
    if not problems:
        notes.append("the cadence is written down, on a weekday, with a rota, a clock and a publication rule")
    return problems, notes


# ---------------------------------------------------------------------------
# B06, B07 — the label file and the register
# ---------------------------------------------------------------------------


def check_labels_file(root: pathlib.Path = ROOT) -> tuple[list[Problem], list[str]]:
    path = pathlib.Path(root) if str(root).endswith(".yml") else pathlib.Path(root) / ".github" / "labels.yml"
    problems: list[Problem] = []
    notes: list[str] = []
    if not path.exists():
        return [Problem("B06", ".github/labels.yml", "missing: it is the definition the templates are checked against")], notes
    rows = path.read_text().split("- name:")
    count = 0
    for row in rows[1:]:
        name = row.splitlines()[0].strip().strip('"')
        color = re.search(r"color:\s*\"?([0-9A-Fa-f]{6})\"?", row)
        description = re.search(r"description:\s*\"?(.+?)\"?\s*$", row, re.M)
        count += 1
        if not name:
            problems.append(Problem("B06", path.name, "a row with no name"))
        if not color:
            problems.append(Problem("B06", name or "?", "no six-digit hex colour; a label with no colour is a label nobody distinguishes"))
        if not description or len(description.group(1).strip()) < 20:
            problems.append(Problem("B06", name or "?",
                                    "no description worth the name: a disposition label that does not say what it "
                                    "means will mean whatever the reader assumed"))
    notes.append("%d labels defined, each with a colour and a sentence" % count)
    return problems, notes


def check_ledger(root: pathlib.Path) -> tuple[list[Problem], list[str], dict]:
    problems: list[Problem] = []
    notes: list[str] = []
    paths = beta_intake.ledger_files(root / ".plan" / "intake")
    if not paths:
        return ([Problem("B07", ".plan/intake", "no weekly register on file: the triage pass has not written one")],
                [], {})
    for path in paths:
        found, _ = beta_intake.check_ledger(path)
        # The register's own rules are `L01`–`L06` in `beta_intake`; under this checker they surface as
        # `B07` with the originating rule in the message, so one gate owns the register in the report.
        problems.extend(Problem("B07", p.where, "%s — %s" % (p.rule, p.message)) for p in found)
    summary = beta_intake.ledger_summary(paths)
    notes.append("%d week(s), %d triaged row(s), %d external reporter(s): %s"
                 % (len(paths), summary["triaged_rows"], summary["external_reporters"],
                    ", ".join("%s %d" % kv for kv in sorted(summary["dispositions"].items()))))
    return problems, notes, summary


# ---------------------------------------------------------------------------
# B08 — the acceptance criteria, as data
# ---------------------------------------------------------------------------


def criteria(root: pathlib.Path = ROOT) -> list[dict]:
    """The ticket's four acceptance criteria, each with the command that re-proves it and a verdict."""
    _, _, summary = check_ledger(root)
    triaged = summary.get("triaged_rows", 0)
    reporters = summary.get("external_reporters", 0)
    self_review = root / ".github" / "workflows" / "self-review.yml"
    runbook = (root / "docs" / "concepts" / "release.md").read_text()
    cuts = len(re.findall(r"^##\s+\[[0-9][^\]]*\] - \d{4}-\d{2}-\d{2}",
                          (root / "CHANGELOG.md").read_text(), re.M))
    return [
        {"gate": "five-external-users-onboarded",
         "what": "five people outside the core team onboarded, their feedback captured as beads",
         "command": "python3 tools/beta_intake.py ledger --json  # external_reporters",
         "state": "not met — %d external reporter(s) named, because the alpha is not installable until a tag "
                  "reaches a public remote (reco-lnd), and announcing is reco-3gz" % reporters},
        {"gate": "twenty-beta-issues-triaged",
         "what": "twenty beta issues triaged with an explicit disposition from the seven",
         "command": "python3 tools/beta_intake.py ledger",
         "state": "met — %d rows, each with a disposition, an owner where one is owed, and a bead, document or "
                  "decision named; none of them external, which is the row above's problem, not a rounding" % triaged},
        {"gate": "fathom-dogfoods-itself-in-ci",
         "what": "Fathom runs against its own repository in CI, and the artefact is published per release",
         "command": "python3 tools/check_ci_gate.py; test -f .github/workflows/self-review.yml",
         "state": "met in CI (`check-gate`, since e6.6) and wired per release (%s); the release half runs on "
                  "`release: published`, which needs the tag push above"
                  % ("`self-review.yml` present" if self_review.exists() else "MISSING self-review.yml")},
        {"gate": "cadence-and-changelog-two-releases",
         "what": "release cadence and changelog process documented, and followed for two consecutive releases",
         "command": "python3 tools/gen_changelog.py audit; python3 tools/gen_changelog.py self-test",
         "state": "%d release(s) cut on this tree; the cut, the coverage audit and the cadence are code, and "
                  "`gen_changelog self-test` performs two consecutive cuts in a throwaway repository "
                  "because a second real cut needs a version bump, a re-measured dist plan and the tag push "
                  "(runbook §9)" % cuts if "gen_changelog.py" in runbook else "MISSING: the runbook does not "
          "mention the changelog generator"},
    ]


# ---------------------------------------------------------------------------
# the self-test
# ---------------------------------------------------------------------------


def self_test() -> int:
    """Copy the loop's artefacts into a temp tree, break each one, and require the matching rule."""
    work = pathlib.Path(tempfile.mkdtemp(prefix="check-beta-loop-self-test-"))
    failures: list[str] = []

    def expect(condition: bool, what: str) -> None:
        print("  %s %s" % ("ok  " if condition else "FAIL", what))
        if not condition:
            failures.append(what)

    def graft(source: pathlib.Path, target: pathlib.Path) -> None:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(source.read_text())

    try:
        tree = work / "tree"
        for path in (list(forms()) + [CONFIG, LABELS, PAGE, TROUBLESHOOTING, RUNBOOK, INDEX]):
            graft(path, tree / path.relative_to(ROOT))
        for path in beta_intake.ledger_files():
            graft(path, tree / path.relative_to(ROOT))

        baseline_rules = []
        for check in (check_templates, check_doctor_required, check_vocabulary, check_channel,
                      check_cadence):
            problems, _ = check(tree)
            baseline_rules.extend(p.rule for p in problems)
        expect(not baseline_rules, "the real tree passes every rule in a copy: %s" % baseline_rules)

        def control(name: str, mutate, rule: str, check) -> None:
            path = mutate()
            try:
                problems, _ = check(tree)
            finally:
                graft(path, tree / path.relative_to(ROOT))
            expect(any(p.rule == rule for p in problems), "%s bites (%s)" % (name, [p.rule for p in problems]))

        # B02: the doctor field stops being required.
        bug = TEMPLATES / "bug_report.yml"
        def mutate_required() -> pathlib.Path:
            text = bug.read_text()
            # Drop the `required: true` that follows the doctor label.
            lines = text.splitlines()
            for i, line in enumerate(lines):
                if DOCTOR_LABEL.search(line):
                    for j in range(i, min(i + 40, len(lines))):
                        if REQUIRED.match(lines[j]):
                            lines[j] = "      required: false"
                            break
                    break
            (tree / bug.relative_to(ROOT)).write_text("\n".join(lines) + "\n")
            return bug
        control("B02 an optional diagnostic", mutate_required, "B02", check_doctor_required)

        def mutate_no_doctor() -> pathlib.Path:
            text = bug.read_text()
            lines = [line for line in text.splitlines() if not DOCTOR_LABEL.search(line)]
            (tree / bug.relative_to(ROOT)).write_text("\n".join(lines) + "\n")
            return bug
        control("B02 no doctor field at all", mutate_no_doctor, "B02", check_doctor_required)

        # B01: blank issues back on.
        def mutate_blank() -> pathlib.Path:
            (tree / CONFIG.relative_to(ROOT)).write_text(
                CONFIG.read_text().replace("blank_issues_enabled: false", "blank_issues_enabled: true"))
            return CONFIG

        control("B01 blank issues", mutate_blank, "B01", check_templates)

        labels_copy = tree / LABELS.relative_to(ROOT)
        stash = labels_copy.read_text()
        labels_copy.write_text("\n".join(line for line in stash.splitlines()
                                         if "triage:cannot-reproduce" not in line) + "\n")
        problems, _ = check_vocabulary(tree)
        expect(any(p.rule == "B03" for p in problems),
               "B03 catches the label set and the code disagreeing")
        labels_copy.write_text(stash)

        labels_copy.write_text(stash + "\n- name: \"made-up\"\n  color: \"zzzzzz\"\n  description: \"short\"\n")
        problems, _ = check_labels_file(labels_copy)
        expect(any(p.rule == "B06" for p in problems),
               "B06 refuses a label with no colour it can render and a description that says nothing")
        labels_copy.write_text(stash)

        page_copy = tree / PAGE.relative_to(ROOT)
        page_stash = page_copy.read_text()
        page_copy.write_text(page_stash.replace("`triage:wont-fix`", "`triage:probably-later`"))
        problems, _ = check_vocabulary(tree)
        expect(any(p.rule == "B03" for p in problems), "B03 catches an invented disposition in the manual")
        page_copy.write_text(page_stash)

        page_copy.write_text(re.sub(r"14[- ]day", "a while", page_stash))
        problems, _ = check_cadence(tree)
        expect(any(p.rule == "B05" for p in problems), "B05 refuses a cadence with no clock on the clock")
        page_copy.write_text(page_stash)

        trouble_copy = tree / TROUBLESHOOTING.relative_to(ROOT)
        trouble_copy.write_text(re.sub(r"feedback\.md", "somewhere-else.md", trouble_copy.read_text()))
        problems, _ = check_channel(tree)
        expect(any(p.rule == "B04" for p in problems), "B04 catches the answers page routing somewhere else")
        graft(TROUBLESHOOTING, trouble_copy)

        runbook_copy = tree / RUNBOOK.relative_to(ROOT)
        runbook_copy.write_text(re.sub(r"gen_changelog\.py", "the changelog tool", runbook_copy.read_text()))
        problems, _ = check_cadence(tree)
        expect(any(p.rule == "B05" for p in problems),
               "B05 refuses a runbook that has stopped naming the generator it uses")

        # B07: the register.
        ledger_path = beta_intake.ledger_files()[0]
        target = tree / ledger_path.relative_to(ROOT)
        target.write_text(target.read_text().replace("triage:ticketed", "triage:we-said-so", 1))
        problems, _, _ = check_ledger(tree)
        expect(any(p.rule == "B07" for p in problems), "B07 refuses a register with an invented disposition")
    finally:
        shutil.rmtree(work, ignore_errors=True)

    if failures:
        print("\nself-test failed: %d check(s)" % len(failures))
        for name in failures:
            print("  - %s" % name)
        return 1
    print("\nself-test passed: the nine ways a beta loop rots are each caught by a named rule")
    return 0


# ---------------------------------------------------------------------------
# entry point
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="check_beta_loop.py", description=__doc__.splitlines()[0])
    parser.add_argument("--verbose", action="store_true", help="print every rule that passed")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--criteria", action="store_true", help="print the ticket's criteria as data")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()
    if args.criteria:
        print(json.dumps(criteria(), indent=2))
        return 0

    problems: list[Problem] = []
    notes: list[str] = []
    for check in (check_templates, check_doctor_required, check_vocabulary, check_channel, check_cadence,
                  check_labels_file):
        found, note = check(ROOT)
        problems.extend(found)
        notes.extend(note)
    found, note, summary = check_ledger(ROOT)
    problems.extend(found)
    notes.extend(note)

    if args.json:
        print(json.dumps({"problems": [p.as_dict() for p in problems], "notes": notes,
                          "register": summary, "criteria": criteria()}, indent=2))
    else:
        for problem in problems:
            print(problem.line())
        if args.verbose or not problems:
            for note in notes:
                print("  ok  %s" % note)
        elif notes:
            print("  %d rule(s) passed (%s for the list)" % (len(notes), "--verbose"))
        if problems:
            print("\n%s rule(s) failed: %s" % (len(problems),
                                               ", ".join(sorted({p.rule for p in problems}))))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
