#!/usr/bin/env python3
"""Build the HTML documentation site that GitHub Pages serves, out of the markdown in `docs/`.

`docs/` is already a site — `docs/index.md` is a front door, every page is reachable from it, no anchor
dangles — and `tools/check_docs_site.py` keeps all of that true. What it is not is *rendered*: read on
GitHub and you get 86 markdown files with the repository chrome around them, no navigation between
sections, no search, and a `../schemas/episode-delta.schema.json` link that dumps a blob instead of a
page. This tool turns the same tree into static HTML: one page per `.md`, a sidebar, a per-page table of
contents, a search box, copied assets, and every link rewritten so that it lands somewhere inside the
built site or on the commit that built it.

    python3 tools/build_docs_site.py write [--out DIR]   # build (default `site/`)
    python3 tools/build_docs_site.py check               # build in memory and assert the invariants
    python3 tools/build_docs_site.py --self-test         # prove this gate can fail, in milliseconds
    python3 tools/build_docs_site.py check --verbose     # print every rule that passed

`.github/workflows/docs.yml` runs `check` on every pull request and `write` + a Pages deploy on every
merge to `master`. Nothing here needs the Rust build: the generated pages (`cli`, `cheatsheet`,
`features`, `detectors`) are already committed, and the generator that owns them is checked by
`contracts.yml` — so a docs-only pull request is red in seconds, not after a four-minute dependency
build.

**Why a builder in this repository rather than a hosted markdown renderer.** Three reasons, and each is a
gate below: the links are the manual's argument (a page cites the schema, the fixture and the source file
that prove its claim), so rewriting them needs to know this repository's layout, which a general-purpose
generator does not; the output has to be deterministic, because a build that changes when nothing changed
is a build nobody diffs; and the whole point of `docs/` is that it is *checked* — a renderer that silently
drops a table cell or swallows a `<C-c>` placeholder is a renderer that lies, so `check` compares the
words it emitted against the words it was given.

**The markdown subset.** `docs/` uses a deliberate, boring subset, and `check` refuses anything outside it
rather than guessing: ATX headings, paragraphs, hard breaks, fenced code (``` and ~~~), thematic breaks,
blockquotes, ordered and unordered lists with nested items and lazy continuation, GFM pipe tables with
alignment, inline code spans, links, images, autolinks, `**strong**`, `*em*`, `~~strike~~`, backslash
escapes and HTML comments. There are no reference-style links, no footnotes, no shortcodes and — the part
that matters for safety — **no raw HTML**: every `<` in the prose is escaped, so nothing a page writes can
inject markup, script or a tracker. HTML comments pass through inert, which is what keeps the
`<!-- BEGIN GENERATED: … -->` markers of `tools/gen_docs.py` from printing as text. If a page needs a
construct outside the subset, the failure message says which one it is; the fix is to write it in the
subset, not to grow the renderer for one page.

**Links, and where they go.** A relative link is resolved against the page it is written on and then
classified:

| written | built as | why |
|---|---|---|
| `delta.md`, `delta.md#section` | `delta.html#section` | the page is in the site; the fragment is the heading id the builder gave it |
| `../concepts/`, `adr/` | `../concepts/index.html` | a directory means its `index.md`/`README.md`, which is how `check_docs_site.py` reads it too |
| `five-minute.cast`, `../docs/screenshots/fathom.png` | the same file, copied | assets are part of the site, and `.cast` files are exhibits the demo page links |
| `../../schemas/episode-delta.schema.json`, `../CHANGELOG.md`, `crates/…/privacy.rs` | `https://github.com/<repo>/blob/<ref>/<path>` | 196 of the manual's links are evidence *outside* `docs/`; on a Pages site "up two directories" is a 404, so they point at the commit the site was built from |
| `https://…`, `mailto:…` | unchanged | |
| `#section` | unchanged | |

The remote base is derived, not hardcoded (see `source_base`), and `--source-url` overrides it.

**Determinism.** Two runs of `build()` return the same mapping, byte for byte; `check` builds twice and
compares. Nothing in the output carries a timestamp, a hostname, a filesystem path or a dict iteration
order, and every listing is sorted. That is what makes it honest to publish a build from a runner.

**What `check` asserts**, one line per failure, exit 1:

1. *shape* — every `docs/**/*.md` produced a page; every page has a title; nothing outside `docs/` was
   written;
2. *links* — every `href`/`src` in the built pages is an absolute URL or resolves to a file in the built
   site, and every fragment resolves to an `id` that page actually has;
3. *escape* — raw HTML does not appear anywhere in the built pages except the HTML comments `docs/`
   already carries and this tool's own markup (asserted by feeding the builder a hostile page);
4. *no lost words* — the alphanumeric skeleton of each source page equals the skeleton of the rendered
   page, so a renderer bug that drops a table cell, a list item or a `<placeholder>` is caught here and
   not in a reader's quote of a missing sentence;
5. *subset* — no raw HTML block tag in the prose (the documented subset above), no reference-style link
   definitions, no unclosed code fence;
6. *determinism* — two builds agree;
7. *pages* — the built site has `index.html`, `404.html`, `assets/docs.css`, `assets/docs.js`, one
   sidebar entry per page, and no absolute path (so it works from a project-page subdirectory *and* from
   `file://`).

Exit: 0 clean, 1 with one line per problem.
"""
from __future__ import annotations

import argparse
import html as html_escape_mod
import json
import os
import pathlib
import posixpath
import re
import subprocess
import sys
from dataclasses import dataclass
from html.parser import HTMLParser

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import gen_docs  # noqa: E402  (`anchor`: the fragment rule the whole site agrees on)

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
DEFAULT_OUT = "site"
SITE_TITLE = "Fathom documentation"

#: Tags that would *do* something if they were passed through: the escape rule is the security
#: property, this lint is the "you meant that to render" property.
STRUCTURAL_HTML = re.compile(r"<(/?)(?:script|style|iframe|img|a|div|span|p|table|tr|td|th|ul|ol|li|"
                             r"h[1-6]|br|hr|details|summary|form|input|button|svg|video|audio)\b[^>]*>",
                             re.I)
REFERENCE_LINK = re.compile(r"^ {0,3}\[[^\]]+\]:\s*\S+", re.M)

# ---------------------------------------------------------------------------
# inline rendering
# ---------------------------------------------------------------------------
#: A code span: a run of backticks, content, the same run. Non-greedy, so `` `a` and `b` `` stays two
#: spans, and `re.S` because three pages in this repository wrap a span across a line break.
CODE_SPAN = re.compile(r"(`+)(.+?)\1", re.S)
COMMENT = re.compile(r"<!--.*?-->", re.S)
AUTOLINK = re.compile(r"<((?:https?|mailto):[^\s<>]+)>")
LINK = re.compile(r"!?\[(?P<alt>(?:[^\[\]]|\[[^\[\]]*\])*)\]\(\s*(?P<target>[^()\s]*)\s*"
                  r"(?:\"(?P<title>[^\"]*)\"\s*)?\)")
STRONG = re.compile(r"\*\*(?=\S)(?P<b>.+?(?<=\S))\*\*|__(?=\S)(?P<b2>.+?(?<=\S))__", re.S)
EMPH = re.compile(r"(?<![A-Za-z0-9])\*(?=\S)(?P<i>.+?(?<=\S))\*(?![A-Za-z0-9])", re.S)
STRIKE = re.compile(r"~~(?=\S)(?P<s>.+?(?<=\S))~~", re.S)
ESCAPED = re.compile(r"\\([\\`*_{}\[\]()#+\-.!<>~|\"'])")
#: Two or more trailing spaces: a hard line break, the one CommonMark behaviour a hand-written
#: glossary relies on. `M` because a paragraph is handed to the inline renderer already joined.
HARD_BREAK = re.compile(r" {2,}$", re.M)
#: Standing in for a hard break while the rest of the line is still escaped, so the `<br>` cannot be
#: escaped with it. No markdown text can contain a NUL.
BREAK_MARK = "\x00"


def esc(text: str) -> str:
    """Escape text for an HTML text node."""
    return html_escape_mod.escape(text, quote=False)


def attr(text: str) -> str:
    """Escape text for a `"`-quoted attribute value."""
    return html_escape_mod.escape(text, quote=True)


def strip_inline(text: str) -> str:
    """Plain text of an inline run: what a `<title>`, a table of contents entry and the search index want.

    Code spans are masked out before the link and emphasis passes and restored afterwards, which is the
    same precedence the renderer uses (`TOKENS` takes the earliest match, so a span that starts before a
    link wins). Without the mask, a span that *contains* markdown — this page writes
    `[`[concepts](concepts/)`](concepts/)` as an example — gets its own text treated as markup and loses
    the target, and the audit then reports a mismatch the reader never sees.
    """
    masked: list[str] = []

    def hold(match: re.Match) -> str:
        masked.append(match.group(2))
        return "\x00%d\x00" % (len(masked) - 1)

    text = CODE_SPAN.sub(hold, text)
    text = COMMENT.sub("", text)
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"^ {0,3}>\s?", "", text, flags=re.M)
    text = ESCAPED.sub(r"\1", text)
    text = re.sub(r"\*\*(.+?)\*\*", r"\1", text, flags=re.S)
    text = re.sub(r"__(.+?)__", r"\1", text, flags=re.S)
    text = re.sub(r"~~(.+?)~~", r"\1", text, flags=re.S)
    text = re.sub(r"(?<![A-Za-z0-9])\*(.+?)\*(?![A-Za-z0-9])", r"\1", text, flags=re.S)
    text = re.sub(r"\x00(\d+)\x00", lambda match: masked[int(match.group(1))], text)
    return text.strip()


