#!/usr/bin/env python3
"""Check the documentation site: reachability, anchors, and that no shipped thing is undocumented.

`tools/check_docs.py` guards the glossary, plan references and file-level links. This file guards the
*site* — the structure this documentation is supposed to have, and the acceptance criterion that a
generated page cannot quietly stop covering something:

    python3 tools/check_docs_site.py            # what CI runs
    python3 tools/check_docs_site.py --verbose  # print every check that passed

Five groups of rules, named as they are named here so a failure message points at the rule:

**Site shape.** `docs/index.md` is the front door; every page under `docs/` is reachable from it (or from
`README.md`) by following links, has exactly one `# ` heading, and contains no `TODO`/`TBD` left standing
in as prose.

**Anchors.** A fragment link — `[x](page.md#section)` or `[x](#section)` — resolves to a heading in the
target, computed the way a markdown renderer computes it. A page that is *mentioned* by a link with a
fragment and does not have it is a silent lie, and file-level link checking cannot see it.

**Generated pages are fresh and complete.** The four generated pages exist, and the generator reports no
drift for them (the same call CI makes, so `check_docs_site.py` alone is enough to catch a stale page).

**No shipped feature without a doc entry.** The enumerations come from the program, not from a list
someone maintains:

| thing | enumerated from | must appear in |
|---|---|---|
| verbs (and their subverbs) | `enum Verb` and clap's `Commands:` blocks in `crates/fathom-cli/src/verbs.rs` | `docs/reference/cli.md` |
| build features | `const DECLARED` in `crates/fathom-cli/src/features.rs` | `docs/reference/features.md` |
| detectors | `catalogue.json`, blessed against the Rust `CATALOGUE` | a section in `docs/detectors/index.md`, with its worked example |
| TUI views | `recommender.tui.views.VIEWS` | the generated views table in `docs/reference/cheatsheet.md` |

**The catalogue is complete.** Every detector has a section *and* an example block, and every example the
page claims to show is a fixture that exists.

Exit: 0 clean, 1 with one line per problem.
"""
from __future__ import annotations

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "src"))

import gen_docs  # noqa: E402

DOCS = ROOT / "docs"
INDEX = DOCS / "index.md"
README = ROOT / "README.md"
HOME_DOCS = ["docs/quickstart.md", "docs/troubleshooting.md"]
GENERATED_PAGES = ["docs/reference/cli.md", "docs/reference/features.md",
                   "docs/reference/cheatsheet.md", "docs/detectors/index.md"]

LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*$")
CODE_FENCE = re.compile(r"^\s*(```|~~~)")
PLACEHOLDER = re.compile(r"\b(TODO:|FIXME:|TBD)\b")


def md_files() -> list[pathlib.Path]:
    return sorted(p for p in DOCS.rglob("*.md"))


def prose_lines(text: str):
    """Yield `(line_no, line)` for lines outside fenced code blocks."""
    inside = False
    for number, line in enumerate(text.splitlines(), start=1):
        if CODE_FENCE.match(line):
            inside = not inside
            continue
        if not inside:
            yield number, line


def headings(text: str) -> set[str]:
    out: set[str] = set()
    for _number, line in prose_lines(text):
        match = HEADING.match(line)
        if match:
            out.add(gen_docs.anchor(match.group(2)))
    return out


def links(text: str) -> list[tuple[int, str]]:
    out: list[tuple[int, str]] = []
    for number, line in prose_lines(text):
        for target in LINK.findall(line):
            out.append((number, target))
    return out


def resolve(source: pathlib.Path, target: str) -> tuple[pathlib.Path | None, str]:
    """`(file, fragment)` for a markdown link target, or `(None, fragment)` for a same-page link."""
    file_part, _, fragment = target.partition("#")
    if not file_part or file_part.startswith(("http://", "https://", "mailto:")):
        return (None if file_part else source), fragment
    path = (source.parent / file_part).resolve()
    if path.is_dir():
        # A link at a directory means its README, which is how `docs/adr/` reaches the index.
        readme = path / "README.md"
        return (readme if readme.exists() else None), fragment
    return path, fragment


def check_shape(problems: list[str], report: list[str]) -> None:
    if not INDEX.exists():
        problems.append("docs/index.md: the front door is missing")
        return
    report.append("front door: docs/index.md exists")

    for path in md_files():
        text = path.read_text(encoding="utf-8")
        titles = [line for _n, line in prose_lines(text) if HEADING.match(line)
                  and HEADING.match(line).group(1) == "#"]
        if len(titles) != 1 and DOCS / "adr" not in path.parents:
            # ADRs are allowed their own structure (`ADR-005` opens with `# Part one: events`); a
            # site page is not, because the site's navigation shows the title.
            problems.append("%s: %d `# ` headings; a site page has exactly one title"
                            % (path.relative_to(ROOT), len(titles)))
        for number, line in prose_lines(text):
            stripped = re.sub(r"`[^`]*`", "", line)  # `TODO` in a code span is a fact about the code
            match = PLACEHOLDER.search(stripped)
            if match:
                problems.append("%s:%d: %s in prose — finish it or name the plan item that owns it"
                                % (path.relative_to(ROOT), number, match.group(1).rstrip(":")))
    report.append("titles/placeholders: %d pages" % len(md_files()))


