# The dependency surface: what a revision declares, what it resolved, and what this build could not read

- **Status**: Accepted, and implemented
- **Ticket / plan key**: `reco-j83` (`e2.5`)
- **Implements**: the glossary entries [`Dependency surface`](thesis.md) and
  [`Delta`](thesis.md); ADR-002 §9 (team 5 owns `fathom-analyze/src/deps/`); ADR-004 §6.6 R2 and R7
  (a warm path whose cost is O(changed paths), never a tree re-enumeration); and the
  [path policy](paths.md) rule that an installed-copy directory is not source.
- **Executes as**: `crates/fathom-analyze/src/deps/` (`model`, `discover`, `scan`, `text`, `toml`,
  `yaml`, `xml`, and one module per ecosystem), tested by
  `tests/deps_golden.rs` (+ `tests/golden/deps/`), `tests/deps_tolerance.rs`,
  `tests/deps_monorepo.rs`, `tests/deps_repository.rs` and `tests/deps_bench.rs`.
- **Feature gates**: `deps-node`, `deps-cargo`, `deps-python`, `deps-go`, `deps-ruby`,
  `deps-elixir`, `deps-jvm`, `deps-dotnet`, aggregated by `deps-all`, which the shipped binary
  enables (see [ADR-002 §6.4](../adr/0002-workspace.md)).

Everything in epic 2 that touches a dependency — `e2.6`'s change impact, `e2.7`'s advisory
cross-check, the review pane's "this diff adds a dependency to a build-time code path" warning —
reads the structure this ticket produces. That is the reason a parse failure here is a *value* with
raw text attached rather than an error: an episode whose lockfile is unreadable still has to be
reviewable, and "we could not read it, here is the text" is a different product from "no row".

---

## 1. What this decides, and what it refuses to

Three questions per file: **is this path dependency surface**, **what does it declare or resolve**,
and **what could not be read**. Four rules shape the rest.