class Renderer:
    """Renders one page.

    `link` is the only thing the renderer needs from the site: it turns a markdown link target into the
    `href` the built page should carry, and reports what it could not resolve (see `Builder.resolve`).
    Heading `id`s come out of `gen_docs.anchor`, the same function `check_docs_site.py` uses to prove
    every fragment link in the repository lands on a heading — so a `[x](page.md#section)` that is green
    in the markdown gate is green in the HTML too.
    """

    def __init__(self, link):
        self.link = link
        self.ids: dict[str, int] = {}
        self.headings: list[tuple[int, str, str]] = []   # (level, id, plain text)
        self.problems: list[str] = []
        self.intra_page: set[str] = set()               # fragments this page links to
        #: In audit mode the renderer emits nothing and collects the words it was given instead, in
        #: order. Comparing that collection with the words of the finished page is how `check` knows the
        #: renderer lost nothing (`audit_skeletons`).
        self.audit = False
        self.sink: list[str] = []

    # -- inline ---------------------------------------------------------------
    #: Leftmost token wins, and the order of this tuple is the tie-break. Code spans come before links
    #: (`` `[a](b)` `` is a code span, CommonMark agrees), links before anything that could light up
    #: inside their text, and `**strong**` before `*em*` because they start at the same character.
    TOKENS: tuple = (
        ("code", CODE_SPAN), ("link", LINK), ("autolink", AUTOLINK), ("comment", COMMENT),
        ("escape", ESCAPED), ("strong", STRONG), ("strike", STRIKE), ("em", EMPH),
    )

    def inline(self, text: str) -> str:
        """Inline markdown to HTML.

        One left-to-right scan for the next token of any kind, rather than one pass per construct. The
        difference is nesting: `[`adapters/fathom-hook.py`](../../adapters/fathom-hook.py)` is a link
        whose text is a code span, and a renderer that pulls code spans out first leaves
        `](../../adapters/fathom-hook.py)` printed on the page. Link text is rendered recursively, so
        nesting works one level deep, which is as deep as this documentation goes.
        """
        if self.audit:
            self.sink.append(strip_inline(text))
            return ""
        out: list[str] = []
        pos = 0
        pending: dict[str, "re.Match | None"] = {}
        while True:
            best_name, best = "", None
            for name, pattern in self.TOKENS:
                if name not in pending:
                    pending[name] = pattern.search(text, pos)
                found = pending[name]
                while found is not None and found.start() < pos:
                    found = pending[name] = pattern.search(text, pos)
                if found is not None and (best is None or found.start() < best.start()):
                    best_name, best = name, found
            if best is None:
                break
            out.append(esc(HARD_BREAK.sub(BREAK_MARK, text[pos:best.start()])))
            out.append(self.token(best_name, best))
            pos = best.end()
        out.append(esc(HARD_BREAK.sub(BREAK_MARK, text[pos:])))
        return "".join(out).replace(BREAK_MARK, "<br>\n")

    def token(self, name: str, match: "re.Match") -> str:
        if name == "code":
            return "<code>%s</code>" % esc(match.group(2).strip())
        if name == "comment":
            return match.group(0)
        if name == "autolink":
            url = self.link(match.group(1))
            return '<a href="%s">%s</a>' % (attr(url), esc(match.group(1)))
        if name == "escape":
            return esc(match.group(1))
        if name == "link":
            return self._link(match)
        if name == "strong":
            inner = match.group("b") if match.group("b") is not None else match.group("b2")
            return "<strong>%s</strong>" % self.inline(inner)
        if name == "em":
            return "<em>%s</em>" % self.inline(match.group("i"))
        return "<del>%s</del>" % self.inline(match.group("s"))

    def _link(self, match: re.Match) -> str:
        target = match.group("target")
        alt = match.group("alt")
        title = match.group("title")
        image = match.group(0).startswith("!")
        href = self.link(target)
        if not image and target.startswith("#"):
            self.intra_page.add(target[1:])
        extra = ' title="%s"' % esc(title) if title else ""
        if image:
            return '<img src="%s" alt="%s"%s>' % (attr(href), attr(strip_inline(alt)), extra)
        return '<a href="%s"%s>%s</a>' % (attr(href), extra, self.inline(alt))

    # -- blocks ---------------------------------------------------------------
    def render(self, text: str) -> str:
        return "\n".join(self.blocks(text.split("\n")))

    def blocks(self, lines: list[str]) -> list[str]:
        out: list[str] = []
        i, n = 0, len(lines)
        while i < n:
            line = lines[i]
            if not line.strip():
                i += 1
                continue
            fence = FENCE.match(line)
            if fence:
                body, lang, i = self.fence(lines, i)
                label = ' data-lang="%s" class="language-%s"' % (attr(lang), attr(lang)) if lang else ""
                out.append("<pre%s><code>%s</code></pre>" % (label, body))
                continue
            if COMMENT.fullmatch(line.strip()):
                out.append(line.strip())
                i += 1
                continue
            atx = ATX.match(line)
            if atx:
                out.append(self.heading(len(atx.group(2)), atx.group(3)))
                i += 1
                continue
            if BREAK.match(line):
                out.append("<hr>")
                i += 1
                continue
            if QUOTE.match(line):
                quote, i = self.quote(lines, i)
                out.append("<blockquote>\n%s\n</blockquote>" % "\n".join(self.blocks(quote)))
                continue
            if i + 1 < n and "|" in line and is_delimiter(lines[i + 1]):
                table, i = self.table(lines, i)
                out.append(table)
                continue
            if LIST_ITEM.match(line):
                listing, i = self.listing(lines, i)
                out.append(listing)
                continue
            para, i = paragraph(lines, i)
            out.append("<p>%s</p>" % self.inline("\n".join(para)))
        return out

    def heading(self, level: int, raw: str) -> str:
        text = re.sub(r"\s+#+\s*$", "", raw.strip())
        base = gen_docs.anchor(strip_inline(text)) or "section"
        count = self.ids.get(base, 0)
        self.ids[base] = count + 1
        hid = base if count == 0 else "%s-%d" % (base, count + 1)
        plain = strip_inline(text)
        self.headings.append((level, hid, plain))
        return '<h%d id="%s">%s</h%d>' % (level, attr(hid), self.inline(text), level)

    def fence(self, lines: list[str], i: int) -> tuple[str, str, int]:
        first = FENCE.match(lines[i])
        marker, info = first.group(1), first.group(2)
        lang = info.split(",")[0].split(" ")[0].strip()
        body: list[str] = []
        j = i + 1
        closed = False
        tick = marker[0]
        while j < len(lines):
            candidate = lines[j].strip()
            # CommonMark's own rule, and the one this repository has tripped over: a fence closes on a
            # bare run of the same character with nothing after it — a line like ```text is content,
            # not a closer, and pairing it as a closer swallows the rest of the page one block at a time.
            if candidate and set(candidate) == {tick} and len(candidate) >= len(marker):
                closed = True
                break
            body.append(lines[j])
            j += 1
        for marker_line in generated_markers(body):
            self.problems.append("a code fence opened at line %d contains %s: a generated block carries "
                                 "its own fence, so the outer one has to go"
                                 % (i + 1, marker_line))
        if not closed:
            self.problems.append("unclosed code fence at line %d" % (i + 1))
        if self.audit:
            self.sink.append("\n".join(body))
            return "", lang, j + 1
        return esc("\n".join(body)), lang, j + 1

    def quote(self, lines: list[str], i: int) -> tuple[list[str], int]:
        body: list[str] = []
        n = len(lines)
        while i < n:
            match = QUOTE.match(lines[i])
            if match:
                body.append(match.group(2))
                i += 1
                continue
            # A lazy continuation line belongs to the quoted paragraph it follows.
            if lines[i].strip() and not starts_block(lines, i):
                body.append(lines[i].strip())
                i += 1
                continue
            break
        return body, i

    def table(self, lines: list[str], i: int) -> tuple[str, int]:
        header = split_row(lines[i])
        aligns = alignments(lines[i + 1])
        rows: list[list[str]] = []
        j = i + 2
        n = len(lines)
        while j < n and "|" in lines[j] and lines[j].strip():
            if FENCE.match(lines[j]) or ATX.match(lines[j]):
                break
            rows.append(split_row(lines[j]))
            j += 1
        width = len(header)
        out = ["<table>", "<thead>", "<tr>"]
        for index, cell in enumerate(header):
            out.append('<th%s>%s</th>' % (align(aligns, index), self.inline(cell)))
        out += ["</tr>", "</thead>", "<tbody>"]
        for row in rows:
            cells = list(row)
            if len(cells) < width:
                cells += [""] * (width - len(cells))
            elif len(cells) > width and not self.audit:
                # GFM drops a row's extra cells and leaves a short row's missing ones empty, and this
                # renders GFM. The audit pass keeps every cell instead, so a row that was written with
                # too many cells — usually an unescaped `|` inside a code span — cannot lose its text
                # quietly: the word audit sees the difference and says which page.
                cells = cells[:width]
            out.append("<tr>")
            for index, cell in enumerate(cells):
                tag = "th" if index == 0 and width == 2 else "td"
                out.append("<%s%s>%s</%s>" % (tag, align(aligns, index), self.inline(cell), tag))
            out.append("</tr>")
        out += ["</tbody>", "</table>"]
        return "\n".join(out), j

    def listing(self, lines: list[str], i: int) -> tuple[str, int]:
        """One list: items, their nested blocks, and lazy continuations."""
        first = LIST_ITEM.match(lines[i])
        indent = len(first.group(1))
        ordered = bool(ORDERED_ITEM.match(lines[i]))
        start = int(ORDERED_ITEM.match(lines[i]).group(2)) if ordered else 1
        items: list[list[str]] = []
        content_indent = indent + 2
        loose = False
        n = len(lines)
        while i < n:
            line = lines[i]
            if not line.strip():
                j = i
                while j < n and not lines[j].strip():
                    j += 1
                if j < n and self._own_item(lines[j], indent, ordered):
                    loose = True
                    if items:
                        items[-1].append("")
                    i = j
                    continue
                i = j
                break
            match = LIST_ITEM.match(line)
            if match and len(match.group(1)) == indent and bool(ORDERED_ITEM.match(line)) == ordered:
                content_indent = len(match.group(1)) + len(match.group(2)) + 1
                items.append([match.group(3)])
                i += 1
                continue
            if items and line.startswith(" " * (indent + 2)):
                items[-1].append(dedent(line, content_indent))
                i += 1
                continue
            if items and line.strip() and not starts_block(lines, i):
                items[-1].append(line.strip())       # lazy continuation
                i += 1
                continue
            break
        tag = "ol" if ordered else "ul"
        start_attr = ' start="%d"' % start if ordered and start != 1 else ""
        out = ["<%s%s>" % (tag, start_attr)]
        for item in items:
            body = self.blocks(list(item))
            if not loose and len(body) == 1 and body[0].startswith("<p>") and body[0].endswith("</p>"):
                body = [body[0][3:-4]]
            out.append("<li>%s</li>" % "\n".join(body))
        out.append("</%s>" % tag)
        return "\n".join(out), i

    def _own_item(self, line: str, indent: int, ordered: bool) -> bool:
        match = LIST_ITEM.match(line)
        return bool(match) and len(match.group(1)) == indent \
            and bool(ORDERED_ITEM.match(line)) == ordered


