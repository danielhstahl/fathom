"""Contract: `docs/` renders into a GitHub Pages site that is complete, escaped, and keeps its links.

The ticket's acceptance criteria, as assertions:

* **the doc markdown becomes HTML** — every `docs/**.md` becomes a page, every non-markdown file under
  `docs/` is copied, and the whole site is built in memory here before CI writes it anywhere;
* **the site is navigable** — every built page is in the sidebar, and a section directory with no
  `README.md` of its own gets a generated listing, so `[concepts](concepts/)` lands on a page instead of
  a 404;
* **the links survive the move** — a relative markdown link becomes a relative HTML link that resolves
  inside the built site, a fragment resolves to a heading id the builder assigned with
  `gen_docs.anchor` (the same rule `check_docs_site.py` checks in the markdown), and the links that cite
  evidence outside `docs/` — a schema, a source file, a benchmark report — point at the repository
  instead of at a directory above the site root;
* **nothing a page writes can inject markup** — the builder escapes HTML, and a hostile page is built to
  prove it;
* **the render does not lose the manual** — each page's characters are compared against the ones its
  markdown contained, which is the check that a dropped table cell or a swallowed `<key>` placeholder is a
  build failure rather than a reader's confusion;
* **the build is reproducible** — two builds of the same tree agree byte for byte, so a published
  artefact can be traced to the commit it was built from.

`.github/workflows/docs.yml` publishes the same artefact; `tools/check_docs_site.py` runs the same rules
as part of the documentation gate `contracts.yml` runs. These tests exist so `python3 -m pytest -q`
catches all of it before a push, and so the builder's failure modes are named —
`python3 tools/build_docs_site.py --self-test` — rather than assumed.
"""
from __future__ import annotations

import pathlib
import posixpath
import re
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "src"))

import build_docs_site as site  # noqa: E402
import gen_docs  # noqa: E402

HREF = re.compile(r"(?:href|src)=\"([^\"]*)\"")
FENCE = re.compile(r"^ {0,3}(```|~~~)")
ATX = re.compile(r"^ {0,3}#{1,6} +(.+?)\s*$")


@pytest.fixture(scope="module")
def built():
    """One build of the real `docs/`, shared by the tests that only look at the artefact."""
    blob, repo = site.source_urls(ROOT)
    cfg = site.Config(root=ROOT, docs=DOCS, source_url=blob, repo_url=repo,
                      base_url="https://example.test/fathom/")
    return site.build_all(cfg)


def out_of(source: pathlib.Path) -> str:
    """The page a markdown file becomes: `docs/concepts/delta.md` → `concepts/delta.html`."""
    parts = list(source.relative_to(DOCS).parts)
    if parts[-1] == "index.md" or (parts[-1] == "README.md"
                                   and not (source.parent / "index.md").exists()):
        parts[-1] = "index.html"
    else:
        parts[-1] = source.stem + ".html"
    return "/".join(parts)


def target_of(name: str, ref: str) -> str:
    """Where an `href` on `name` points, as a path inside the built site."""
    return posixpath.normpath(posixpath.join(posixpath.dirname(name), ref.partition("#")[0]))


def html_files(files: dict) -> list[tuple[str, str]]:
    return [(name, value) for name, value in sorted(files.items())
            if name.endswith(".html") and isinstance(value, str)]


def markdown_pages() -> list[pathlib.Path]:
    return sorted(path for path in DOCS.rglob("*.md") if path.is_file())


def heading_texts(source: pathlib.Path) -> list[str]:
    """The text of every ATX heading in a page, outside fenced code."""
    out: list[str] = []
    inside = False
    for line in source.read_text(encoding="utf-8").splitlines():
        if FENCE.match(line):
            inside = not inside
            continue
        if inside:
            continue
        match = ATX.match(line)
        if match:
            out.append(match.group(1))
    return out


def test_the_builder_can_fail():
    """A gate nobody has watched fail is a rumour, so the builder ships its own failure cases."""
    assert site.self_test() == 0


def test_the_real_docs_build_without_problems(built):
    files, problems, _builder = built
    assert files, "the build produced nothing"
    assert problems == []


def test_every_markdown_page_and_asset_is_in_the_site(built):
    files, _problems, _builder = built
    for source in markdown_pages():
        assert out_of(source) in files, "%s has no page" % source.relative_to(ROOT)
    for source in sorted(path for path in DOCS.rglob("*")
                         if path.is_file() and path.suffix.lower() != ".md"):
        assert source.relative_to(DOCS).as_posix() in files, \
            "%s was not copied" % source.relative_to(ROOT)


def test_the_build_is_deterministic(built):
    _files, _problems, _builder = built
    blob, repo = site.source_urls(ROOT)
    one = site.Config(root=ROOT, docs=DOCS, source_url=blob, repo_url=repo,
                      base_url="https://example.test/fathom/")
    assert site.Builder(one).generate() == site.Builder(one).generate()


def test_no_link_or_asset_is_an_absolute_path(built):
    """Pages serves a project site under `/<repo>/`, so an absolute path is a 404 with a green build."""
    files, _problems, _builder = built
    offenders = ["%s: %r" % (name, ref) for name, _value in html_files(files)
                 for ref in HREF.findall(_value) if ref.startswith("/")]
    assert offenders == []


def test_every_relative_link_lands_on_a_built_page(built):
    files, _problems, _builder = built
    broken = [f"{name} -> {ref!r}" for name, value in html_files(files)
              for ref in HREF.findall(value)
              if ref and not ref.startswith("#") and not site.SCHEME.match(ref)
              and not ref.startswith("//") and target_of(name, ref) not in files]
    assert broken == []


