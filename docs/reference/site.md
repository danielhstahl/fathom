# Publishing the docs as HTML

`docs/` is the manual, and it is read in two places: on the source host, where a `.md` file renders in
place, and on GitHub Pages, where the same tree becomes a static site at
<https://danielhstahl.github.io/fathom/>. Both readings have to agree, so the site is not a second copy of
the documentation and no file in `docs/` is generated: the markdown is the source of truth and the HTML is
a build artefact, produced by one script and never committed.

## Why a script in `tools/` and not mdBook

`docs/index.md` states the rule behind this documentation: **"a document that is copied by hand is a
document that lies the day the code moves"**, which is why anything that already exists as data is generated
and every gate here has a way to prove it can fail. An off-the-shelf docs generator is a hand-copy with a
nicer face: it wants a `SUMMARY.md` that duplicates the tree, a config file of its own, and it re-decides
what a table or a `<details>` block means. And this markdown carries obligations such a tool would break:
the `<!-- BEGIN GENERATED: usage -->` markers whose blocks `gen_docs.py` rewrites in place (including the
fences around them — see below), the `.cast` recordings and their sidecars that have to stay next to the
page that links them, and the links that cite evidence outside `docs/`, at a depth the site root cannot see.

So `tools/build_docs_site.py` is one script of about eighteen hundred lines of standard library, with two
deliberate limits:

* **No dependencies.** The same command runs on a laptop, in CI and on a runner with no network. Every
  other docs gate in this repository is Python 3.11 and nothing else, and a build step that needs a crate
  registry is a build step that fails on the machine that has no registry.
* **A subset, checked, not assumed.** The renderer implements the markup the tree actually uses — headings,
  emphasis, inline code, links, images, lists, tables, blockquotes, rules, fenced code, HTML comments —
  and it refuses anything outside that: raw HTML, reference-style link definitions, a heading with no blank
  line above it. Silently dropping a construct is how a manual loses a sentence in the render, so an unknown
  line is a build problem and a `<script>` in a page is escaped *and* reported rather than believed.

## What the build produces

```bash
python3 tools/build_docs_site.py write --out .docs-site   # build (gitignored)
python3 tools/build_docs_site.py check                    # build in memory, assert, write nothing
python3 tools/build_docs_site.py check --verbose           # one line per rule group
python3 tools/build_docs_site.py --self-test               # prove the gate can fail
```

For every `docs/**.md` it writes a page of the same name with `.html`; `index.md` and a directory's
`README.md` become that directory's `index.html`. A directory with neither gets a generated listing, which
is what makes a link at `concepts/` land on a page instead of a 404, on GitHub and on Pages alike. It also
writes:

| | |
|---|---|
| `assets/docs.css` | the stylesheet: layout, dark mode via `prefers-color-scheme`, print rules |
| `assets/docs.js` | the search box (`/` focuses it, arrow keys move the selection, the index is lazy-loaded on the first keystroke) and the table of contents' current heading |
| `assets/search.js` | the search index, one entry per page: `url`, `title`, `section`, `headings`, `text` |
| `404.html` | what GitHub Pages serves for an address that is not a page: the same shell, the search box, and a pointer at the front door |
| `sitemap.xml`, `robots.txt` | written only when `--base-url` is given — a sitemap of relative paths is not an address a crawler can use |
| every non-markdown file under `docs/` | copied verbatim, which is why `docs/demo/*.cast` plays on Pages. A hand-written `.html` inside `docs/` is the exception, and is reported rather than shipped: it would claim a path a generated page already owns |

Navigation is generated from the tree: a sidebar of the sections, the current page expanded, a per-page
table of contents in the right column, and breadcrumbs above the title. The sidebar is real links, so
right-click-and-open-middle-click work, and the site stays usable with JavaScript off: navigation, the table
of contents and every link are in the HTML, and the page says so where the search box is.

## Where links go