def starts_block(lines: list[str], i: int) -> bool:
    line = lines[i]
    return bool(FENCE.match(line) or ATX.match(line) or BREAK.match(line) or QUOTE.match(line)
                or LIST_ITEM.match(line) or (i + 1 < len(lines) and "|" in line
                                             and is_delimiter(lines[i + 1])))


def paragraph(lines: list[str], i: int) -> tuple[list[str], int]:
    body = [lines[i]]
    i += 1
    while i < len(lines) and lines[i].strip() and not starts_block(lines, i):
        body.append(lines[i])
        i += 1
    return body, i


def is_delimiter(line: str) -> bool:
    """The `|---|:--:|` row that makes a pipe table a table rather than a paragraph with bars."""
    text = line.strip()
    if not text or "|" not in text and "-" not in text:
        return False
    cells = split_row(line)
    return bool(cells) and all(re.fullmatch(r":?-{1,}:?", cell) for cell in cells)


def split_row(line: str) -> list[str]:
    """Split a table row on unescaped `|`, dropping the optional outer pipes."""
    text = line.strip()
    if text.startswith("|"):
        text = text[1:]
    if text.endswith("|") and not text.endswith("\\|"):
        text = text[:-1]
    cells: list[str] = []
    current: list[str] = []
    index = 0
    while index < len(text):
        char = text[index]
        if char == "\\" and index + 1 < len(text) and text[index + 1] == "|":
            current.append("|")
            index += 2
            continue
        if char == "|":
            cells.append("".join(current).strip())
            current = []
            index += 1
            continue
        current.append(char)
        index += 1
    cells.append("".join(current).strip())
    return cells


def alignments(line: str) -> list[str]:
    out = []
    for cell in split_row(line):
        left, right = cell.startswith(":"), cell.endswith(":")
        out.append("center" if left and right else "right" if right else "left" if left else "")
    return out


def align(alignments: list[str], index: int) -> str:
    if index < len(alignments) and alignments[index] in ("center", "right"):
        return ' style="text-align: %s"' % alignments[index]
    return ""


def dedent(line: str, width: int) -> str:
    """Strip up to `width` leading spaces — the amount a list marker occupied, no more."""
    stripped = line.lstrip(" ")
    return stripped if len(line) - len(stripped) >= width else line[width:]


FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)")
ATX = re.compile(r"^( {0,3})(#{1,6})\s+(.*)$")
BREAK = re.compile(r"^ {0,3}([-*_])[ \t]*\1[ \t]*\1(?:[ \t]*\1)*[ \t]*$")
QUOTE = re.compile(r"^( {0,3})> ?(.*)$")
UNORDERED_ITEM = re.compile(r"^( *)([-*+]) +(.*)$")
ORDERED_ITEM = re.compile(r"^( *)(\d{1,9})[.)] +(.*)$")
LIST_ITEM = re.compile(r"^( *)([-*+]|\d{1,9}[.)]) +(.*)$")


# ---------------------------------------------------------------------------
# the site: configuration, pages, links
# ---------------------------------------------------------------------------
#: Sidebar labels for the directories under `docs/`. A directory that is not listed still gets a
#: section, with its own name — so a new directory appears in the site rather than disappearing.
SECTION_LABELS = {
    "": "Start here",
    "adr": "Decision records",
    "concepts": "Concepts",
    "demo": "The scripted demo",
    "detectors": "Detector catalogue",
    "harness": "Harness adapters",
    "recipes": "Recipes",
    "reference": "Reference",
    "security": "Security",
}


@dataclass
class Config:
    """Everything the build is allowed to know about the outside world."""

    root: pathlib.Path = ROOT
    docs: pathlib.Path = DOCS
    source_url: str = ""      # blob base for links that leave `docs/`, e.g. `…/fathom/blob/master`
    repo_url: str = ""        # the repository home, for the header and the per-page source link
    base_url: str = ""        # deployed site root, with its trailing slash; empty means "local"
    title: str = SITE_TITLE


@dataclass
class Page:
    source: pathlib.Path
    out: str
    title: str
    section: str
    body: str
    headings: list[tuple[int, str, str]]
    description: str
    prefix: str
    #: A directory listing the builder wrote, not a page that exists in `docs/`. It gets the shell and
    #: the sidebar like any other page, and no "edit this page" link, because there is nothing to edit.
    synthesized: bool = False


def git(root: pathlib.Path, *args: str) -> str:
    """`git` if it is there and says something; empty string if it is not or does not."""
    try:
        done = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True,
                              timeout=15)
    except (OSError, subprocess.SubprocessError):
        return ""
    return done.stdout.strip() if done.returncode == 0 else ""


def source_urls(root: pathlib.Path = ROOT) -> tuple[str, str]:
    """Derive `(blob base, repository home)` for this checkout: CI's environment first, then `git`.

    Nothing here is hardcoded, because a link that names somebody's fork as this project is a lie that
    survives until a reader clicks it. GitHub Pages deploys and Actions builds both export
    `GITHUB_REPOSITORY` and a ref name; a plain local clone falls back to its `origin` remote and its
    current branch (detached: the short `HEAD` sha, which is what a detached tree's links should say).
    """
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com").rstrip("/")
    repo = os.environ.get("GITHUB_REPOSITORY", "").strip("/")
    ref = os.environ.get("GITHUB_REF_NAME", "")
    if not repo:
        remote = git(root, "config", "--get", "remote.origin.url")
        found = re.search(r"[:/]([^/]+)/([^/]+?)(?:\.git)?/?$", remote)
        if found:
            repo = "%s/%s" % (found.group(1), found.group(2))
        if not ref:
            ref = git(root, "rev-parse", "--abbrev-ref", "HEAD")
            if ref in ("", "HEAD"):
                ref = git(root, "rev-parse", "--short=10", "HEAD") or "HEAD"
    if not repo:
        return "", ""
    return "%s/%s/blob/%s" % (server, repo, ref or "HEAD"), "%s/%s" % (server, repo)


def root_prefix(out: str) -> str:
    """How far up from a built page to the site root: `""`, `"../"`, `"../../"`.

    Everything the shell emits is relative for this reason, which is what lets the same artefact be
    served from `https://user.github.io/fathom/` and opened from `file:///tmp/site/index.html`.
    """
    return "../" * out.count("/")


def html_text(markup: str) -> str:
    """The words a page shows: tags gone, image alts kept, entities decoded."""
    return html_escape_mod.unescape(TAG.sub(" ", IMG_ALT.sub(r"\1", markup)))


def skeleton(text: str) -> str:
    """Only the letters and digits, lowercased and run together: what two renderings can be compared on.

    Nothing else survives, which is the point. Markdown punctuation (`|`, `#`, `*`, backticks) and HTML
    markup (`<strong>`, `</td>`) both vanish, so the comparison says exactly one thing: the page contains
    the characters the markdown contained, in the order it contained them. Comparing *words* instead was
    wrong, because a tag boundary is a space to an HTML reader and not to a markdown reader — `**x**y`
    and `| x | y |` are both one word and two words at once.
    """
    return re.sub(r"[^0-9a-z]+", "", text.lower())


IMG_ALT = re.compile(r"<img[^>]*\balt=\"([^\"]*)\"[^>]*>", re.I)
TAG = re.compile(r"<[^>]+>")
RAW_TAG = re.compile(r"</?([a-zA-Z][a-zA-Z0-9-]*)")
#: Every element the renderer is allowed to write into a page's article. A rendered page that contains
#: a tag outside this set got it from the markdown, and the markdown cannot write tags: they are escaped.
#: The shell's own markup (nav, header, script, form) is not in scope — the rule is about the render.
ALLOWED_TAGS = frozenset("""h1 h2 h3 h4 h5 h6 p pre code a img strong em del table thead tbody tr th
    td ul ol li blockquote br hr""".split())
HREF = re.compile(r"(?:href|src)=\"([^\"]*)\"")
ANCHOR_ID = re.compile(r"\bid=\"([^\"]+)\"")
SCHEME = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.\-]*:")


