#!/usr/bin/env python3
"""Check the documentation site: reachability, anchors, and that no shipped thing is undocumented.

`tools/check_docs.py` guards the glossary, plan references and file-level links. This file guards the
*site* — the structure this documentation is supposed to have, and the acceptance criterion that a
generated page cannot quietly stop covering something:

    python3 tools/check_docs_site.py            # what CI runs
    python3 tools/check_docs_site.py --verbose  # print every check that passed
    python3 tools/check_docs_site.py --no-html  # skip the Pages build (the render rules, about a second)

Seven groups of rules, named as they are named here so a failure message points at the rule:

**Site shape.** `docs/index.md` is the front door; every page under `docs/` is reachable from it (or from
`README.md`) by following links, has exactly one `# ` heading, and contains no `TODO`/`TBD` left standing
in as prose.

**Anchors.** A fragment link — `[x](page.md#section)` or `[x](#section)` — resolves to a heading in the
target, computed the way a markdown renderer computes it. A page that is *mentioned* by a link with a
fragment and does not have it is a silent lie, and file-level link checking cannot see it.

**Relocations.** A retired path leaves a pointer page behind, and `RELOCATIONS` is the list of them:
every retired path exists, is a pointer (one `# ` title, under `POINTER_MAX_LINES` non-empty lines, no
table rows, no generated block, a link to the page that took its claims), is linked from `docs/index.md`,
and is linked by **nothing else** in the tree. That last clause is what forces a merge to rewrite its
inbound links instead of letting the whole repository lean on a redirect. `--self-test` breaks a copy of
the documentation four ways and fails unless each break is named.

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

**Rendered site.** `docs/` is also published as HTML on GitHub Pages, built by
`tools/build_docs_site.py`, and this gate builds it too: a page that is well-formed markdown and still
renders wrong — a table row whose code span contains an unescaped `|`, a link at a directory with nothing
in it, a header row pasted into the middle of a sentence — is a broken page either way, and the reader
sees the render and not the source.

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
import build_docs_site  # noqa: E402  (`check_html_build` renders the whole site in memory)

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
            if path.suffix.lower() not in (".md", ".markdown"):
                # A link at a PNG, a JSON report or a `.cast` file has no headings to land on, and
                # reading it as text is how this check crashed on the README's screenshot link.
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


#: A retired path -> the page that owns its claims now: one row per row of
#: `docs/reference/doc-map.md` §6.2, and one place. A relocation list duplicated across a workflow, a
#: test and a page is a list that drifts, which is what rule `C01` refuses.
RELOCATIONS = {
    "docs/concepts/risk-score.md": "docs/concepts/risk.md",
    "docs/concepts/risk-scoring.md": "docs/concepts/risk.md",
    "docs/concepts/risk-ranking.md": "docs/concepts/risk.md",
    "docs/concepts/delta-model.md": "docs/concepts/delta.md",
    "docs/concepts/episode-segmentation.md": "docs/concepts/episode-derivation.md",
    "docs/concepts/episode-relink.md": "docs/concepts/episode-derivation.md",
    "docs/concepts/rename-detection.md": "docs/concepts/rename.md",
    "docs/concepts/rename-pairing.md": "docs/concepts/rename.md",
    # M6 (`reco-48qd`): three pages about one crate, split by question, merged into one in the order the
    # questions are asked. The map's §6.2 predicted 5 + 7 + 7 inbound links, and the merge found 19.
    "docs/concepts/dependency-surface.md": "docs/concepts/dependencies.md",
    "docs/concepts/dependency-delta.md": "docs/concepts/dependencies.md",
    "docs/concepts/dependency-risk.md": "docs/concepts/dependencies.md",
}

#: How long a pointer page may be, counted in non-empty lines. `doc-map.md` §6.1 wrote "five to fifteen"
#: and §9 named the number as one M0 gets to decide, because `risk-ranking.md` had fourteen sections and
#: fourteen destinations do not fit in fifteen lines without dropping a row — and a section map with a
#: dropped row is the lie a section map exists to prevent. So: the longest map on the board, with room
#: for one more row.
POINTER_MAX_LINES = 24


def link_targets(source: pathlib.Path, text: str, root: pathlib.Path) -> list[str]:
    """The paths a page links to, relative to `root`: fragments ignored, remote links dropped."""
    out: list[str] = []
    for _number, target in links(text):
        path, _fragment = resolve(source, target)
        if path is None:
            continue
        try:
            out.append(str(path.resolve().relative_to(root.resolve())))
        except ValueError:
            continue
    return out


def relocation_problems(root: pathlib.Path, prefix: str = "") -> list[str]:
    """Every retired path resolves — and resolves *only* as a pointer.

    A function of a tree rather than of this module's own `docs/`, so `--self-test` can run it against a
    broken copy. A rule nobody has seen fail is a rule nobody can trust.
    """
    problems: list[str] = []
    docs = root / "docs"
    index = docs / "index.md"

    def rel(path: pathlib.Path) -> str:
        try:
            return str(path.resolve().relative_to(root.resolve()))
        except ValueError:
            return ""

    pages = sorted(docs.rglob("*.md"))
    index_targets = (link_targets(index, index.read_text(encoding="utf-8"), root)
                     if index.exists() else [])

    # Who links a pointer page. `docs/index.md` is the only page allowed to: a redirect serves a reader
    # with a bookmark, never this repository, so every real link in the tree has to go to the page that
    # now owns the claim. This clause is what makes a merge rewrite its inbound links.
    linkers: dict[str, list[str]] = {old: [] for old in RELOCATIONS}
    for page in pages:
        if page == index:
            continue
        for target in link_targets(page, page.read_text(encoding="utf-8"), root):
            if target in linkers:
                linkers[target].append(rel(page))

    for old, new in sorted(RELOCATIONS.items()):
        say = prefix + old
        path = root / old
        if not path.exists():
            problems.append("%s: retired path does not exist — a pointer page is what keeps a bookmark, a "
                            "CHANGELOG line or a four-month-old bead from landing on nothing" % say)
            continue
        if not (root / new).exists():
            problems.append("%s: points at %s, which does not exist" % (say, new))
        text = path.read_text(encoding="utf-8")
        titles = [line for _n, line in prose_lines(text)
                  if HEADING.match(line) and HEADING.match(line).group(1) == "#"]
        if len(titles) != 1:
            problems.append("%s: %d `# ` headings; a pointer has exactly one, and it is the title the "
                            "retired page had" % (say, len(titles)))
        body = [line for line in text.splitlines() if line.strip()]
        if len(body) > POINTER_MAX_LINES:
            problems.append("%s: %d non-empty lines, over the pointer cap of %d — a pointer maps the old "
                            "sections and says where they went; the claims live on one page" %
                            (say, len(body), POINTER_MAX_LINES))
        for number, line in prose_lines(text):
            if line.lstrip().startswith("|"):
                problems.append("%s:%d: a table row in a pointer — the table belongs to the page that "
                                "owns the claim, or two pages will disagree in private" % (say, number))
            if "BEGIN GENERATED" in line:
                problems.append("%s:%d: a generated block in a pointer — regenerate it on the page that "
                                "owns it" % (say, number))
        if rel(path) not in index_targets:
            problems.append("%s: not linked from docs/index.md — the retired list is what keeps a pointer "
                            "reachable, and reachability is what keeps it read" % say)
        for linker in sorted(set(linkers[old])):
            problems.append("%s: still linked from %s — a redirect is for a reader with a bookmark, not "
                            "for this repository; rewrite the link to %s" % (say, linker, new))
    return problems


def check_relocations(problems: list[str], report: list[str]) -> None:
    if not RELOCATIONS:
        report.append("relocations: nothing retired, nothing to check")
        return
    found = relocation_problems(ROOT)
    problems.extend(found)
    if not found:
        report.append("relocations: %d retired path(s) exist, name their target, stay under %d lines, and "
                      "are linked only from docs/index.md" % (len(RELOCATIONS), POINTER_MAX_LINES))


def self_test() -> int:
    """Break a copy of the documentation four ways and require each break to be named.

    `check_docs_site.py` had no self-test, and `docs/reference/doc-map.md` §4.0 said the first rule that
    needed one should bring it: a gate that has never said no cannot say yes. The fixture is a copy of
    the real tree, because the rule reads paths, links and line counts — a synthetic tree would prove
    less. Each mutation gets its own scratch copy so a fix cannot leak into the next break.
    """
    import shutil
    import tempfile

    def scratch() -> pathlib.Path:
        work = pathlib.Path(tempfile.mkdtemp(prefix="relocations"))
        shutil.copytree(ROOT / "docs", work / "docs")
        return work

    retired = sorted(RELOCATIONS)[0]
    name = pathlib.Path(retired).name

    work = scratch()
    checks: list[tuple[str, list[str]]] = [("the clean tree passes",
                                            relocation_problems(work, prefix="fixture:"))]

    work = scratch()
    (work / retired).unlink()
    checks.append(("a retired path that has gone entirely is named",
                   relocation_problems(work, prefix="fixture:")))

    work = scratch()
    pointer = work / retired
    pointer.write_text(pointer.read_text(encoding="utf-8") + "\n"
                       + "\n".join("row %d of a table restating the retired page" % i for i in range(30)))
    checks.append(("a pointer that has grown into a second copy is named",
                   relocation_problems(work, prefix="fixture:")))

    work = scratch()
    front = work / "docs" / "index.md"
    front.write_text(front.read_text(encoding="utf-8").replace("(concepts/%s)" % name,
                                                               "(concepts/nowhere.md)"))
    checks.append(("a pointer the front door does not link is named",
                   relocation_problems(work, prefix="fixture:")))

    work = scratch()
    quick = work / "docs" / "quickstart.md"
    quick.write_text(quick.read_text(encoding="utf-8") + "\n\nSee [the old page](concepts/%s).\n" % name)
    checks.append(("a real page still leaning on the redirect instead of the new page is named",
                   relocation_problems(work, prefix="fixture:")))

    failures = []
    for what, found in checks:
        ok = (not found) if what.startswith("the clean tree") else bool(found)
        if not ok:
            failures.append(what)
        print("%s check_relocations: %s%s" % (
            "ok —" if ok else "FAIL", what,
            "" if ok else (" — " + ("; ".join(found[:2]) if found else "the rule stayed silent"))))
    if failures:
        print("\nself-test failed: " + "; ".join(failures))
        return 1
    print("check_relocations self-test OK (%d ways the rule bites, with the clean tree passing between "
          "them)" % (len(checks) - 1))
    return 0


def check_html_build(problems: list[str], report: list[str]) -> None:
    """Build the Pages site in memory and assert its invariants (see `tools/build_docs_site.py`).

    The builder is the only thing in this repository that renders the manual, so it is also the only place
    that can notice a page whose markdown is legal and whose render is wrong. The same rules run in
    `.github/workflows/docs.yml`, on the artefact that gets published; they are here too so that
    `python3 tools/check_docs_site.py` — the one command the README names for documentation — cannot pass
    on a manual that will not build.
    """
    blob, repo = build_docs_site.source_urls(ROOT)
    cfg = build_docs_site.Config(root=ROOT, docs=DOCS, source_url=blob or "https://example.invalid/blob/none",
                                 repo_url=repo)
    builder = build_docs_site.Builder(cfg)
    files = builder.generate()
    found = builder.check(files)
    problems.extend("html: %s" % problem for problem in found)
    pages = len(builder.pages)
    remote = ("remote links resolve to %s" % blob if blob else
              "remote links were made absolute but not pointed anywhere: no git remote and no "
              "GITHUB_REPOSITORY, so install with --source-url to assert that half")
    note = ("; %d hand-written .html under docs/ is not published (%s)" %
            (len(builder.handwritten), ", ".join(builder.rel(path) for path in builder.handwritten))
            if builder.handwritten else "")
    report.append("html: %d pages, %d files built; %s%s" % (pages, len(files), remote, note))


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
    parser.add_argument("--no-html", action="store_true",
                        help="skip the Pages build (the render rules; about a second of the run)")
    parser.add_argument("--self-test", action="store_true",
                        help="break a copy of the docs four ways and prove `check_relocations` names each")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    binary = pathlib.Path(args.bin)

    problems: list[str] = []
    report: list[str] = []
    check_shape(problems, report)
    check_reachability(problems, report)
    check_anchors(problems, report)
    check_nav(problems, report)
    check_relocations(problems, report)
    check_generated_fresh(problems, report, binary)
    check_verbs(problems, report, binary)
    check_subverbs(problems, report, binary)
    check_features(problems, report)
    check_detectors(problems, report)
    check_views(problems, report)
    check_commands(problems, report, binary)
    if not args.no_html:
        check_html_build(problems, report)

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
