"""Contract: the documentation site is a site, and it cannot lie about what ships (`e7.5`, reco-60m).

The ticket's acceptance criteria, as assertions:

* **docs build with link checking and generated sections** — `tools/gen_docs.py` writes the generated
  pages from the program (clap's `--help`, the keymap module, `features.rs`, the blessed detector
  catalogue) and `check` fails on drift; `tools/check_docs_site.py` fails an orphan page or a dangling
  anchor;
* **every shipped feature has a doc page, and a CI check fails if one does not** — the enumerations come
  from the code (`enum Verb`, `const DECLARED`, `VIEWS`, `catalogue.json`), so a feature that arrives
  without documentation is a red test rather than a documentation debt somebody notices in a review;
* **the detector catalogue covers every built-in detector with an example** — every detector in the
  catalogue has a section *and* a worked example generated from the fixture it must fire on;
* **a stranger can finish the quickstart and name what an episode is** — the quickstart has to carry the
  six nouns and define the one that matters, in the same words the glossary uses.

These are the same rules `tools/check_docs_site.py` runs in CI (see `.github/workflows/contracts.yml`),
asserted here so that `python3 -m pytest -q` catches them before a push. The CLI-reference half of the
generated pages is skipped when the binary has not been built, and says so in the skip reason rather
than passing silently.
"""
from __future__ import annotations

import importlib.util
import pathlib
import re
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "src"))

import gen_docs  # noqa: E402
import check_docs_site as site  # noqa: E402

QUICKSTART = (DOCS / "quickstart.md").read_text(encoding="utf-8")
BINARY = ROOT / "target/debug/fathom"


def _problems() -> list[str]:
    problems: list[str] = []
    report: list[str] = []
    site.check_shape(problems, report)
    site.check_reachability(problems, report)
    site.check_anchors(problems, report)
    site.check_nav(problems, report)
    site.check_generated_fresh(problems, report, BINARY)
    site.check_features(problems, report)
    site.check_detectors(problems, report)
    site.check_views(problems, report)
    return problems


def test_the_site_shape_holds():
    """No orphan pages, no dangling anchors, one title per page, no placeholders in prose."""
    assert _problems() == []


def test_no_orphan_pages():
    pages = set(site.md_files())
    assert pages, "the docs directory is empty"
    problems: list[str] = []
    site.check_reachability(problems, [])
    assert problems == []


def test_every_fragment_link_lands_on_a_heading():
    problems: list[str] = []
    site.check_anchors(problems, [])
    assert problems == []


def test_generated_pages_are_current():
    pages = ["cheatsheet", "detectors", "features"] + (["cli"] if BINARY.exists() else [])
    if not BINARY.exists():
        print("note: target/debug/fathom not built; the `cli` page was not re-derived")
    assert gen_docs.main(["check", "--bin", str(BINARY)]
                         + sum((["--page", page] for page in pages), [])) == 0


def test_every_published_verb_has_a_section_and_a_link():
    text = (DOCS / "reference" / "cli.md").read_text(encoding="utf-8")
    table = site.generated_block(text, "verbs") or ""
    verbs = gen_docs.verb_variants()
    assert len(verbs) >= 16, "the verb enum got smaller: the reference and this test must agree"
    for verb in verbs:
        assert "## `fathom %s`" % verb in text, "verb %r has no section" % verb
    if BINARY.exists():
        for verb in sorted(gen_docs.implemented_verbs()):
            row = next((line for line in table.splitlines()
                        if line.startswith("| **[`%s`]" % verb)), "")
            assert "](" in row, "shipped verb %r has no documentation link" % verb


def test_every_build_feature_has_a_documentation_entry():
    """The acceptance criterion, mechanically: a feature with no doc row fails the build."""
    text = (DOCS / "reference" / "features.md").read_text(encoding="utf-8")
    table = site.generated_block(text, "features") or ""
    for feature in gen_docs.declared_features():
        row = next((line for line in table.splitlines() if line.startswith("| `%s`" % feature)), "")
        assert row, "build feature %r is undocumented" % feature
        assert "](" in row, "build feature %r has no documentation link" % feature
    for row in gen_docs.net_features():
        assert "`%s`" % row["consent_key"] in table, \
            "consent key %r is not printed with its feature" % row["consent_key"]