class Builder:
    """Turns `docs/**.md` into a mapping of built path to file content, plus the problems it found.

    `generate()` never touches the output directory: the whole site is built in memory, so `check` can
    build it twice and diff it, and `write` can refuse to write half a site when a page does not build.
    """

    def __init__(self, cfg: Config):
        self.cfg = cfg
        self.problems: list[str] = []
        self.sources = sorted(p for p in cfg.docs.rglob("*.md") if p.is_file())
        # A hand-written `.html` under `docs/` is not copied: it would claim a path that a rendered page
        # already owns (`docs/demo/README.md` is `demo/index.html`, and so is any `demo/index.html` someone
        # left next to it). It is reported (see the note in `main`), not dropped quietly, and
        # `generate()` refuses to let any remaining asset overwrite a page path either.
        self.all_assets = sorted(p for p in cfg.docs.rglob("*")
                                 if p.is_file() and p.suffix.lower() != ".md")
        self.handwritten = [p for p in self.all_assets if p.suffix.lower() in (".html", ".htm")]
        self.assets = [p for p in self.all_assets if p.suffix.lower() not in (".html", ".htm")]
        self.out_of = {str(p): self.out_path(p) for p in self.sources}
        self.asset_out = {str(p): p.relative_to(cfg.docs).as_posix() for p in self.assets}
        self.pages: list[Page] = []
        self.audit: dict[str, str] = {}        # built page -> the words its source contained
        self.bodies: dict[str, str] = {}       # built page -> the rendered markdown, before the shell

    # -- paths --------------------------------------------------------------
    def out_path(self, src: pathlib.Path) -> str:
        """`docs/concepts/delta.md` → `concepts/delta.html`; a directory's `README.md` → `index.html`.

        The README rule is what makes `docs/adr/` (a link style this documentation uses a lot) resolve
        to a page that exists, and it is only applied where the directory has no `index.md` to displace.
        """
        parts = list(src.relative_to(self.cfg.docs).parts)
        if parts[-1] == "index.md":
            parts[-1] = "index.html"
        elif parts[-1] == "README.md" and not (src.parent / "index.md").exists():
            parts[-1] = "index.html"
        else:
            parts[-1] = src.stem + ".html"
        return "/".join(parts)

    def resolve(self, target: str, source: pathlib.Path) -> str:
        """The `href` for a markdown link target written on `source`.

        See the table in the module docstring; this is that table as code. Every branch that cannot do
        something quietly records a problem instead of guessing, because a guessed `href` looks fine and
        404s in the browser.
        """
        target = target.strip()
        if not target:
            return "#"
        if SCHEME.match(target) or target.startswith("//"):
            return target
        file_part, _, fragment = target.partition("#")
        suffix = "#" + fragment if fragment else ""
        if not file_part:
            return suffix or "#"
        here = os.path.normpath(os.path.join(os.path.dirname(str(source)), file_part))
        docs = str(self.cfg.docs)
        if here == docs or here.startswith(docs + os.sep):
            target_path = pathlib.Path(here)
            found: str | None = None
            if target_path.is_dir():
                for name in ("index.md", "README.md"):
                    candidate = target_path / name
                    if candidate.exists():
                        found = self.out_of.get(str(candidate))
                        break
                if found is None and any(page.suffix.lower() == ".md" for page in target_path.rglob("*.md")):
                    # A directory the site builds pages for still has a page: `section_indexes` writes
                    # the listing GitHub would have shown, and `[concepts](concepts/)` lands there.
                    found = "%s/index.html" % target_path.relative_to(self.cfg.docs).as_posix()
                if found is None:
                    self.problems.append("%s: link to %r names a directory with nothing in it"
                                         % (self.rel(source), target))
            elif target_path.suffix.lower() == ".md":
                found = self.out_of.get(here)
                if found is None:
                    self.problems.append("%s: link to %r names a markdown file the site does "
                                         "not build" % (self.rel(source), target))
            else:
                found = self.asset_out.get(here)
                if found is None:
                    self.problems.append("%s: link to %r names a file that is not in docs/"
                                         % (self.rel(source), target))
            if found is None:
                return esc(target) + suffix
            return posixpath.relpath(found, posixpath.dirname(self.out_of[str(source)])) + suffix
        if not self.cfg.source_url:
            self.problems.append("%s: link to %r leaves docs/ and no source URL is configured "
                                 "(pass --source-url)" % (self.rel(source), target))
            return esc(target) + suffix
        path = pathlib.Path(here)
        try:
            repo_rel = path.relative_to(self.cfg.root).as_posix()
        except ValueError:
            self.problems.append("%s: link to %r resolves outside the repository"
                                 % (self.rel(source), target))
            return esc(target) + suffix
        base = self.cfg.source_url
        if path.is_dir():
            base = base.replace("/blob/", "/tree/", 1)
        return "%s/%s%s" % (base.rstrip("/"), repo_rel, suffix)

    def rel(self, path: pathlib.Path) -> str:
        try:
            return str(path.relative_to(self.cfg.root))
        except ValueError:
            return str(path)

    # -- build --------------------------------------------------------------
    def generate(self) -> dict[str, "str | bytes"]:
        files: dict[str, "str | bytes"] = {}
        for source in self.sources:
            text = source.read_text(encoding="utf-8")
            renderer = Renderer(lambda target, s=source: self.resolve(target, s))
            body = renderer.render(text)
            audit = Renderer(lambda target, s=source: target)
            audit.audit = True
            audit.render(text)
            self.audit[self.out_of[str(source)]] = " ".join(audit.sink)
            self.bodies[self.out_of[str(source)]] = body
            self.problems.extend("%s: %s" % (self.rel(source), problem)
                                 for problem in renderer.problems)
            title = page_title(renderer, source)
            self.pages.append(Page(source=source, out=self.out_of[str(source)], title=title,
                                   section=posixpath.dirname(self.out_of[str(source)]),
                                   body=body, headings=renderer.headings,
                                   description=page_description(body),
                                   prefix=root_prefix(self.out_of[str(source)])))
        pages = sorted(self.pages + self.section_indexes(), key=lambda page: page.out)
        self.pages = pages
        for page in pages:
            files[page.out] = self.shell(page)
        for source in self.assets:
            name = self.asset_out[str(source)]
            if name in files:
                self.problems.append("the site would write %s twice: a rendered page and %s claim the "
                                     "same path"
                                     % (name, self.rel(source)))
            files[name] = source.read_bytes()
        for name, content in self.extras(pages).items():
            if name in files:
                self.problems.append("the site would write %s twice" % name)
            files[name] = content
        return files

    def sections(self, pages: list[Page]) -> list[tuple[str, str, list[Page]]]:
        """(directory, label, pages) with the site root first and each directory's own index first."""
        grouped: dict[str, list[Page]] = {}
        for page in pages:
            grouped.setdefault(page.section, []).append(page)
        out = []
        for section in sorted(grouped, key=lambda s: (s != "", s)):
            members = sorted(grouped[section],
                             key=lambda page: (not page.out.endswith("index.html"), page.out))
            out.append((section, SECTION_LABELS.get(section, posixpath.basename(section)), members))
        return out

    def nav(self, pages: list[Page], current: str) -> str:
        """The sidebar: one `<details>` per directory, open for the section you are reading.

        It is built per page rather than once, so that the current page and its section are marked in
        the HTML, and so that every link is relative to the page it is printed on: a sidebar link written
        from the linked page's own depth is a 404 the moment you read the site from a subdirectory.
        """
        current_section = posixpath.dirname(current)
        prefix = root_prefix(current)
        out = ['<nav class="sidebar" aria-label="Documentation">']
        for section, label, members in self.sections(pages):
            inner = []
            for page in members:
                href = prefix + page.out
                if page.out == current:
                    inner.append('<a href="%s" aria-current="page">%s</a>'
                                 % (attr(href), esc(page.title)))
                else:
                    inner.append('<a href="%s">%s</a>' % (attr(href), esc(page.title)))
            listing = "\n".join(inner)
            out.append('<section class="section" data-section="%s">' % attr(section or "."))
            if section:
                out.append('<details%s><summary>%s</summary><div class="pages">%s</div></details>'
                           % (" open" if section == current_section else "", esc(label), listing))
            else:
                out.append('<h2 class="section-title">%s</h2><div class="pages">%s</div>'
                           % (esc(label), listing))
            out.append("</section>")
        out.append("</nav>")
        return "\n".join(out)

    def section_indexes(self) -> list[Page]:
        """A front page for every directory in `docs/` that does not have one.

        `docs/index.md` links `[concepts](concepts/)`, and on GitHub that target is a directory listing.
        A Pages site has no listings, so the builder writes the listing: the section's pages, each with
        the first line of its own description — the same list the sidebar shows, so it cannot drift from
        it. `docs/adr/`, `docs/recipes/`, `docs/harness/` and `docs/demo/` already carry a `README.md`
        that a human wrote, and those are left alone.
        """
        have = {page.section for page in self.pages if page.out.endswith("index.html")}
        made: list[Page] = []
        for section, label, members in self.sections(self.pages):
            if not section or section in have:
                continue
            out = "%s/index.html" % section
            intro = ("%d pages under <code>docs/%s/</code>. This is the same list the panel on the "
                     "left shows, and <a href=\"index.html\">the front page</a> says where these fit." %
                     (len(members), esc(section)))
            lines = ["<h1>%s</h1>" % esc(label), "<p>%s</p>" % intro, "<ul>"]
            for page in sorted(members, key=lambda page: page.out):
                lines.append('<li><a href="%s">%s</a> — %s</li>'
                             % (attr(posixpath.relpath(page.out, section)), esc(page.title),
                                esc(page.description)))
            lines.append("</ul>")
            page = Page(source=self.cfg.docs / section, out=out, title=label, section=section,
                        body="\n".join(lines), headings=[(1, gen_docs.anchor(label), label)],
                        description=html_text(intro), prefix=root_prefix(out), synthesized=True)
            self.bodies[out] = page.body
            made.append(page)
        return made

    def shell(self, page: Page) -> str:
        prefix = page.prefix
        canonical = ("\n<link rel=\"canonical\" href=\"%s\">" % attr(self.cfg.base_url + page.out)
                     if self.cfg.base_url else "")
        toc = toc_list(page.headings)
        crumbs = breadcrumbs(page, self.cfg)
        source_link = ""
        if page.synthesized:
            source_link = "<span>this page is generated: it is the directory's listing</span>"
        elif self.cfg.source_url:
            repo_rel = page.source.relative_to(self.cfg.root).as_posix()
            source_link = ('<a class="source" href="%s">edit this page</a>'
                           % attr("%s/%s" % (self.cfg.source_url.rstrip("/"), repo_rel)))
        return PAGE_TEMPLATE % {
            "title": esc(page.title),
            "site_title": esc(self.cfg.title),
            "description": esc(page.description),
            "canonical": canonical,
            "prefix": prefix,
            "nav": self.nav(self.pages, page.out),
            "crumbs": crumbs,
            "body": page.body,
            "toc": toc,
            "source_link": source_link,
            "repo": ('<a href="%s">source</a>' % attr(self.cfg.repo_url)) if self.cfg.repo_url else "",
        }

    def extras(self, pages: list[Page]) -> dict[str, "str | bytes"]:
        """Everything that is not a rendered page: the stylesheet, the search, the 404, the map."""
        out: dict[str, "str | bytes"] = {
            "assets/docs.css": DOCS_CSS,
            "assets/docs.js": DOCS_JS,
            "assets/search.js": search_script(pages),
            "404.html": self.not_found(pages),
        }
        lines = ["<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                 '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">']
        if self.cfg.base_url:
            # A sitemap is a list of absolute URLs or it is nothing: `<loc>concepts/delta.html</loc>` is not
            # a address a crawler can use, and a crawler is the only reader of this file. Without a known
            # host the site writes neither this nor robots.txt, and says so in the report.
            for page in sorted(pages, key=lambda page: page.out):
                lines.append("  <url><loc>%s</loc></url>" % attr(self.cfg.base_url + page.out))
        lines += ["</urlset>", ""]
        if self.cfg.base_url:
            out["sitemap.xml"] = "\n".join(lines)
            robots = ["User-agent: *", "Allow: /",
                      "Sitemap: %ssitemap.xml" % self.cfg.base_url]
            out["robots.txt"] = "\n".join(robots + [""])
        return out

    def not_found(self, pages: list[Page]) -> str:
        body = ("<h1>That page is not here</h1>\n"
                "<p>The address you followed is not a page of this site. The manual starts at "
                "<a href=\"index.html\"><em>Fathom documentation</em></a>; the search box above "
                "searches every page's title and headings, and the panel on the left is the whole "
                "site in order.</p>"
                "<p>If you followed a link from another page, that link is a bug in the docs: "
                "<code>python3 tools/check_docs_site.py</code> is the gate that is supposed to "
                "catch it, so please report it.</p>")
        page = Page(source=self.cfg.docs / "index.md", out="404.html", title="Page not found",
                    section="", body=body, headings=[], description="Page not found", prefix="")
        self.bodies[page.out] = body
        return self.shell(page)

    def lint_subset(self) -> list[str]:
        """Refuse what the renderer does not implement, instead of rendering it wrong.

        The subset is the one in the module docstring. Two members of it are worth a gate rather than a
        comment: raw HTML (which this builder escapes, so a page that writes `<details>` would show
        `<details>`) and reference-style link definitions (which this builder does not resolve, so
        `[x][ref]` would print literally). Both are invisible in a markdown diff and obvious in a
        browser, which is exactly the class of thing a gate is for.
        """
        problems = []
        for source in self.sources:
            naked = without_code(source.read_text(encoding="utf-8"))
            for match in STRUCTURAL_HTML.finditer(naked):
                problems.append("%s: raw HTML %s in the markdown — the site builder escapes HTML, so "
                                "write it in the supported subset (or put it in a code span if it is a "
                                "placeholder)" % (self.rel(source), match.group(0)))
            for line in naked.splitlines():
                if REFERENCE_LINK.match(line):
                    problems.append("%s: reference-style link definition %r — the builder renders "
                                    "links inline" % (self.rel(source), line.strip()))
        return problems

    # -- checks -------------------------------------------------------------
    def check(self, files: dict[str, "str | bytes"]) -> list[str]:
        """The invariants of the built site, as problem lines (empty means the site is good).

        Nothing here re-reads the markdown: every rule is asserted on the artefact that gets published,
        because that is the thing a reader judges.
        """
        problems = list(self.problems)
        html_pages = {name: value for name, value in files.items()
                      if name.endswith(".html") and isinstance(value, str)}
        ids = {name: set(ANCHOR_ID.findall(value)) for name, value in html_pages.items()}
        for name, value in sorted(html_pages.items()):
            body = self.bodies.get(name, value)
            for ref in HREF.findall(value):
                if not ref:
                    continue
                if ref.startswith("/"):
                    problems.append("%s: %r is an absolute path, which will not resolve under a "
                                    "site subdirectory" % (name, ref))
                    continue
                if SCHEME.match(ref) or ref.startswith("//"):
                    continue
                file_part, _, fragment = ref.partition("#")
                target = name if not file_part else posixpath.normpath(
                    posixpath.join(posixpath.dirname(name), file_part))
                if target.startswith("../"):
                    problems.append("%s: link %r escapes the built site" % (name, ref))
                    continue
                if target not in files:
                    problems.append("%s: link %r has no target in the built site" % (name, ref))
                    continue
                if fragment and target in ids and fragment not in ids[target]:
                    problems.append("%s: fragment %r does not match a heading in %s"
                                    % (name, "#" + fragment, target))
            for href in re.findall(r'href="([^"#]*)"', value):
                if href.endswith(".md") and not SCHEME.match(href):
                    problems.append("%s: href %r is still markdown; the builder left a .md link"
                                    % (name, href))
            for foreign in foreign_tags(body):
                problems.append("%s: the rendered page contains raw HTML <%s> — the builder escapes "
                                "HTML, so it came from somewhere it should not have"
                                % (name, foreign))
            for broken in unbalanced_tags(value):
                problems.append("%s: %s — a browser would repair this by guessing, and the guess "
                                "would be the page" % (name, broken))
            wanted = self.audit.get(name)
            if wanted is not None:
                missing = audit_skeletons(wanted, html_text(body))
                if missing:
                    problems.append("%s: the render does not contain what the markdown contained "
                                    "(first difference: %s)" % (name, missing))
        problems.extend(self.lint_subset())
        for page in self.pages:
            if page.out not in files:
                problems.append("%s produced no page" % self.rel(page.source))
            if not page.headings:
                problems.append("%s: no heading, so no title and no place to link it from"
                                % self.rel(page.source))
        for required in ("index.html", "404.html", "assets/docs.css", "assets/docs.js",
                         "assets/search.js"):
            if required not in files:
                problems.append("the built site has no %s" % required)
        if not self.pages:
            problems.append("no markdown under %s" % self.cfg.docs)
        return problems


def without_code(text: str) -> str:
    """The page's prose: fenced and inline code gone, so a lint can look at markup rather than examples.

    Without this, half the code samples in this repository — `<episode>` placeholders, a `markdown`
    fence quoting a page, a `toml` block full of `<url>` — would trip the subset lint for content that
    is already safe because it renders as text.
    """
    kept: list[str] = []
    inside = False
    for line in text.splitlines():
        if FENCE.match(line):
            inside = not inside
            continue
        if inside:
            continue
        kept.append(line)
    return CODE_SPAN.sub("", "\n".join(kept))


def generated_markers(strand: list[str]) -> list[str]:
    """The `<!-- BEGIN/END GENERATED: x -->` markers inside a block of lines.

    A fence that contains one of these is almost always an accident: the block between two markers is
    `gen_docs.py`'s own text — fence lines included — and wrapping it in a second fence makes the outer
    opener close on the inner one and the rest of the page fall into a code block, on GitHub as much as
    here. The repair is to delete the outer fence, never to move a marker.
    """
    return [line.strip() for line in strand if GENERATED_MARKER.search(line)]


GENERATED_MARKER = re.compile(r"<!-- *(?:BEGIN|END) GENERATED: [\w.\-]+ *-->")


def unbalanced_tags(markup: str) -> list[str]:
    """Tags that never close, or close out of order — a render a browser would silently repair.

    A browser fixes broken nesting by guessing, which is how a table that lost a `</td>` becomes a page
    where half the manual has moved. This says so instead.
    """
    parser = _Balance()
    parser.feed(markup)
    return parser.errors + ["%s (line %d) is never closed" % (tag, line) for tag, line in parser.stack]


class _Balance(HTMLParser):
    VOID = frozenset(("img", "br", "hr", "input", "meta", "link"))

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.stack: list[tuple[str, int]] = []
        self.errors: list[str] = []

    def handle_starttag(self, tag: str, attrs) -> None:
        if tag not in self.VOID:
            self.stack.append((tag, self.getpos()[0]))

    def handle_endtag(self, tag: str) -> None:
        if tag in self.VOID:
            return
        for index in range(len(self.stack) - 1, -1, -1):
            if self.stack[index][0] == tag:
                for open_tag, line in self.stack[index + 1:]:
                    self.errors.append("%s (line %d) is closed by </%s>"
                                       % (open_tag, line, tag))
                del self.stack[index:]
                return
        self.errors.append("</%s> at line %d closes nothing" % (tag, self.getpos()[0]))


def foreign_tags(markup: str) -> list[str]:
    """Tags in a built page that the builder never writes — the escape rule, asserted on the output.

    HTML comments are excluded on purpose: passing `<!-- BEGIN GENERATED: … -->` through inert is a
    documented part of the subset, not a leak.
    """
    stripped = COMMENT.sub("", markup)
    return sorted({tag.lower() for tag in RAW_TAG.findall(stripped)
                   if tag.lower() not in ALLOWED_TAGS})


def audit_skeletons(wanted: str, got: str) -> str:
    """The first difference between the words the markdown had and the words the page has, or ``.

    This is the check that a renderer bug is not silent: a dropped table cell, a swallowed list item, a
    `<placeholder>` eaten as a tag. Both sides go through `skeleton`, so markdown punctuation and HTML
    markup are not noise — only characters that went missing, or appeared out of nowhere.
    """
    left, right = skeleton(wanted), skeleton(got)
    if left == right:
        return ""
    index = next((position for position, (mine, theirs) in enumerate(zip(left, right))
                  if mine != theirs), min(len(left), len(right)))
    return ("after %r the markdown continues %r and the page continues %r"
            % (left[max(0, index - 30):index] or "the start", left[index:index + 30] or "nothing",
               right[index:index + 30] or "nothing"))


def page_title(renderer: Renderer, source: pathlib.Path) -> str:
    for level, _hid, text in renderer.headings:
        if level == 1:
            return text
    if renderer.headings:
        return renderer.headings[0][2]
    return source.stem.replace("-", " ").title()


def page_description(body: str) -> str:
    """The first real paragraph, for the `<meta name=\"description\">` search results use."""
    for match in re.finditer(r"<p>(.*?)</p>", body, re.S):
        text = html_text(match.group(1))
        text = re.sub(r"\s+", " ", text).strip()
        if len(text) > 20:
            return text[:280] + ("…" if len(text) > 280 else "")
    return SITE_TITLE


def toc_list(headings: list[tuple[int, str, str]]) -> str:
    """`On this page`: levels 2 and 3, which is where a page's sections live."""
    entries = [(level, hid, text) for level, hid, text in headings if level in (2, 3)]
    if len(entries) < 2:
        return ""
    out = ['<aside class="toc" aria-label="On this page"><h2>On this page</h2><ul>']
    for level, hid, text in entries[:60]:
        out.append('<li class="l%d"><a href="#%s">%s</a></li>' % (level, attr(hid), esc(text)))
    out += ["</ul></aside>"]
    return "\n".join(out)


def breadcrumbs(page: Page, cfg: Config) -> str:
    """`docs / section / page`. Every section has an index page by the time this runs — the ones that
    did not have one were written by `section_indexes`."""
    if page.out in ("index.html", "404.html"):
        return ""
    depth = page.out.count("/")
    out = ['<nav class="crumbs" aria-label="Breadcrumb">']
    out.append('<a href="%s">docs</a>' % attr("../" * depth + "index.html"))
    if page.section:
        label = SECTION_LABELS.get(page.section, page.section.split("/")[-1])
        out.append("<span>/</span>")
        if page.out == page.section + "/index.html":
            out.append("<span>%s</span>" % esc(label))
        else:
            out.append('<a href="index.html">%s</a>' % esc(label))
    out.append("<span>/</span>")
    out.append("<span>%s</span>" % esc(page.title))
    out.append("</nav>")
    return "\n".join(out)


def search_script(pages: list[Page]) -> str:
    """`assets/search.js`: titles, headings and the opening paragraph of every page.

    A locator, not a full-text index: it is 1 request, it works offline, it needs no server-side help,
    and the pages it lists are the pages the site has. Loading it lazily on the first keystroke keeps
    it off every page load (`assets/docs.js` does the lazy bit).
    """
    entries = []
    for page in sorted(pages, key=lambda page: page.out):
        entries.append({"url": page.out, "title": page.title,
                        "section": SECTION_LABELS.get(page.section,
                                                      posixpath.basename(page.section) or "docs"),
                        "headings": [text for _level, _hid, text in page.headings[1:]],
                        "text": page.description})
    return ("window.FATHOM_DOCS_SEARCH = %s;\n"
            % json.dumps(entries, indent=1, sort_keys=True, ensure_ascii=False))


# ---------------------------------------------------------------------------
# the shell, the stylesheet and the ten lines of script
# ---------------------------------------------------------------------------
#: There is no templating engine here, and `%(name)s` is enough of one. `%` in this string would have to
#: be doubled, so keep it out; the stylesheet and the script are separate files, not inline.
PAGE_TEMPLATE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>%(title)s · %(site_title)s</title>
<meta name="description" content="%(description)s">
<meta name="generator" content="tools/build_docs_site.py">
<link rel="stylesheet" href="%(prefix)sassets/docs.css">%(canonical)s
</head>
<body>
<a class="skip" href="#content">Skip to content</a>
<header class="masthead">
<a class="brand" href="%(prefix)sindex.html">Fathom <span>documentation</span></a>
<form class="search" role="search" onsubmit="return false">
<label class="visually-hidden" for="q">Search the documentation</label>
<input id="q" type="search" placeholder="Search the docs (/)" autocomplete="off"
       data-root="%(prefix)s">
<div id="search-results" class="search-results" hidden></div>
<noscript><p class="noscript">Search needs JavaScript. Everything else on this site — the sidebar, the
headings, the links — is in the HTML and works without it.</p></noscript>
</form>
%(repo)s
</header>
<div class="layout">
%(nav)s
<main id="content">
%(crumbs)s
<article>
%(body)s
</article>
<footer class="page-footer">%(source_link)s<span class="built">Built from <code>docs/</code> by
<code>tools/build_docs_site.py</code>.</span></footer>
</main>
%(toc)s
</div>
<script src="%(prefix)sassets/docs.js" defer></script>
</body>
</html>
"""

DOCS_CSS = """\
/* The Fathom documentation site. One stylesheet, no framework, no CDN: the site is built by
   `tools/build_docs_site.py` and has to work offline, from any base path, and in a print preview. */
:root {
  color-scheme: light dark;
  --bg: #ffffff;
  --panel: #f6f7f9;
  --code: #f2f3f5;
  --text: #1c1f24;
  --muted: #5c6370;
  --line: #dfe2e7;
  --accent: #2f5fd0;
  --mark: #fff3c4;
  --max: 46rem;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #14161a; --panel: #1b1e24; --code: #22262d; --text: #e6e8ec; --muted: #99a1af;
    --line: #2c313a; --accent: #7aa2ff; --mark: #4a3c10;
  }
}
* { box-sizing: border-box; }
body {
  margin: 0; background: var(--bg); color: var(--text);
  font: 16px/1.6 ui-sans-serif, system-ui, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
}
code, pre, kbd { font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace; }
a { color: var(--accent); text-decoration-thickness: 1px; text-underline-offset: 2px; }
a:hover { text-decoration-thickness: 2px; }
:is(a, input, summary):focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.skip, .visually-hidden {
  position: absolute; left: -10000px; top: auto; width: 1px; height: 1px; overflow: hidden;
}
.skip:focus { position: static; width: auto; height: auto; }
img { max-width: 100%; height: auto; }
hr { border: 0; border-top: 1px solid var(--line); margin: 2rem 0; }

