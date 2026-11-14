#!/usr/bin/env python3
"""Check that the documentation stays internally consistent.

Guards the acceptance criteria of ADR-001 (docs/concepts/thesis.md):

  * every glossary term resolves to exactly one entry (no duplicates, no drift);
  * every plan reference in the docs (`e1.3`, `s2`, `m0`, `reco-982`) names something that exists in
    .plan/keymap.json, so the documents cannot cite tickets the plan no longer has;
  * every relative link in README.md and docs/** resolves to a file in the repo;
  * every link *definition* (`[label]: path`) resolves too, which is the half that bit: a reference-style
    link's target lives in a definition line no inline-link regex ever looks at, and a page whose
    `[ADR-002][]` points at a file that never existed rendered as a dead link forever;
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
ADR_NUMBERS = {"001", "002", "003", "004", "005", "006", "007"}

GLOSSARY_ENTRY = re.compile(r"^\*\*(.+?)\*\* —", re.M)
PLAN_KEY = re.compile(r"(?<![\w.-])([esdm]\d+(?:\.\d+)?)(?![\w-])")
BEAD_ID = re.compile(r"(?<![\w-])(reco-[a-z0-9]{3})(?![\w-])")
ADR_REF = re.compile(r"ADR-(\d{3})")
LINK = re.compile(r"\]\(([^)\s]+)\)")
#: A link reference definition: `[label]: target`, optionally indented, target possibly in `<>`.
LINK_DEFINITION = re.compile(r"^ {0,3}(?:> ?)*\[([^\]^]+)\]:\s*<?([^>\s]+)>?", re.M)

#: Strings a deleted surface printed, and the pages still allowed to print them. ADR-006 §5.1.2 kills the
#: Queue view, its ordering strip, its rank figure, its facade and its field names, and §7 names this gate
#: as the thing that keeps them dead: a documentation page that prints `order: risk × staleness × blast
#: radius` as though it were on screen is a page telling a reviewer to look for a strip that no longer
#: exists, and no test of the binary can catch that because the binary prints the right thing.
#:
#: The allowance is per page, not per phrase. ADR-006 keeps the layout of a pane it deleted and
#: [risk-ranking](../concepts/risk-ranking.md) keeps a retired ordering function's arithmetic — both are
#: records, deliberately, and an unversioned deletion of the record is the other way a page lies. A page
#: outside the list printing one of these is a page that has not been told the pane is gone.
RETIRED_TOKENS = {
    "order: risk × staleness × blast radius": {
        "docs/adr/0006-tui.md", "docs/concepts/risk-ranking.md"},
    "rank N of M": {"docs/adr/0006-tui.md", "docs/concepts/risk-ranking.md",
                    "docs/concepts/thesis.md"},
    "fathom queue": {"docs/adr/0006-tui.md"},
    "`q` queue": set(),
    "queue_open": set(),
    "fathom_analyze::queue": {"docs/adr/0006-tui.md", "docs/concepts/risk-ranking.md"},
    "fathom-tui::worklist": {"docs/adr/0006-tui.md", "docs/concepts/risk-ranking.md"},
    "rank_queue": {"docs/adr/0006-tui.md", "docs/concepts/risk-ranking.md"},
    "QueuePolicy": {"docs/adr/0006-tui.md", "docs/concepts/risk-ranking.md"},
    "QueuePlacement": {"docs/adr/0006-tui.md", "docs/concepts/risk-ranking.md"},
}


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


def check_link_definitions(path, text, problems):
    """Resolve every `[label]: target` definition the way inline links are resolved.

    The 1.0 gate found the hole by accident: `docs/concepts/ci-gate.md` defined `[ADR-002]` as
    `../adr/0002-rust-analyzer.md`, a file that has never existed in this repository, and every
    `[ADR-002][]` on the page has been a dead link ever since. `check_docs.py` said "docs OK" the whole
    time, because its link rule looks at `](target)` and a definition is not that. A reader who follows a
    dead link concludes the documentation is sloppy; a reader who does not is worse off.
    """
    for label, target in LINK_DEFINITION.findall(text):
        if target.startswith(("http://", "https://", "mailto:")):
            continue
        file_part = target.split("#", 1)[0]
        if not file_part:
            continue
        resolved = (path.parent / file_part).resolve()
        if not resolved.exists():
            problems.append(
                "%s: link definition [%s] points at %r, which does not exist"
                % (path.relative_to(ROOT), label, target)
            )


def check_retired_tokens(files, problems):
    """No page prints a retired surface's strings unless it is the page keeping its record.

    The other half of the rule is the exemption's own expiry: a page on the allow-list that no longer
    prints the token is a page whose record was rewritten, and a stale exemption lets the token back in on
    the first page that re-prints it. So both directions are problems.
    """
    printed = {token: set() for token in RETIRED_TOKENS}
    for path, text in files.items():
        rel = str(path.relative_to(ROOT))
        for token, allowed in RETIRED_TOKENS.items():
            if token not in text:
                continue
            printed[token].add(rel)
            if rel not in allowed:
                problems.append(
                    "%s: prints %r, which a deleted surface printed. %s"
                    % (rel, token,
                       "It is retired; say so in the sentence that uses it, on a page already carrying "
                       "the record (" + (", ".join(sorted(allowed)) or "no page is exempt yet") + ")."
                       if allowed else
                       "It is retired and no page is allowed to print it — say what replaced it instead."))
    for token, allowed in RETIRED_TOKENS.items():
        for page in sorted(allowed - printed[token]):
            if (ROOT / page).exists():
                problems.append(
                    "%s: is allowed to print %r and no longer does — drop the exemption, or the record "
                    "on this page went missing" % (page, token))


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
        check_link_definitions(path, text, problems)

    checked = {path: path.read_text(encoding="utf-8") for path in doc_files()}
    check_retired_tokens(checked, problems)

    for problem in problems:
        print(problem)
    if problems:
        print("\n%d problem(s)" % len(problems))
        return 1
    print("docs OK: %d glossary terms, %d documents checked, %d retired tokens held to their record pages"
          % (len(GLOSSARY_ENTRY.findall(thesis_text)), len(doc_files()), len(RETIRED_TOKENS)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