def test_a_fragment_link_lands_on_a_heading_the_builder_wrote(built):
    files, _problems, _builder = built
    ids = {name: set(site.ANCHOR_ID.findall(value)) for name, value in html_files(files)}
    broken = []
    for name, value in html_files(files):
        for ref in HREF.findall(value):
            file_part, _, fragment = ref.partition("#")
            if not fragment or site.SCHEME.match(ref) or ref.startswith("//"):
                continue
            target = name if not file_part else target_of(name, ref)
            if target in ids and fragment not in ids[target]:
                broken.append(f"{name} -> {ref!r}")
    assert broken == []


def test_the_heading_ids_are_the_ones_the_markdown_gate_already_checks(built):
    """`check_docs_site.py` proves `page.md#section` resolves; the HTML has to use those same ids."""
    files, _problems, _builder = built
    for source in markdown_pages():
        page = files[out_of(source)]
        want = {gen_docs.anchor(text) for text in heading_texts(source)}
        have = set(site.ANCHOR_ID.findall(page))
        missing = sorted(want - have)
        assert missing == [], "%s has no id for %s" % (source.relative_to(ROOT), missing[:3])


def test_links_that_leave_docs_point_at_the_repository(built):
    """The manual cites schemas, sources and reports outside `docs/`; on Pages they cannot stay relative."""
    files, _problems, _builder = built
    blob, _repo = site.source_urls(ROOT)
    if not blob:
        print("note: no git remote and no GITHUB_REPOSITORY; the remote base was not asserted")
        return
    page = files["concepts/delta.html"]
    assert blob in page, "the page that cites a schema no longer links it on the source host"
    assert "../../schemas/" not in page


def test_sections_without_a_readme_get_a_generated_listing(built):
    files, _problems, _builder = built
    for directory in sorted({page.parent for page in markdown_pages()}):
        if (directory / "index.md").exists() or (directory / "README.md").exists():
            continue
        listing = directory.relative_to(DOCS).as_posix() + "/index.html"
        assert listing in files, "%s has no front page" % directory.relative_to(ROOT)
        assert "<h1>" in files[listing]


def test_every_page_appears_in_the_navigation(built):
    _files, _problems, builder = built
    for page in builder.pages:
        nav = builder.nav(builder.pages, page.out)
        assert 'href="%s"' % (page.prefix + page.out) in nav, "%s is not in the sidebar" % page.out


def test_the_search_index_lists_every_page(built):
    files, _problems, builder = built
    script = files["assets/search.js"]
    for page in builder.pages:
        assert '"url": "%s"' % page.out in script, "%s is not searchable" % page.out


def test_a_site_with_a_known_host_has_an_absolute_sitemap(built):
    files, _problems, builder = built
    assert files["sitemap.xml"].count("<loc>https://example.test/fathom/") == len(builder.pages)
    assert "Sitemap: https://example.test/fathom/sitemap.xml" in files["robots.txt"]


def test_a_local_build_writes_no_sitemap_and_no_robots(built):
    """Relative `<loc>` entries are not addresses, and a crawler is a sitemap's only reader."""
    blob, repo = site.source_urls(ROOT)
    cfg = site.Config(root=ROOT, docs=DOCS, source_url=blob, repo_url=repo)
    files, problems, _builder = site.build_all(cfg)
    assert problems == []
    assert "sitemap.xml" not in files and "robots.txt" not in files


def test_the_shell_is_balanced_everywhere(built):
    files, _problems, _builder = built
    for name, value in html_files(files):
        assert site.unbalanced_tags(value) == [], name


def test_the_render_says_what_the_markdown_said(built):
    """No page lost words on the way through the renderer."""
    _files, _problems, builder = built
    assert builder.audit, "the builder audits nothing"
    for out, wanted in sorted(builder.audit.items()):
        assert site.audit_skeletons(wanted, site.html_text(builder.bodies[out])) == "", out


def test_a_hand_written_html_file_under_docs_is_not_published(tmp_path):
    """Two files cannot both be `index.html`, and the generated one is the one that is on GitHub too."""
    root = tmp_path / "repo"
    (root / "docs").mkdir(parents=True)
    (root / "docs" / "index.md").write_text("# Manual\n\nMarkdown, as always.\n", encoding="utf-8")
    (root / "docs" / "index.html").write_text("<html><body>hand-built</body></html>", encoding="utf-8")
    cfg = site.Config(root=root, docs=root / "docs", source_url="https://example.test/r/blob/x")
    files, problems, builder = site.build_all(cfg)
    assert problems == []
    assert "hand-built" not in files["index.html"]
    assert 'id="manual"' in files["index.html"]
    assert [path.name for path in builder.handwritten] == ["index.html"]


def test_a_hostile_page_cannot_inject_markup(tmp_path):
    """The escape rule and the subset rule, on a page written to break both."""
    root = tmp_path / "repo"
    (root / "docs").mkdir(parents=True)
    (root / "docs" / "index.md").write_text(
        "# Index\n\n<script>alert(1)</script>\n\n<iframe src=\"//evil\"></iframe>\n\n"
        "and a <b>bold</b> claim\n", encoding="utf-8")
    cfg = site.Config(root=root, docs=root / "docs", source_url="https://example.test/r/blob/x")
    files, problems, _builder = site.build_all(cfg)
    page = files["index.html"]
    assert "&lt;script&gt;alert(1)&lt;/script&gt;" in page
    assert "<script>alert" not in page
    assert "<iframe" not in page
    assert "&lt;b&gt;bold&lt;/b&gt;" in page
    assert "<b>" not in page
    # and the builder says so instead of shipping the escape quietly
    assert any("raw HTML" in problem for problem in problems), problems