.masthead {
  position: sticky; top: 0; z-index: 3; display: flex; gap: 1rem; align-items: center;
  padding: .6rem 1rem; border-bottom: 1px solid var(--line); background: var(--panel);
}
.brand { font-weight: 700; text-decoration: none; color: var(--text); white-space: nowrap; }
.brand span { color: var(--muted); font-weight: 400; }
.masthead > a:last-child { margin-left: auto; color: var(--muted); font-size: .9rem; }
.search { position: relative; margin-left: auto; }
.masthead > a:last-child + .search, .search { margin-left: auto; }
.search input {
  font: inherit; padding: .35rem .6rem; width: 17rem; max-width: 45vw;
  border: 1px solid var(--line); border-radius: 6px; background: var(--bg); color: var(--text);
}
.search-results {
  position: absolute; right: 0; top: calc(100% + .35rem); width: 26rem; max-width: 70vw;
  max-height: 60vh; overflow: auto; margin: 0; padding: .35rem; list-style: none;
  border: 1px solid var(--line); border-radius: 8px; background: var(--bg);
  box-shadow: 0 8px 24px rgba(0, 0, 0, .18);
}
.search-results li { margin: 0; padding: .35rem .5rem; border-radius: 6px; }
.search-results li[aria-selected="true"] { background: var(--panel); }
.search-results a { display: block; text-decoration: none; color: var(--text); font-weight: 600; }
.search-results .where { color: var(--muted); font: .8rem/1.4 ui-monospace, monospace; }
.noscript { margin: .4rem 0 0; color: var(--muted); font-size: .82rem; line-height: 1.4; }
.search-results .none { color: var(--muted); padding: .35rem .5rem; }

