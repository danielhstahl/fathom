#!/usr/bin/env python3
"""Validate the relink ground-truth corpus in `crates/fathom-fixtures/corpus/relinks/` — without building.

The harness (`crates/fathom-analyze/examples/s6_spike.rs`) already refuses to score a corpus whose
annotations do not resolve, and it is the authority: it recomputes the after-image and checks every
`expect` against it. This script is the cheap pre-flight, the same role `tools/author_turn_corpus.py`
plays for `corpus/turns/`: it costs no compile, so an annotation edit can be checked in two seconds
instead of ninety, and it checks the things the harness *cannot* see — that the corpus README, which is
the normative statement of the vocabularies, still matches the files, and that the annotations that
require a reason carry one.

    python3 tools/author_relink_corpus.py --validate-only

The checks, in order of the lie each one prevents:

* structure    — the fields the harness reads exist, and nothing else pretends to be truth
* vocabularies — every `family`, `op.kind`, `relation` and `catch` is one the README publishes. The README
                 is parsed for them: if a family were added to a file and not to the table, the table would
                 be documentation about a subset of the corpus, which is how vocabularies rot
* references   — every `history` names a history in `corpus/turns/` (or `*`), and event ids are unique
                 across files, because the report keys its per-family rows by them
* R7           — every silent expectation (`to: null`) and every non-default `catch` carries a note of at
                 least a sentence: a judgement call without its reason is not an annotation
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CORPUS = ROOT / "crates" / "fathom-fixtures" / "corpus" / "relinks"
TURNS = ROOT / "crates" / "fathom-fixtures" / "corpus" / "turns"
README = CORPUS / "README.md"

CATCHES = ("tier-1", "tier-2", "tier-3", "none")
RELATIONS = ("amend", "rebase", "squash", "split", "cherry", "merge", "other")
EXPANDS = ("each-turn",)
MIN_NOTE_WORDS = 12


def table_rows(text: str, begin: str, stop: str) -> list[list[str]]:
    """The rows of one README table, by section heading, so the document stays the source of the lists."""
    section = text.split(begin, 1)
    if len(section) != 2:
        return []
    body = section[1].split(stop, 1)[0]
    rows = []
    for line in body.splitlines():
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if cells and set("".join(cells)) <= set("-: "):
            continue
        rows.append(cells)
    return rows


def readme_vocabularies() -> tuple[set[str], set[str]]:
    """The families and op kinds the README publishes, read out of its own tables.

    Parsed rather than copied into this file: if a family were added to a corpus file and not to the table,
    the table would become documentation about a subset of the corpus, which is how a closed vocabulary
    rots.
    """
    text = README.read_text()
    families = {r[0].strip("`") for r in table_rows(text, "## 3. The families", "## 4.")
                if len(r) >= 2 and r[0].startswith("`") and r[0].endswith("`")}
    kinds = set()
    for r in table_rows(text, "## 2. The rewrites", "## 3."):
        token = r[0].strip("`") if r and r[0].startswith("`") and r[0].endswith("`") else ""
        if token and token != "kind" and re.fullmatch(r"[a-z][a-z0-9-]*(\+[a-z0-9-]+)*", token):
            kinds.add(token)
    return families, kinds


def validate() -> list[str]:
    problems: list[str] = []
    if not README.exists():
        return [f"no corpus README at {README}: the vocabularies have no source"]
    families, kinds = readme_vocabularies()
    if not families or not kinds:
        return [f"{README}: could not read the family and op-kind vocabularies out of its tables"]
    histories = {p.stem for p in TURNS.glob("*.json")}
    if not histories:
        problems.append(f"no turn histories under {TURNS}: the operands of every rewrite are missing")

    seen_ids: dict[str, str] = {}
    files = sorted(CORPUS.glob("*.json"))
    if not files:
        return [f"no corpus files under {CORPUS}"]
    for path in files:
        doc = json.loads(path.read_text())
        where = path.relative_to(ROOT)
        for field in ("schema", "file", "protocol", "authored_by", "claim", "events"):
            if field not in doc:
                problems.append(f"{where}: missing `{field}`")
        if doc.get("schema") != "fathom.relink-ground-truth/1":
            problems.append(f"{where}: schema is {doc.get('schema')!r}, not the version the harness reads")
        if doc.get("file") != path.stem:
            problems.append(f"{where}: `file` says {doc.get('file')!r}, the filename says {path.stem!r}")
        if len(str(doc.get("claim", "")).split()) < 15:
            problems.append(f"{where}: a file-level claim is what says what this file is for; write it")
        for event in doc.get("events", []):
            eid = event.get("id", "?")
            here = f"{where} · {eid}"
            if eid in seen_ids:
                problems.append(f"{here}: event id already used in {seen_ids[eid]}")
            seen_ids[eid] = str(where)
            if event.get("family") not in families:
                problems.append(f"{here}: family {event.get('family')!r} is not in README §3")
            if event.get("relation") not in RELATIONS:
                problems.append(f"{here}: relation {event.get('relation')!r} is not ADR-003 §6.4's vocabulary")
            if event.get("history") != "*" and event.get("history") not in histories:
                problems.append(f"{here}: history {event.get('history')!r} is not in corpus/turns/")
            if event.get("expand") not in (None, *EXPANDS):
                problems.append(f"{here}: expand {event.get('expand')!r} is not a spelled-out expansion")
            op = event.get("op", {})
            if op.get("kind") not in kinds:
                problems.append(f"{here}: op.kind {op.get('kind')!r} is not in README §2")
            if "expect" not in event and "expect_rule" not in event:
                problems.append(f"{here}: an event with no `expect` and no `expect_rule` asserts nothing")
            if event.get("expand") == "each-turn" and "expect" in event:
                if any(x["from"] != "<target>" for x in event["expect"]):
                    problems.append(
                        f"{here}: an each-turn event must state its truth about the target turn, "
                        "not about a fixed turn id")
            for expectation in event.get("expect", []):
                source, target = expectation.get("from"), expectation.get("to")
                if not source:
                    problems.append(f"{here}: an `expect` with no `from` is not a claim about anything")
                if "catch" not in expectation:
                    problems.append(f"{here}/{source}: no `catch` — the annotation must say who could catch it")
                elif expectation["catch"] not in CATCHES:
                    problems.append(f"{here}/{source}: catch {expectation['catch']!r} is not in README §3")
                note = str(expectation.get("note", ""))
                needs_note = target is None or expectation.get("catch") in ("none", "tier-3")
                if needs_note and len(note.split()) < MIN_NOTE_WORDS:
                    problems.append(
                        f"{here}/{source}: R7 — a silent expectation or a non-default catch owes its reason "
                        f"in at least {MIN_NOTE_WORDS} words")
                for side in ("from", "to"):
                    value = expectation.get(side)
                    if value is None:
                        continue
                    for token in str(value).split("+"):
                        if token == "<target>" or token.startswith("nth:"):
                            continue
                        if not re.fullmatch(r"(t\d{1,2}(#\d+)?|ep-[a-z_]+|nth:\d+)", token):
                            problems.append(f"{here}/{source}: {side} token {token!r} resolves to nothing")
            rule = event.get("expect_rule")
            if rule is not None:
                if not isinstance(rule, dict) or not rule:
                    problems.append(f"{here}: `expect_rule` with no arms")
                else:
                    for arm_name, arm in rule.items():
                        if arm.get("catch") not in CATCHES:
                            problems.append(f"{here}: rule arm {arm_name!r} has no legal `catch`")
                        if len(str(arm.get("note", "")).split()) < MIN_NOTE_WORDS:
                            problems.append(
                                f"{here}: rule arm {arm_name!r} speaks for many episodes at once, so it "
                                "owes its reason in writing")
    unused = sorted(families - {e["family"] for path in files
                                for e in json.loads(path.read_text()).get("events", [])})
    if unused:
        problems.append(f"README §3 publishes families the corpus never uses: {', '.join(unused)}")
    unused_kinds = sorted(kinds - {e["op"]["kind"] for path in files
                                   for e in json.loads(path.read_text()).get("events", [])
                                   if "op" in e})
    if unused_kinds:
        problems.append(f"README §2 publishes op kinds the corpus never uses: {', '.join(unused_kinds)}")
    return problems


def main(argv: list[str]) -> int:
    if argv and argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    problems = validate()
    files = sorted(CORPUS.glob("*.json"))
    events = sum(len(json.loads(p.read_text()).get("events", [])) for p in files)
    if problems:
        for problem in problems:
            print(f"relink corpus: {problem}")
        print(f"relink corpus: {len(problems)} problem(s) over {events} events in {len(files)} files")
        return 1
    print(f"relink corpus: {events} rewrite events in {len(files)} files, every annotation resolves "
          "against the published vocabularies")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