def check_reachability(problems: list[str], report: list[str]) -> None:
    pages = set(md_files())
    roots = {INDEX, README.resolve()}
    seen: set[pathlib.Path] = set()
    frontier = [root for root in roots if root.exists()]
    while frontier:
        source = frontier.pop()
        if source in seen:
            continue
        seen.add(source)
        for _number, target in links(source.read_text(encoding="utf-8")):
            path, _fragment = resolve(source, target)
            if path and path in pages and path not in seen:
                frontier.append(path)
    orphans = sorted(pages - seen)
    for path in orphans:
        problems.append("%s: not reachable from docs/index.md or README.md — link it or delete it"
                        % path.relative_to(ROOT))
    if not orphans:
        report.append("reachability: all %d pages reachable from the front door" % len(pages))


def check_anchors(problems: list[str], report: list[str]) -> None:
    cache: dict[pathlib.Path, set[str]] = {}
    total = 0
    for source in [README.resolve()] + md_files():
        if not source.exists():
            continue
        text = source.read_text(encoding="utf-8")
        for number, target in links(text):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            file_part, _, fragment = target.partition("#")
            if file_part.startswith(("http://", "https://", "mailto:")):
                continue
            path, _ = resolve(source, target)
            if path is None or not path.exists() or not path.is_file():
                continue
            total += 1
            if path not in cache:
                cache[path] = headings(path.read_text(encoding="utf-8"))
            if fragment and fragment not in cache[path]:
                problems.append("%s:%d: anchor %r does not exist in %s"
                                % (source.relative_to(ROOT), number, "#" + fragment,
                                   path.relative_to(ROOT)))
    report.append("anchors: %d fragment links checked" % total)


def generated_block(text: str, name: str) -> str | None:
    for match in gen_docs.MARK.finditer(text):
        if match.group(1) == name:
            return match.group(2)
    return None


def check_verbs(problems: list[str], report: list[str], binary: pathlib.Path) -> None:
    page = DOCS / "reference" / "cli.md"
    if not page.exists():
        problems.append("docs/reference/cli.md is missing")
        return
    text = page.read_text(encoding="utf-8")
    for name in gen_docs.verb_variants():
        heading = "## `fathom %s`" % name
        if heading not in text:
            problems.append("verb `%s` has no section in docs/reference/cli.md — add it and "
                            "`python3 tools/gen_docs.py write --page cli`" % name)
    block = generated_block(text, "verbs")
    if block is None:
        problems.append("docs/reference/cli.md: the generated `verbs` block is missing")
        return
    for name in gen_docs.verb_variants():
        if "`%s`" % name not in block:
            problems.append("docs/reference/cli.md: the generated verb table does not list `%s`" % name)
    if binary.exists():
        shipped = set(gen_docs.implemented_verbs())
        rows = {match.group(1): match.group(0)
                for match in re.finditer(r"^\| \*\*\[`([a-z\-]+)`\]\(.*$", block, re.M)}
        missing = sorted(name for name in shipped
                         if "](" not in rows.get(name, ""))
        if missing:
            problems.append("docs/reference/cli.md: shipped verbs with no doc link in the table: %s"
                            % ", ".join(missing))
        report.append("verbs: %d published, %d shipped, all with a section and a link"
                      % (len(gen_docs.verb_variants()), len(shipped)))
    else:
        report.append("verbs: sections checked against source; the binary was not built so the "
                      "shipped/refused split was not asserted")


def check_subverbs(problems: list[str], report: list[str], binary: pathlib.Path) -> None:
    if not binary.exists():
        report.append("subverbs: skipped with them (no binary)")
        return
    page = DOCS / "reference" / "cli.md"
    text = page.read_text(encoding="utf-8")
    root_short = gen_docs.run_help(binary, [])
    count = 0
    for verb, _summary in gen_docs.subcommands(root_short):
        for sub, _sub_summary in gen_docs.subcommands(gen_docs.run_help(binary, [verb])):
            count += 1
            if "## `fathom %s %s`" % (verb, sub) not in text and \
               "### `fathom %s %s`" % (verb, sub) not in text:
                problems.append("subverb `fathom %s %s` has no section in docs/reference/cli.md"
                                % (verb, sub))
    report.append("subverbs: %d checked" % count)