.layout { display: grid; grid-template-columns: 17rem minmax(0, 1fr) 15rem; align-items: start; }
.sidebar {
  position: sticky; top: 3.1rem; max-height: calc(100vh - 3.1rem); overflow: auto;
  padding: 1rem .75rem 3rem 1rem; border-right: 1px solid var(--line); font-size: .9rem;
}
.sidebar .section-title { font-size: .78rem; margin: .2rem 0 .4rem; color: var(--muted); }
.sidebar summary {
  font-size: .78rem; font-weight: 700; cursor: pointer; margin: .7rem 0 .3rem;
  color: var(--muted); text-transform: uppercase; letter-spacing: .04em;
}
.sidebar .pages { display: flex; flex-direction: column; }
.sidebar .pages a {
  padding: .12rem .4rem; border-radius: 5px; color: var(--text); text-decoration: none;
  border-left: 2px solid transparent;
}
.sidebar .pages a:hover { background: var(--panel); }
.sidebar .pages a[aria-current="page"] {
  background: var(--panel); border-left-color: var(--accent); font-weight: 600;
}
main { padding: 1.25rem 2rem 4rem; min-width: 0; }
article { max-width: var(--max); }
.crumbs { font-size: .8rem; color: var(--muted); margin-bottom: 1rem; }
.crumbs span { margin: 0 .35rem; }
.crumbs a { color: var(--muted); }
.toc {
  position: sticky; top: 3.1rem; max-height: calc(100vh - 3.1rem); overflow: auto;
  padding: 1.5rem 1rem 3rem 0; font-size: .82rem; color: var(--muted);
}
.toc h2 { font-size: .78rem; text-transform: uppercase; letter-spacing: .04em; margin: 0 0 .4rem; }
.toc ul { list-style: none; margin: 0; padding: 0; }
.toc li.l3 { padding-left: .8rem; }
.toc a { color: var(--muted); text-decoration: none; display: block; padding: .1rem 0; }
.toc a:hover, .toc a.active { color: var(--accent); }

h1 { font-size: 1.9rem; line-height: 1.25; margin: .2rem 0 1rem; }
h2 { font-size: 1.3rem; margin: 2.2rem 0 .6rem; padding-bottom: .25rem; border-bottom: 1px solid var(--line); }
h3 { font-size: 1.08rem; margin: 1.6rem 0 .4rem; }
h4, h5, h6 { font-size: 1rem; margin: 1.3rem 0 .3rem; }
h1, h2, h3 { overflow-wrap: anywhere; }
p, li { overflow-wrap: break-word; }
ul, ol { padding-left: 1.4rem; }
li { margin: .2rem 0; }
blockquote {
  margin: 1rem 0; padding: .1rem 0 .1rem 1rem; border-left: 3px solid var(--line); color: var(--muted);
}
code { background: var(--code); border-radius: 4px; padding: .1em .3em; font-size: .92em; }
pre {
  background: var(--code); border: 1px solid var(--line); border-radius: 8px;
  padding: .7rem .9rem; overflow-x: auto; tab-size: 4; line-height: 1.45;
}
pre code { background: none; padding: 0; font-size: .86rem; }
pre[data-lang] { position: relative; }
table { border-collapse: collapse; width: 100%; margin: 1rem 0; font-size: .93rem; }
th, td { border: 1px solid var(--line); padding: .35rem .55rem; vertical-align: top; text-align: left; }
thead th { background: var(--panel); }
tbody th { font-weight: 600; }
tbody tr:nth-child(even) { background: color-mix(in srgb, var(--panel) 55%, transparent); }
del { color: var(--muted); }
mark { background: var(--mark); }
.page-footer {
  margin-top: 3rem; padding-top: 1rem; border-top: 1px solid var(--line);
  display: flex; gap: 1.5rem; flex-wrap: wrap; font-size: .8rem; color: var(--muted);
}
.page-footer .source { font-weight: 600; }