1. **A parse failure is data.** Every file that was recognised produces a row. A row whose
   `status` is `unparsed` carries a typed [`reason`](#5-when-a-file-cannot-be-read), the parser's
   own words, and a preview of the raw text. Nothing in this module can make an episode
   disappear; the count of files is how a reviewer notices something went wrong, so it never moves
   without a note explaining the move.
2. **Verbatim beats normalised.** A requirement is stored as the file spells it (`^18.2.0`,
   `~> 1.7`, `[13.0.3, )`, `workspace:*`), not as a normalised range. Translating
   poetry's `^2.31` into `>=2.31,<3.0` would be a claim about poetry's resolver that this module
   does not own, and a wrong translation is invisible while a verbatim string is checkable.
3. **The path decides before the bytes.** `node_modules/`, `vendor/`, `target/`, `dist/`, `build/`,
   `.venv/`, `site-packages/`, `bower_components/` and `.fathom/` are install or output directories:
   a `package.json` inside one is a copy of someone else's declaration. Those paths are refused
   before anything is read, and the refusal is counted and named in the scan's notes, so "the scan
   found 5 files" and "the scan found 5 files and skipped 3 under `node_modules/`" are different —
   and only the second is honest.
4. **A member, not a repository, is the unit.** `apps/web` and `apps/admin` declaring their own
   `react` is two requirements. Membership, lockfile ownership and workspace globs are modelled
   explicitly ([§4](#4-workspaces-and-monorepos)), because the alternative — one flat list — makes
   "a dependency was added" ambiguous by construction.

Refused deliberately: **no version resolution** (a lockfile is read, never recomputed), **no
evaluation of build DSLs** (a `Gemfile`'s `ruby \"3.3.0\"` or Gradle's `${spring.version}` is
recorded as text, not executed — running arbitrary build code is how a review tool becomes an
attack surface), **no network** (advisory lookups are `e2.7`'s, behind a `net-*` feature and a
consent key), and **no opinion about whether a dependency is *safe***.

## 2. The surface

```text
Surface ── files:   [DepFile]      one row per recognised path
         ── members: [Member]      one row per package directory
         ── totals:  Totals        counts, always self-consistent
         ── notes, scanned, complete

DepFile ── path, ecosystem, role, format, status, notes, body
Body    ── manifest { name, version, requirements[], workspaces[], properties }
         ── lock     { version, packages[], importers[], records_edges, complete, properties }
         ── workspace { members[] }                       (pnpm-workspace.yaml, go.work)
         ── unparsed  { reason, detail, preview, preview_truncated }
```

A `Dependency` (declared) is `{ name, requirement, kind, source, actual }`; a `LockPackage`
(resolved) is `{ name, version, source, integrity, license, edges[], dev, direct }`. Both carry a
`Source`, which is one of `registry`, `named_registry(name)`, `git{url, ref}`, `path`, `url` — the
field `e2.6` needs in order to call "registry → git" an event rather than a shrug, and the field
that makes an `npm:` alias (`lodash` installed as `lodash-es`) or cargo's `package = "…"` rename
survive into the review.

`records_edges` and `complete` on a lock exist to keep `e2.6` honest. A `requirements.txt` that
lists 400 pinned packages records no edges and is not complete on its own: it does not say which of
the 400 were declared. A `poetry.lock` records edges and is. Flattening that distinction is how a
tool ends up reporting 400 direct dependencies.

## 3. Twenty-five formats, eight ecosystems

| ecosystem | manifests | lockfiles |
|---|---|---|
| js | `package.json` | `package-lock.json` v1–v3, `yarn.lock` v1, `yarn.lock` Berry v8, `pnpm-lock.yaml` |
| cargo | `Cargo.toml` (package, workspace, targets, features) | `Cargo.lock` v1–v4 |
| python | `pyproject.toml` (PEP 621, Poetry, PEP 735 groups, uv), `requirements*.txt` | `poetry.lock`, `uv.lock`, `Pipfile.lock` |
| go | `go.mod` | `go.sum` |
| ruby | `Gemfile` | `Gemfile.lock` |
| elixir | `mix.exs` | `mix.lock` |
| jvm | `pom.xml`, `build.gradle(.kts)` | `gradle.lockfile` |
| dotnet | `*.csproj`, `Directory.Packages.props` | `packages.lock.json` |
| (config) | `pnpm-workspace.yaml`, `go.work` | — |

Each format knows one thing worth knowing about its ecosystem, and the parsers are written around
that fact rather than around a common grammar:

- **npm** puts the workspace root in `""` inside `packages`, and v1 locks nest one level per
  install depth. Two shapes, one answer: package, version, source, integrity, edges.
- **yarn v1** writes a `resolved` URL for *everything*. Telling a registry tarball
  (`registry.yarnpkg.com/react/-/react-18.2.0.tgz#1e`, where `#1e` is yarn's own sha1) from a git
  archive (`codeload.github.com/acme/ui/tar.gz/v3.1.0`) is done on host and shape. Splitting on
  `://` — the obvious first attempt — calls the first one a git source and invents a `git_ref` out
  of an integrity fragment.
- **pnpm** states each importer's requirements *and* the resolution: `importers:` is the
  monorepo answer, `packages:` is identity, `snapshots:` is the resolved edges. When both blocks
  name a dependency, the snapshot wins outright; concatenating them yields the same package twice
  with two versions.
- **cargo** writes a dependency three ways depending on author mood — `serde = "1"`,
  `serde = { version = "1", features = [...] }`, `[dependencies.serde]` — and all three must
  produce the same `Dependency`. Walking `toml_edit`'s item tree reads the inline form as "no
  version given", which is a silent wrong answer on the most common file in the ecosystem, so
  [`toml`](../../crates/fathom-analyze/src/deps/toml.rs) flattens the document into one map shape
  first.
- **python** is four generations wearing one filename. Precedence follows the tools: `[project]`
  names the package, Poetry names it only if `[project]` did not, and `[tool.uv.sources]` rewrites
  *where* a name comes from without adding a dependency of its own.
- **go** separates declaration (`go.mod`) from verification (`go.sum`), so the lock's "packages" are
  module-plus-`/go.mod` rows and the integrity is a `h1:` digest — and `go.mod`'s `replace`,
  `exclude` and `tool` lines are part of the surface, because a `replace` to `./forks/pq` is exactly
  the kind of thing a reviewer should be told about.
- **ruby**'s `Gemfile` is Ruby. It is read as a line-oriented DSL — quoted name, following quoted
  strings are constraints, keyword options are `git:`/`branch:`/`path:`/`platforms:` — and anything
  that needs a real interpreter (``dir.glob("*.gemspec")``) becomes a note, not a guess.
  `Gemfile.lock` is a section machine, and its `DEPENDENCIES` block is the only place a Ruby lock
  says which packages are direct.
- **elixir**'s `mix.lock` is a keyword map: `"ecto": {:hex, :ecto, "3.11.2", chk, [:mix],
  [deps…], "hexpm", inner-chk}`. Field 5 is the dependency list and field 6 is the repository;
  reading one lower prints the whole dependency list as a registry name (it did).
- **jvm/dotnet** use XML, read with a small tolerant reader: a POM cut inside a start tag yields the
  elements before the cut plus a note. Maven's `dependencyManagement` is recorded as
  `other:managed`, not as a runtime dependency — a version pin is not something code compiles
  against. Gradle is matched per declaration (`implementation`, `testImplementation`,
  `compileOnly`, `classpath`, …), including `platform("g:a:v")` BOM imports and the
  `group:/name:/version:` map form; `${...}` properties stay verbatim with a note.
  `packages.lock.json` is the one lock format in the table that states directly whether a package
  is a direct dependency, which is what `LockPackage::direct` exists to carry.

## 4. Workspaces and monorepos

`scan` returns `members`, one per directory that holds a manifest — plus one per directory that
holds a *lock* with no manifest, because a committed lockfile with no manifest is still proof that
something installs, and it is named in the notes.

- `manifest` and `locks[]` bind a lockfile to the package it locks, by ecosystem and nearest
  directory: `apps/web/package-lock.json` belongs to `apps/web`, not to the repository.
- `direct` and `resolved` are per member. Two members declaring `react` is two requirements, and
  `totals` sums them, which is what makes a folder-level view (`e2.3`) derivable rather than
  re-derived.
- `claimed` records whether some workspace glob — `workspaces` in a `package.json`,
  `pnpm-workspace.yaml`, `[workspace] members`, `go.work use(...)`, `[tool.uv] workspace` — covers
  the member. A member no glob claims is reported in the notes: it is the classic half-migrated
  monorepo, and the person who added the directory is the last to know. Leading `./` is decoration
  and is stripped before matching, because `go.work` writes `./api`.
- Config files (`pnpm-workspace.yaml`, `go.work`) declare membership and no dependencies, so they
  are `configs` in the totals, not manifests.

## 5. When a file cannot be read

`UnparsedReason` is eight values, each with a one-line piece of advice that is printed next to the
raw diff — the half of the ticket's degradation rule that makes the difference between an alert and
a shrug:

| reason | when | what the advice says |
|---|---|---|
| `malformed_syntax` | the bytes are wrong somewhere other than the end | where the parser stopped |
| `truncated` | the bytes stop mid-token, or a read cap cut them | raise the cap, or the file really is cut |
| `too_large` | over the cap, so not read at all | raise the cap |
| `not_text` | a NUL in the head, or undecodable | it is a binary; the preview is a `·`-sanitised caption so the *kind* is legible |
| `feature_absent` | the ecosystem is not compiled into this binary | which `deps-*` feature would read it |
| `empty` | zero bytes | compare it against the previous revision — an emptied lockfile looks like an absent one |
| `unknown_shape` | recognised name, unrecognised contents | the format in a shape this build has not been taught |
| `content_unavailable` | the caller could not read the blob | whose failure that is |

Two kinds of note ride along, and the difference matters: a **note** is a deficiency (a line
skipped, a version guessed) and makes the file `partial`; an **observation** was read perfectly well
and is merely worth repeating (a yarn entry answering two specs, a package locked under a marker, a
virtual workspace manifest). Without the second kind, every informative note is a false alarm —
a healthy `yarn.lock` would be `partial` for being normal.

`Options` bounds the work: `max_blob_bytes` (8 MiB — larger than any lockfile measured, and going
over it yields `truncated`, never a partial parse presented as a whole one), `max_files` (a
pathological tree cannot make a scan unbounded; hitting the cap sets `complete = false` and says
so), `preview_bytes`, and `keep_bodies` (false gives counts and statuses only, which is what a
listing needs before a row is opened).

## 6. Diffs

`delta` is the per-path answer: one row per changed dependency file with both sides parsed, a
one-line summary in counts, and `show_raw` — true when either side failed to parse, which is the
machine-readable instruction "print the raw diff under this". `delta_repository` reaches it through
`fathom-ingest`'s changed-path walk: one blob read per changed dependency file, and a commit that
touches forty `.rs` files produces no rows and costs nothing. That is not a micro-optimisation; it
is ADR-004 §6.6 R2, and the test that pins it is
`a_source_only_commit_produces_no_dependency_rows`.

## 7. Features

One `deps-*` feature per ecosystem, and `deps-all` for all eight; `fathom-cli` enables `deps-all` by
default because a review tool that silently skipped the one ecosystem a repository uses is worse
than one that admits it. A build with any subset — including none — compiles warning-free: the
shared helpers that a given subset leaves unused are exempt from `dead_code` *only* in that
configuration, so the full build (the one CI lints and users ship) still catches a helper nothing
calls. A gated-out ecosystem answers `feature_absent` rather than failing the scan, so the same
binary can describe a repository it cannot fully read.

## 8. The budget

The ticket's number is 500 ms for a full-repo monorepo scan, measured as the whole surface — every
ecosystem's manifests and locks, parsed and assembled — on an optimised build. `tests/deps_bench.rs`
builds an ~8 MiB synthetic monorepo in memory (24 npm members with 1 200-package locks, a
2 500-package `Cargo.lock`, a pnpm lock with 20 importers and 1 500 packages, a 600-package
`poetry.lock`, a 900-line `go.sum`, an 800-spec `Gemfile.lock`, a 400-entry `mix.lock`, JVM and
.NET projects, plus 200 ignored `node_modules` entries so the timing includes the paths that must
*not* be paid for) and asserts against it: **~90 ms in release**, printed on every run. `cargo test`
runs debug, which is ~6× slower on the same corpus, so the assertion scales by a documented factor
and the number that matters is the one printed. A second test asserts the shape behind the number:
8× the lockfile must not cost 8× the time squared.

## 9. What is deliberately not parsed

Named, because a gap you can list is a gap you can close:

- Gradle and Maven **property interpolation** and any build logic: `${spring.version}` is recorded
  as written, with a note. `buildSrc` and included builds are not followed.
- **`Gemfile` Ruby evaluation**: `ruby "3.3.0"` is recorded as a property, but a `Gemfile` that
  builds its dependency list programmatically yields what it can be read as, plus notes.
- **pnpm integrity digests**: pnpm keeps its content addresses in the store, not the lockfile, so
  `integrity` is `None` and `e2.6` must not infer "unchanged" from it.
- **Maven parent resolution** across files: the parent is recorded as `group:artifact:version` and
  its dependencies are not inherited.
- **Transitive closure**: nothing resolves. A lockfile that records no edges stays a flat list,
  marked as such.
- `Gemfile.lock`'s `CHECKSUMS`, NuGet's `*.nupkg.metadata`, `node_modules/.package-lock.json`,
  `Pipfile` (as opposed to `Pipfile.lock`), `setup.py`/`setup.cfg`, `build.gradle` **subprojects**
  declared outside the file system, and Elixir's umbrella-only metadata.

## 10. Where the tests are

| file | what it pins |
|---|---|
| `tests/deps_golden.rs` + `tests/golden/deps/` | 28 hand-readable fixtures, one per format, compared through a projection (not a struct dump) so a change is reviewable; the test fails if any claimed `Format` has no fixture |
| `tests/deps_tolerance.rs` | every fixture × 12 degradations (348 scans): no panic, exactly one row each, typed reason and non-empty preview when there were bytes, prefix-of-the-file previews, mixed surfaces where one bad file costs nothing else |
| `tests/deps_monorepo.rs` | membership, lock ownership, globs and `claimed`, ignored directories named, one lock with no manifest, 50-member trees |
| `tests/deps_repository.rs` | real `git`: whole-tree scan of a revision, added/removed/modified/corrupted cases through `delta_repository`, a source-only commit producing no rows |
| `tests/deps_bench.rs` | the 500 ms budget and its scaling shape |
