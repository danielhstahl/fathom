#!/usr/bin/env python3
"""Check that the documentation stays internally consistent.

Guards the acceptance criteria of ADR-001 (docs/concepts/thesis.md):

  * every glossary term resolves to exactly one entry (no duplicates, no drift);
  * every plan reference in the docs (`e1.3`, `s2`, `m0`, `reco-982`) names something that exists in
    .plan/keymap.json, so the documents cannot cite tickets the plan no longer has;
  * every relative link in README.md and docs/** resolves to a file in the repo;
  * the README links the thesis document.

Run:  python3 tools/check_docs.py
Exit: 0 clean, 1 problems found (print one line per problem).
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
KEYMAP = ROOT / ".plan" / "keymap.json"
THESIS = ROOT / "docs" / "concepts" / "thesis.md"
DOC_GLOBS = ["README.md", "docs/**/*.md"]
ADR_NUMBERS = {"001", "002", "003", "004", "005", "006"}

GLOSSARY_ENTRY = re.compile(r"^\*\*(.+?)\*\* —", re.M)
PLAN_KEY = re.compile(r"(?<![\w.-])([esdm]\d+(?:\.\d+)?)(?![\w-])")
BEAD_ID = re.compile(r"(?<![\w-])(reco-[a-z0-9]{3})(?![\w-])")
ADR_REF = re.compile(r"ADR-(\d{3})")
LINK = re.compile(r"\]\(([^)\s]+)\)")


def doc_files():
    seen = []
    for pattern in DOC_GLOBS:
        for path in sorted(ROOT.glob(pattern)):
            if path not in seen:
                seen.append(path)
    return seen


def check_glossary(terms_text, problems):
    seen = {}
    for entry in GLOSSARY_ENTRY.findall(terms_text):
        # one entry may define slash synonyms ("A / B"); split them apart
        for part in re.split(r"\s*/\s*| \(|, ", entry):
            term = part.strip().strip("*`").lower()
            if not term:
                continue
            if term in seen:
                problems.append("glossary: term defined twice: %r" % term)
            seen[term] = entry
    if len(seen) < 60:
        problems.append("glossary: only %d terms defined; did a section go missing?" % len(seen))


def check_plan_refs(path, text, keymap, problems):
    keys, ids = set(keymap), set(keymap.values())
    for ref in PLAN_KEY.findall(text):
        if ref not in keys:
            problems.append("%s: unknown plan key %r (not in .plan/keymap.json)" % (path, ref))
    for ref in BEAD_ID.findall(text):
        if ref not in ids:
            problems.append("%s: unknown beads id %r (not in .plan/keymap.json)" % (path, ref))
    for ref in ADR_REF.findall(text):
        if ref not in ADR_NUMBERS:
            problems.append("%s: reference to ADR-%s, which the ADR index does not have" % (path, ref))


def check_links(path, text, problems):
    for target in LINK.findall(text):
        if target.startswith(("http://", "https://", "mailto:")):
            continue
        file_part = target.split("#", 1)[0]
        if not file_part:
            continue
        resolved = (path.parent / file_part).resolve()
        if not resolved.exists():
            problems.append("%s: broken link %r" % (path.relative_to(ROOT), target))


def main():
    problems = []
    if not KEYMAP.exists():
        print("missing .plan/keymap.json; cannot verify plan references")
        return 1
    keymap = json.loads(KEYMAP.read_text())
    if not THESIS.exists():
        print("missing docs/concepts/thesis.md")
        return 1

    thesis_text = THESIS.read_text(encoding="utf-8")
    check_glossary(thesis_text, problems)

    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    if "docs/concepts/thesis.md" not in readme:
        problems.append("README.md: does not link docs/concepts/thesis.md")

    for path in doc_files():
        text = path.read_text(encoding="utf-8")
        check_plan_refs(path.relative_to(ROOT), text, keymap, problems)
        check_links(path, text, problems)

    for problem in problems:
        print(problem)
    if problems:
        print("\n%d problem(s)" % len(problems))
        return 1
    print("docs OK: %d glossary terms, %d documents checked"
          % (len(GLOSSARY_ENTRY.findall(thesis_text)), len(doc_files())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