@media (max-width: 1080px) {
  .layout { grid-template-columns: 15rem minmax(0, 1fr); }
  .toc { display: none; }
}
@media (max-width: 780px) {
  .layout { grid-template-columns: minmax(0, 1fr); }
  .sidebar { position: static; max-height: none; border-right: 0; border-bottom: 1px solid var(--line); }
  main { padding: 1rem 1rem 3rem; }
}
@media print {
  .masthead, .sidebar, .toc, .page-footer, .skip { display: none; }
  .layout { display: block; }
  main { padding: 0; }
  a { color: inherit; }
}
"""

DOCS_JS = """\
// The documentation site's script: search, and the current heading in the table of contents.
//
// It is deliberately small and offline. Nothing is fetched from another origin, the search index is a
// sibling file the builder writes, and it is loaded on the first keystroke rather than on every page
// load. A page without JavaScript is still a page: navigation, the table of contents and the sidebar
// are all in the HTML, and the search box is simply inert.
(function () {
  "use strict";
  var input = document.getElementById("q");
  var panel = document.getElementById("search-results");
  if (!input || !panel) { return; }
  var index = null, chosen = -1;

  function load(done) {
    if (index) { done(); return; }
    var script = document.createElement("script");
    script.src = input.dataset.root + "assets/search.js";
    script.onload = function () { index = window.FATHOM_DOCS_SEARCH || []; done(); };
    script.onerror = function () { panel.hidden = false; panel.textContent = "search index unavailable"; };
    document.body.appendChild(script);
  }

  function score(entry, words) {
    var title = entry.title.toLowerCase(), heads = entry.headings.join(" ").toLowerCase();
    var text = (entry.text || "").toLowerCase(), total = 0;
    for (var i = 0; i < words.length; i++) {
      var word = words[i];
      if (!word) { continue; }
      var at = title.indexOf(word);
      if (at === 0) { total += 40; } else if (at > 0) { total += 20; }
      if (heads.indexOf(word) >= 0) { total += 10; }
      if (text.indexOf(word) >= 0) { total += 4; }
      if (entry.section.toLowerCase().indexOf(word) >= 0) { total += 2; }
    }
    return total;
  }

  function render(query) {
    var words = query.toLowerCase().split(/\\s+/);
    var hits = [];
    index.forEach(function (entry) {
      var value = score(entry, words);
      if (value > 0) { hits.push([value, entry]); }
    });
    hits.sort(function (a, b) { return b[0] - a[0] || a[1].url.length - b[1].url.length; });
    panel.innerHTML = "";
    panel.hidden = false;
    if (!hits.length) {
      var none = document.createElement("div");
      none.className = "none";
      none.textContent = "nothing matches " + query;
      panel.appendChild(none);
      return;
    }
    var list = document.createElement("ul");
    hits.slice(0, 12).forEach(function (hit) {
      var item = document.createElement("li");
      var link = document.createElement("a");
      link.href = input.dataset.root + hit[1].url;
      link.textContent = hit[1].title;
      var where = document.createElement("span");
      where.className = "where";
      where.textContent = hit[1].section + " · " + hit[1].url;
      item.appendChild(link);
      item.appendChild(where);
      list.appendChild(item);
    });
    panel.appendChild(list);
    chosen = -1;
  }

  function move(step) {
    var items = panel.querySelectorAll("li");
    if (!items.length) { return; }
    chosen = (chosen + step + items.length) % items.length;
    items.forEach(function (item, i) { item.setAttribute("aria-selected", i === chosen); });
    items[chosen].querySelector("a").focus();
  }

  input.addEventListener("input", function () {
    var query = input.value.trim();
    if (!query) { panel.hidden = true; return; }
    load(function () { render(query); });
  });
  input.addEventListener("keydown", function (event) {
    if (event.key === "Escape") { panel.hidden = true; input.value = ""; return; }
    if (event.key === "ArrowDown") { event.preventDefault(); move(1); }
    if (event.key === "ArrowUp") { event.preventDefault(); move(-1); }
  });
  document.addEventListener("keydown", function (event) {
    if (event.key === "/" && document.activeElement !== input) {
      event.preventDefault();
      input.focus();
    }
  });
  document.addEventListener("click", function (event) {
    if (!panel.contains(event.target) && event.target !== input) { panel.hidden = true; }
  });

  var links = Array.prototype.slice.call(document.querySelectorAll(".toc a"));
  if (links.length) {
    var byId = {};
    links.forEach(function (link) { byId[link.getAttribute("href").slice(1)] = link; });
    var visible = function (entries) {
      entries.forEach(function (entry) {
        var link = byId[entry.target.id];
        if (!link) { return; }
        if (entry.isIntersecting) {
          links.forEach(function (other) { other.classList.remove("active"); });
          link.classList.add("active");
        }
      });
    };
    var observer = new IntersectionObserver(visible, { rootMargin: "0px 0px -75% 0px" });
    Array.prototype.forEach.call(document.querySelectorAll("article h2, article h3"), function (heading) {
      if (heading.id) { observer.observe(heading); }
    });
  }
}());
"""


# ---------------------------------------------------------------------------
# the two actions, and the self-test that proves they can fail
# ---------------------------------------------------------------------------
MARKER = ".docs-site-build"


def config(args: argparse.Namespace) -> Config:
    blob, repo = source_urls(args.root)
    return Config(root=args.root, docs=args.docs, source_url=args.source_url or blob,
                  repo_url=args.repo_url if args.repo_url is not None else repo,
                  base_url=args.base_url)


def build_all(cfg: Config) -> tuple[dict[str, "str | bytes"], list[str], "Builder"]:
    builder = Builder(cfg)
    files = builder.generate()
    problems = builder.check(files)
    return files, sorted(set(problems)), builder


def write_site(files: dict[str, "str | bytes"], out: pathlib.Path, clean: bool) -> list[str]:
    """Write the built site. Only ever writes inside `out`, and only after the whole build is good."""
    if clean and out.exists():
        marker = out / MARKER
        if not marker.exists():
            return ["refusing to clean %s: it has no %s, so this tool did not write it"
                    % (out, MARKER)]
        for name in sorted(out.rglob("*"), reverse=True):
            if name.is_file():
                name.unlink()
    out.mkdir(parents=True, exist_ok=True)
    for name, content in sorted(files.items()):
        target = out / name
        if not target.resolve().is_relative_to(out.resolve()):
            return ["refusing to write %s outside %s" % (name, out)]
        target.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            target.write_bytes(content)
        else:
            target.write_text(content, encoding="utf-8")
    (out / MARKER).write_text("written by tools/build_docs_site.py\n", encoding="utf-8")
    return []


def report(files: dict[str, "str | bytes"], builder: Builder, verbose: bool) -> None:
    pages = len(builder.pages)
    lines = [
        "pages: %d, from docs/**.md, each with a title and a heading id" % pages,
        "assets: %d non-markdown files copied verbatim" % len(builder.assets),
        "shell: one stylesheet, one script, one lazily-loaded search index over all %d pages" % pages,
        "links: relative everywhere, so the site works from a project-page subdirectory and from disk",
        "escape: raw HTML in the markdown is escaped; %d built pages contain no tag the builder did not write"
        % len(files),
        "audit: every word of every page's markdown is present in the rendered page",
        "remote links: %s" % (builder.cfg.source_url or "none configured (nothing links outside docs/)"),
    ]
    if verbose:
        for line in lines:
            print("ok — %s" % line)


def write_action(args: argparse.Namespace) -> int:
    cfg = config(args)
    files, problems, builder = build_all(cfg)
    if problems:
        for problem in problems:
            print(problem)
        print("\n%d problem(s); nothing written" % len(problems))
        return 1
    errors = write_site(files, args.out, args.clean)
    if errors:
        for error in errors:
            print(error)
        return 1
    report(files, builder, args.verbose)
    print("site OK: %d files written to %s" % (len(files), args.out))
    return 0


def check_action(args: argparse.Namespace) -> int:
    cfg = config(args)
    files, problems, builder = build_all(cfg)
    again = Builder(cfg)
    second = again.generate()
    if second != files:
        drift = sorted(name for name in set(second) | set(files) if second.get(name) != files.get(name))
        problems.append("the build is not deterministic: %s differ between two runs"
                        % ", ".join(drift[:5]))
    if problems:
        for problem in sorted(set(problems)):
            print(problem)
        print("\n%d problem(s)" % len(set(problems)))
        return 1
    report(files, builder, args.verbose)
    print("docs html OK: %d pages, %d files" % (len(builder.pages), len(files)))
    if builder.handwritten:
        print("note: %d hand-written .html under docs/ is not published (%s): the site's pages come from "
              "markdown"
              % (len(builder.handwritten),
                 ", ".join(builder.rel(path) for path in builder.handwritten[:4])))
    return 0


# ---------------------------------------------------------------------------
# the self-test
# ---------------------------------------------------------------------------
FIXTURE = {
    "docs/index.md": """
# Fathom fixture