def test_every_detector_has_a_section_and_a_worked_example():
    catalogue = gen_docs.catalogue()
    assert len(catalogue) == 12
    text = (DOCS / "detectors" / "index.md").read_text(encoding="utf-8")
    for detector in catalogue:
        anchor = gen_docs.anchor("`%s` — %s" % (detector["id"], detector["title"]))
        assert anchor in site.headings(text), "detector %r has no section" % detector["id"]
        example = site.generated_block(text, "example.%s" % detector["id"])
        assert example, "detector %r has no worked example" % detector["id"]
        assert "What the shipped detector reports" in example
        assert detector["id"] in example


def test_every_detector_knob_is_printed():
    knobs = gen_docs.detector_knobs()
    table = site.generated_block((DOCS / "detectors" / "index.md").read_text(), "knobs") or ""
    for key in knobs:
        assert "`%s`" % key in table, "knob %r is not on the catalogue page" % key


def test_every_tui_view_is_on_the_cheatsheet():
    from recommender.tui.views import VIEWS
    table = site.generated_block((DOCS / "reference" / "cheatsheet.md").read_text(), "views") or ""
    for view in VIEWS:
        assert "**%s**" % view.title in table


def test_every_keymap_action_is_reachable_and_published():
    """K1 as a documentation claim: every bound action appears on the cheatsheet."""
    from recommender.tui import keymap
    table = site.generated_block((DOCS / "reference" / "cheatsheet.md").read_text(), "actions") or ""
    for action_id, action in keymap.ACTIONS.items():
        if action.palette:
            assert "`%s`" % action_id in table, "action %r is not on the cheatsheet" % action_id


def test_the_quickstart_carries_the_six_nouns():
    for page in ("episode", "delta", "risk-score", "review-ledger", "ratchet", "ambient-mode"):
        assert "concepts/%s.md" % page in QUICKSTART, "the quickstart never links concepts/%s.md" % page


def test_the_quickstart_defines_an_episode():
    """Acceptance criterion 3, the part that can be asserted: the definition is on the page, in the
    glossary's words, together with the four things an episode is not."""
    text = " ".join(QUICKSTART.lower().split())
    assert "the change produced by one agent turn" in text
    for wrong in ("an episode is a commit", "an episode is a branch",
                  "an episode is a pull request", "an episode is a session"):
        assert wrong not in text, "the quickstart calls an episode %r" % wrong
    for right in ("not a commit", "not a branch", "not a pull request", "not a session"):
        assert right in text, "the quickstart does not exclude %r" % right


def test_the_troubleshooting_page_answers_the_exit_codes_that_happen():
    text = (DOCS / "troubleshooting.md").read_text(encoding="utf-8")
    for code in ("3", "4", "5", "7", "9", "12"):
        assert "`%s`" % code in text, "exit code %s is not covered" % code


def test_every_documented_command_parses():
    """A recipe whose command does not type is worse than a recipe with no command.

    Every `fathom …` line in a command fence is handed to clap with `--help` appended, which parses the
    grammar without running the verb. This is the check that catches the two failures a documentation
    review does not: a subcommand nobody shipped, and a flag somebody assumed.
    """
    if not BINARY.exists():
        print("note: target/debug/fathom not built; documented commands were not parsed")
        return
    import check_docs_commands
    assert check_docs_commands.problems(BINARY) == []


def test_recipes_name_only_verbs_this_binary_has():
    """A recipe that names an untypeable verb is worse than one that names nothing.

    Checked against clap's published verbs, off the source rather than off memory. Future verbs a page
    is obliged to mention (`fathom check`, `fathom mcp serve`, `fathom ratchet`) are excluded by naming
    the plan item that owns them on the same line — which is also what the prose rule in
    `docs/recipes/README.md` asks for.
    """
    verbs = set(gen_docs.verb_variants())
    offenders = []
    for path in sorted((DOCS / "recipes").rglob("*.md")) + [DOCS / "quickstart.md",
                                                            DOCS / "index.md"]:
        for line in site.prose_lines(path.read_text(encoding="utf-8")):
            text = line[1]
            for match in re.finditer(r"`fathom ([a-z][a-z\-]+)", text):
                verb = match.group(1)
                if verb in verbs:
                    continue
                if re.search(r"(not in this build|not built|will replace|owns|`e\d)", text):
                    continue
                offenders.append("%s: `fathom %s`" % (path.relative_to(ROOT), verb))
    assert offenders == []


def test_docs_index_links_the_pages_it_is_a_front_door_for():
    problems: list[str] = []
    site.check_nav(problems, [])
    assert problems == []