`docs/` lives inside a repository, and most of its links are about the repository:

* a link ending in `.md` becomes `.html`;
* a link to a directory becomes that directory's page or generated listing;
* a fragment is checked against the heading ids the builder assigned — the same ids
  `gen_docs.anchor` produces, which is the rule `tools/check_docs_site.py` already enforced in the
  markdown, so `page.md#setting-up` and `page.html#setting-up` cannot disagree;
* a link that leaves `docs/` — `../../schemas/episode.schema.json`, `../../bench/runner.py`,
  `../../AGENTS.md`, 197 of them in all — is rewritten to an absolute URL on the source host, because a
  directory above the site root does not exist on Pages. The base is derived from `GITHUB_REPOSITORY` plus
  `GITHUB_REF_NAME`/`GITHUB_SHA` in CI, and from `git remote origin` plus the current branch locally; pass
  `--source-url` to pin it. Nothing in the site is an absolute path: Pages serves a project site under
  `/<repo>/`, and an absolute path there is a 404 with a green build.

## What the gate asserts

`tools/check_docs_site.py` builds the site in memory and fails on: an unsafe redefinition of a page path,
an unclosed fence, a construct outside the subset, a link to a page or a fragment that does not exist,
markup left unescaped, an unbalanced tag, and — the one that catches everything else — a page whose
rendered characters are not the characters its markdown contained.

That last rule is what found the defects this build had to fix, all of which rendered wrong on GitHub too:

* the `non_code.env.secret_name_pattern` row in [config.md](../concepts/config.md) and in
  [noncode-surfaces.md](../concepts/noncode-surfaces.md) held a regular expression with unescaped `|`
  inside a code span, which cut the row out of the table and, in `config.md`, cut the header row too;
* [rename-pairing.md](../concepts/rename-pairing.md) had two tables pasted into the middle of other
  sentences — one of them a stale duplicate of a generated gate table — leaving a sentence that began
  "three rows would | class | claimable pairs |" and a verb row that read "every recorded decis| gate |";
* two stray fence lines in [cli.md](cli.md) closed on the generated block's own fence, which dropped
  `## The verbs` and `## Global flags` out of the page;
* five pages ([ADR-006](../adr/0006-tui.md), [timeline](../concepts/timeline.md),
  [the headless verbs](cli.md), [the API surface](../concepts/api-surface.md),
  [the demo](../demo/README.md)) wrote a code span containing backticks as backslash-backtick, which is
  not how a code span carries a backtick: the reader saw the backslashes. Those are double-backtick spans
  now.

None of that was visible from the markdown, and the words were still in the file; the render had dropped
them. The rule now fails the build instead.

The same rules run in five places, so a docs page cannot be broken in one and fine in another:

| | |
|---|---|
| `python3 tools/check_docs_site.py` | the documentation gate; `contracts.yml` runs it on every PR |
| `python3 tools/build_docs_site.py check` | the HTML half on its own, in memory |
| `tests/test_docs_site_contract.py` | the documentation gate as a pytest assertion |
| `tests/test_docs_html_contract.py` | the built site: completeness, links, escaping, determinism |
| `.github/workflows/docs.yml` | the build, the preview artefact on a PR, and the publish on `master` |

## Turning Pages on

`docs.yml` cannot enable Pages by itself, and it will not try. In the repository's settings, under
**Pages → Build and deployment**, choose **Source: GitHub Actions**. The first run after that publishes;
every push to `master` publishes again, and every pull request gets a `docs-site-preview` artefact that is
the same directory, unzippable and openable from `index.html`.

A page on a fresh site may say "That page is not here" for a link that is fine —
the published build is the one that is live, not the tree you are reading. To read the render before
merging, unzip the artefact, or run `write --out` locally and open `.docs-site/index.html`. The build is
deterministic, so two runs of the same commit agree byte for byte and a published artefact can be traced
back to a commit.