Start at [the guide](guide/), read [the rules](guide/one.md#rules), or fetch the
[schema](../schemas/thing.json). Upstream: [the changelog](../CHANGELOG.md).

| a **diff** | a **delta** |
|:-----------|--------------:|
| bytes | a claim with `citations` and a `right` column |
| one \\| two | three |
| a short row |

- one
- two with **bold**, *em*, ~~gone~~, `<C-c>` and an [internal link](guide/one.md)
  - nested
  - nested too
1. first
2. second

> a quote
> that continues
>
> *with a list*

<!-- BEGIN GENERATED: marker -->
generated text
<!-- END GENERATED: marker -->

```bash
fathom check --help   # and <script>alert(1)</script> is data here
```

```text
unclosed? no: this one closed
```

A hard break  \nhere, an autolink <https://example.com/x>, and `<ignored>` as text.

A code span keeps its own markup as text: `[concepts](concepts/)` and `[not a link](one.md)` stay literal.
""",
    "docs/guide/README.md": """
# Guide

The guide's front page, reached by a directory link. See [one](one.md).
""",
    "docs/guide/one.md": """
# One

## Rules  ###

Nothing here but a heading and a paragraph, and a picture:
![the logo](../logo.png)
""",
    "docs/logo.png": "\x89PNG\r\n\x1a\n fake bytes for the fixture",
    "docs/guide/index.html": "<html><body>a hand-written page, not published</body></html>",
    "schemas/thing.json": "{}",
    "CHANGELOG.md": "# changelog\n",
}


def self_test() -> int:
    """Build a small repository twice: once to prove it passes, then once per planted defect.

    A gate nobody has seen fail is a rumour. Each case below is a way this builder could be wrong about
    the manual, made small enough to check in milliseconds and with no Rust, no network and no browser.
    """
    failures: list[str] = []
    import tempfile

    def expect(name: str, condition: bool, detail: str = "") -> None:
        if not condition:
            failures.append("self-test %s failed%s" % (name, (": " + detail) if detail else ""))

    with tempfile.TemporaryDirectory(prefix="docs-site-selftest-") as tmp:
        root = pathlib.Path(tmp) / "repo"
        for name, content in FIXTURE.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        cfg = Config(root=root, docs=root / "docs",
                     source_url="https://example.test/repo/blob/deadbeef",
                     repo_url="https://example.test/repo", base_url="https://example.test/site/")

        def build() -> tuple[dict[str, "str | bytes"], list[str], Builder]:
            return build_all(cfg)

        files, problems, builder = build()
        expect("clean fixture", problems == [], "; ".join(problems))
        expect("page per markdown file",
               {"index.html", "guide/index.html", "guide/one.html"} <= set(files), str(sorted(files)))
        expect("asset copied", "logo.png" in files)
        expect("a hand-written .html under docs/ is not published",
               "guide/index.html" in files and "<body>a hand-written page" not in files["guide/index.html"]
               and [path.name for path in builder.handwritten] == ["index.html"],
               "the guide's generated index was shadowed by a hand-written file")
        index = files["index.html"]
        expect("title", "<title>Fathom fixture · Fathom documentation</title>" in index, index[:200])
        expect("heading id follows gen_docs.anchor", 'id="a-guide"' in files["guide/one.html"]
               or 'id="a-guide" ' in files["guide/one.html"] or 'id="rules"' in files["guide/one.html"],
               "no id for `## Rules  ###`")
        expect("directory link became a page", 'href="guide/index.html"' in index)
        expect("relative md link became html", 'href="guide/one.html#rules"' in index)
        expect("outside link became a source link",
               'href="https://example.test/repo/blob/deadbeef/schemas/thing.json"' in index)
        expect("outside changelog", 'href="https://example.test/repo/blob/deadbeef/CHANGELOG.md"'
               in index)
        expect("asset link stayed inside", 'src="../logo.png"' in files["guide/one.html"])
        expect("raw html is escaped", "&lt;script&gt;" in index and "<script>" not in index)
        expect("generated marker comment survived",
               "<!-- BEGIN GENERATED: marker -->" in index)
        expect("fence kept its language", 'data-lang="bash"' in index)
        expect("table rendered with alignment", 'style="text-align: right"' in index)
        expect("escaped pipe in a cell", "one | two" in html_text(index))
        expect("a short row renders (GFM pads the missing cell)",
               "a short row" in html_text(index))
        expect("nested list", "<ul>" in index and "<li>nested</li>" in index.replace("\n", " "))
        expect("ordered list", "<ol>" in index)
        expect("autolink", '<a href="https://example.com/x">https://example.com/x</a>' in index)
        expect("hard break", "<br>" in index)
        expect("emphasis", "<strong>bold</strong>" in index and "<em>em</em>" in index
               and "<del>gone</del>" in index)
        expect("a code span keeps markup as text",
               "[concepts](concepts/)" in html_text(index)
               and "[not a link](one.md)" in html_text(index)
               and 'href="concepts/"' not in index)
        expect("search index has every page",
               files["assets/search.js"].count("\"url\"") == len(builder.pages))
        expect("404 exists and links home", "404.html" in files and 'href="index.html"'
               in files["404.html"])
        expect("the search box says what needs javascript",
               "noscript" in index and "search needs javascript" in index.lower())
        expect("the script is the search and the toc, nothing else",
               'event.key === "/"' in files["assets/docs.js"]
               and "search-results" in files["assets/docs.js"]
               and "fetch(" not in files["assets/docs.js"])
        expect("sitemap is absolute and complete",
               files["sitemap.xml"].count("<loc>https://example.test/site/") == len(builder.pages),
               files["sitemap.xml"][:120])
        expect("robots points at the sitemap",
               "Sitemap: https://example.test/site/sitemap.xml" in files["robots.txt"])
        bare = Config(root=root, docs=root / "docs",
                      source_url="https://example.test/repo/blob/deadbeef")
        bare_files, bare_problems, _bare = build_all(bare)
        expect("no sitemap and no robots without a known host",
               bare_problems == [] and "sitemap.xml" not in bare_files and "robots.txt" not in bare_files,
               "; ".join(bare_problems))
        expect("no absolute paths", not re.search(r'(?:href|src)="/', index))
        expect("canonical when a base url is known",
               'rel="canonical" href="https://example.test/site/index.html"' in index)
        expect("deterministic", Builder(cfg).generate() == files)
        expect("audit keeps every word", audit_skeletons(
            "the markdown has three words", "the page has three word") != "")
        expect("audit is quiet when nothing is missing",
               audit_skeletons("alpha beta   gamma!", "alpha  beta gamma") == "")

        def planted(name: str, filename: str, content: str, needle: str) -> None:
            """Rewrite one fixture file, rebuild, and insist the builder names the defect."""
            path = root / filename
            before = path.read_text(encoding="utf-8")
            path.write_text(content, encoding="utf-8")
            try:
                _, found, _ = build()
                expect("planted %s" % name, any(needle in problem for problem in found),
                       "expected %r in %s" % (needle, found))
            finally:
                path.write_text(before, encoding="utf-8")

        planted("dangling fragment", "docs/index.md",
                FIXTURE["docs/index.md"] + "\n[nowhere](guide/one.md#nope)\n", "fragment")
        planted("missing page", "docs/index.md",
                FIXTURE["docs/index.md"] + "\n[gone](guide/nothere.md)\n", "does not build")
        planted("missing asset", "docs/index.md",
                FIXTURE["docs/index.md"] + "\n[data](guide/nothere.json)\n", "not in docs/")
        planted("link out of the repository", "docs/guide/one.md",
                FIXTURE["docs/guide/one.md"] + "\n[out](../../../etc/passwd)\n", "outside the repository")
        planted("unclosed fence", "docs/guide/one.md", "# One\n\n```text\nnever closed\n",
                "unclosed code fence")
        planted("a fence wrapped around a generated block", "docs/guide/one.md",
                "# One\n\n```text\n<!-- BEGIN GENERATED: usage -->\nusage\n"
                "<!-- END GENERATED: usage -->\n```\n",
                "a generated block carries its own fence")
        planted("raw html", "docs/guide/one.md", "# One\n\n<details><summary>x</summary>y</details>\n",
                "raw HTML")
        planted("reference link", "docs/guide/one.md",
                "# One\n\nSee [the thing][thing].\n\n[thing]: one.md\n", "reference-style")
        planted("directory with no index", "docs/index.md",
                FIXTURE["docs/index.md"].replace("(guide/)", "(guide2/)|[x](guide2/)"), "")
        planted("table row too wide", "docs/guide/one.md",
                "# One\n\n| a | b |\n|---|---|\n| 1 | 2 | 3 |\n",
                "the render does not contain what the markdown contained")

        # Two cases need a tree of their own rather than a rewritten file.
        loose = root / "docs" / "media"
        loose.mkdir(exist_ok=True)
        (loose / "plain.png").write_bytes(b"\x89PNG fake")
        front = root / "docs" / "index.md"
        before = front.read_text(encoding="utf-8")
        front.write_text(before + "\n[nothing to read](media/)\n", encoding="utf-8")
        _, found, _ = build()
        expect("planted directory with no page", any("nothing in it" in problem for problem in found),
               str(found))
        # …and a directory that *does* hold pages but no index gets the generated listing, so a link at
        # the directory lands on a page instead of a 404.
        (root / "docs" / "extra").mkdir(exist_ok=True)
        (root / "docs" / "extra" / "page.md").write_text("# An extra page\n", encoding="utf-8")
        front.write_text(before + "\n[the extra section](extra/)\n", encoding="utf-8")
        good, found, builder = build()
        expect("a section with no index gets one",
               "extra/index.html" in good and not [p for p in found if "extra" in p], str(found))
        expect("the generated listing lists its pages",
               "An extra page" in html_text(good["extra/index.html"]))
        front.write_text(before, encoding="utf-8")
        (loose / "plain.png").unlink()
        loose.rmdir()
        (root / "docs" / "extra" / "page.md").unlink()
        (root / "docs" / "extra").rmdir()
        naked = Config(root=root, docs=root / "docs")
        _, found, _ = build_all(naked)
        expect("planted missing source url",
               any("no source URL" in problem for problem in found), str(found))

    for failure in failures:
        print(failure)
    if failures:
        print("\n%d self-test failure(s)" % len(failures))
        return 1
    print("build_docs_site self-test OK")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="build_docs_site.py",
                                     description=__doc__.splitlines()[0])
    parser.add_argument("action", nargs="?", choices=["write", "check"], default="check")
    parser.add_argument("--out", type=pathlib.Path, default=ROOT / DEFAULT_OUT,
                        help="where to write the site (default: %s/)" % DEFAULT_OUT)
    parser.add_argument("--clean", action="store_true",
                        help="remove a previously built site first (only a directory this tool wrote)")
    parser.add_argument("--root", type=pathlib.Path, default=ROOT, help="the repository root")
    parser.add_argument("--docs", type=pathlib.Path, default=DOCS, help="the markdown tree to publish")
    parser.add_argument("--source-url", default="",
                        help="blob base for links that leave docs/ (default: derived from git/CI)")
    parser.add_argument("--repo-url", default=None,
                        help="repository home for the header link (empty string to drop it)")
    parser.add_argument("--base-url", default=os.environ.get("FATHOM_DOCS_BASE_URL", ""),
                        help="deployed site root with its trailing slash; enables canonical and sitemap")
    parser.add_argument("--self-test", action="store_true",
                        help="run the builder's own cases (no build, milliseconds) and exit")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    args.docs = args.docs.resolve()
    args.out = args.out.resolve()
    if args.action == "write":
        return write_action(args)
    return check_action(args)


if __name__ == "__main__":
    sys.exit(main())