def check_features(problems: list[str], report: list[str]) -> None:
    page = DOCS / "reference" / "features.md"
    if not page.exists():
        problems.append("docs/reference/features.md is missing")
        return
    text = page.read_text(encoding="utf-8")
    block = generated_block(text, "features") or ""
    for name in gen_docs.declared_features():
        if "`%s`" % name not in block:
            problems.append("build feature `%s` is not documented in docs/reference/features.md"
                            % name)
    consent = {row["feature"] for row in gen_docs.net_features()}
    for name in consent:
        if name not in (generated_block(text, "privacy") or ""):
            problems.append("`net-%s` is a consent-gated feature and is not in the privacy table" % name)
    report.append("features: %d declared, %d consent-gated, all documented"
                  % (len(gen_docs.declared_features()), len(consent)))


def check_detectors(problems: list[str], report: list[str]) -> None:
    page = DOCS / "detectors" / "index.md"
    if not page.exists():
        problems.append("docs/detectors/index.md is missing")
        return
    text = page.read_text(encoding="utf-8")
    found = headings(text)
    for detector in gen_docs.catalogue():
        heading = gen_docs.anchor("`%s` — %s" % (detector["id"], detector["title"]))
        if heading not in found:
            problems.append("detector `%s` has no section in docs/detectors/index.md "
                            "(the heading must be the catalogue's id and title)"
                            % detector["id"])
        if generated_block(text, "example.%s" % detector["id"]) is None:
            problems.append("detector `%s` has no worked example block in docs/detectors/index.md "
                            "- run `python3 tools/gen_docs.py write --page detectors`" % detector["id"])
    report.append("detectors: %d, each with a section and a fixture-derived example"
                  % len(gen_docs.catalogue()))


def check_views(problems: list[str], report: list[str]) -> None:
    from recommender.tui.views import VIEWS
    page = DOCS / "reference" / "cheatsheet.md"
    if not page.exists():
        problems.append("docs/reference/cheatsheet.md is missing")
        return
    text = page.read_text(encoding="utf-8")
    block = generated_block(text, "views") or ""
    for view in VIEWS:
        if "**%s**" % view.title not in block:
            problems.append("view **%s** is not in the generated views table on docs/reference/cheatsheet.md"
                            % view.title)
    report.append("views: %d, each in the cheatsheet" % len(VIEWS))


def check_generated_fresh(problems: list[str], report: list[str], binary: pathlib.Path) -> None:
    pages = [p for p in ("cheatsheet", "detectors", "features")
             + (("cli",) if binary.exists() else ())]
    code = gen_docs.main(["check", "--bin", str(binary)] + sum((["--page", p] for p in pages), []))
    if code != 0:
        problems.append("generated pages are stale or their inputs are missing (see above)")
    else:
        report.append("generated pages: %s are current" % ", ".join(pages))


def check_nav(problems: list[str], report: list[str]) -> None:
    if not INDEX.exists():
        return
    text = INDEX.read_text(encoding="utf-8")
    for required in HOME_DOCS + GENERATED_PAGES:
        if "(%s)" % required[len("docs/"):] not in text:
            problems.append("docs/index.md does not link %s — a page the front door does not mention "
                            "is a page nobody finds" % required)
    report.append("nav: the front door links the %d pages it must" % (len(HOME_DOCS) + len(GENERATED_PAGES)))


def check_commands(problems: list[str], report: list[str], binary) -> None:
    """Every command a page tells the reader to type must parse (see `tools/check_docs_commands.py`)."""
    if not binary.exists():
        report.append("commands: skipped (no built binary; `ci.yml` runs this half)")
        return
    import check_docs_commands
    found = check_docs_commands.problems(binary)
    problems.extend(found)
    report.append("commands: %d documented invocation(s) parsed by clap"
                  % check_docs_commands.count())


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="check_docs_site.py", description=__doc__.splitlines()[0])
    parser.add_argument("--bin", default=str(ROOT / "target/debug/fathom"),
                        help="the fathom binary to read clap's verb tree from (optional)")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args(argv)
    binary = pathlib.Path(args.bin)

    problems: list[str] = []
    report: list[str] = []
    check_shape(problems, report)
    check_reachability(problems, report)
    check_anchors(problems, report)
    check_nav(problems, report)
    check_generated_fresh(problems, report, binary)
    check_verbs(problems, report, binary)
    check_subverbs(problems, report, binary)
    check_features(problems, report)
    check_detectors(problems, report)
    check_views(problems, report)
    check_commands(problems, report, binary)

    for problem in problems:
        print(problem)
    if problems:
        print("\n%d problem(s)" % len(problems))
        return 1
    if args.verbose:
        for line in report:
            print("ok — %s" % line)
    print("docs site OK (%d pages)" % len(md_files()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
