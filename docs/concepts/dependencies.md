# Dependencies: what a revision declares, what an episode changed, and whether to be worried

- **Status**: shipped, three pages' worth of it, and in one page. A reviewer asking "how do dependencies
  work" was sent to three pages that each answered one slice of the same crate — what a revision
  declares, what an episode changed, whether to be worried — and none of which said why there were
  three. `reco-48qd` merged them under [the documentation merge map](../reference/doc-map.md)'s M6.
  The three retired paths are signposts now, and nothing was dropped: every claim, table row, refusal,
  measured number and rendered sample from all three is here, each in exactly one place.
- **Merged from**: `dependency-surface.md` (`reco-j83`, `e2.5` — the surface), `dependency-delta.md`
  (`reco-kvs`, `e2.6` — the delta) and `dependency-risk.md` (`reco-yxi`, `e2.7` — the risk). One crate
  is the reason they are one page: `fathom-analyze/src/deps/` (team 5), with the risk answer's
  executable half in `src/recommender/deprisk/`.
- **Implements, the surface half**: the glossary entries [`Dependency surface`](thesis.md) and
  [`Delta`](thesis.md); ADR-002 §9 (team 5 owns `fathom-analyze/src/deps/`); ADR-004 §6.6 R2 and R7
  (a warm path whose cost is O(changed paths), never a tree re-enumeration); and the
  [path policy](paths.md) rule that an installed-copy directory is not source.
- **Executes as, the surface half**: `crates/fathom-analyze/src/deps/` (`model`, `discover`, `scan`,
  `text`, `toml`, `yaml`, `xml`, and one module per ecosystem), tested by `tests/deps_golden.rs`
  (+ `tests/golden/deps/`), `tests/deps_tolerance.rs`, `tests/deps_monorepo.rs`,
  `tests/deps_repository.rs` and `tests/deps_bench.rs`.
- **Implements, the risk half**: the plan's `e2.7` acceptance criteria — provenance for a new
  dependency, optional OSV advisory lookup behind explicit network consent, and licence-policy checks
  configured from `fathom.toml`; ADR-002 §4.5 (the analysis half reads no filesystem, clock,
  environment or socket); ADR-001 §7.5 (consent a cloned repository can grant is not consent);
  ADR-004 §5.5 (the answer is cached behind a digest over every key it read); ADR-005 §2.2 (hooks v1
  stores `prompt_sha256` and `prompt_chars`, not the prompt); *the delta* section below, which owns the
  shape of a dependency change, and [summary-card.md](summary-card.md), which owns where this card is
  printed.
- **Executes as, the risk half**: `src/recommender/deprisk/` (`model.py`, `policy.py`, `osv.py`,
  `licence.py`, `provenance.py`, `delta.py`, `render.py`, `explain.py`, `corpus.py`, `eval.py`,
  `cli.py`), `tests/test_deprisk_contract.py`, and `python3 -m recommender.deprisk …`. The Rust home
  is `fathom-analyze::deps` (team 5), with the card in `fathom-cli::verbs::deps`.
- **Feature gates**: `deps-node`, `deps-cargo`, `deps-python`, `deps-go`, `deps-ruby`, `deps-elixir`,
  `deps-jvm`, `deps-dotnet`, aggregated by `deps-all`, which the shipped binary enables (see
  [ADR-002 §6.4](../adr/0002-workspace.md)); and `net-advisories`, the only gate under which an
  advisory byte can leave the machine, which still needs `privacy.advisories` — see
  [the risk](#the-risk-who-asked-for-this-and-should-i-be-worried).

Three questions about one crate, and they are asked in order: **what does this revision declare and
resolve** (the surface), **what did this episode change** (the delta), **who asked for it, and should I
be worried** (the risk). Each is the input to the next — the delta diffs two surfaces, the risk answer
reads the delta's rows — which is why one page reads better than three that each re-introduced the
neighbour.

## The surface: what a revision declares, what it resolved, and what this build could not read

Decided by `reco-j83` (`e2.5`), the first half of epic 2's dependency work.

Everything in epic 2 that touches a dependency — `e2.6`'s change impact, `e2.7`'s advisory
cross-check, the review pane's "this diff adds a dependency to a build-time code path" warning —
reads the structure this ticket produces. That is the reason a parse failure here is a *value* with
raw text attached rather than an error: an episode whose lockfile is unreadable still has to be
reviewable, and "we could not read it, here is the text" is a different product from "no row".

### What the surface decides, and what it refuses to

Three questions per file: **is this path dependency surface**, **what does it declare or resolve**,
and **what could not be read**. Four rules shape the rest.

1. **A parse failure is data.** Every file that was recognised produces a row. A row whose
   `status` is `unparsed` carries a typed [`reason`](#when-a-file-cannot-be-read), the parser's
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
   explicitly ([Workspaces and monorepos](#workspaces-and-monorepos)), because the alternative — one flat list — makes
   "a dependency was added" ambiguous by construction.

Refused deliberately: **no version resolution** (a lockfile is read, never recomputed), **no
evaluation of build DSLs** (a `Gemfile`'s `ruby \"3.3.0\"` or Gradle's `${spring.version}` is
recorded as text, not executed — running arbitrary build code is how a review tool becomes an
attack surface), **no network** (advisory lookups are `e2.7`'s, behind a `net-*` feature and a
consent key), and **no opinion about whether a dependency is *safe***.

### The shape of the surface

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

### Twenty-five formats, eight ecosystems

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

### Workspaces and monorepos

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

### When a file cannot be read

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

### Diffs

`delta` is the per-path answer: one row per changed dependency file with both sides parsed, a
one-line summary in counts, and `show_raw` — true when either side failed to parse, which is the
machine-readable instruction "print the raw diff under this". `delta_repository` reaches it through
`fathom-ingest`'s changed-path walk: one blob read per changed dependency file, and a commit that
touches forty `.rs` files produces no rows and costs nothing. That is not a micro-optimisation; it
is ADR-004 §6.6 R2, and the test that pins it is
`a_source_only_commit_produces_no_dependency_rows`.

### Features

One `deps-*` feature per ecosystem, and `deps-all` for all eight; `fathom-cli` enables `deps-all` by
default because a review tool that silently skipped the one ecosystem a repository uses is worse
than one that admits it. A build with any subset — including none — compiles warning-free: the
shared helpers that a given subset leaves unused are exempt from `dead_code` *only* in that
configuration, so the full build (the one CI lints and users ship) still catches a helper nothing
calls. A gated-out ecosystem answers `feature_absent` rather than failing the scan, so the same
binary can describe a repository it cannot fully read.

### The budget

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

### What is deliberately not parsed

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

### Where the surface's tests are

| file | what it pins |
|---|---|
| `tests/deps_golden.rs` + `tests/golden/deps/` | 28 hand-readable fixtures, one per format, compared through a projection (not a struct dump) so a change is reviewable; the test fails if any claimed `Format` has no fixture |
| `tests/deps_tolerance.rs` | every fixture × 12 degradations (348 scans): no panic, exactly one row each, typed reason and non-empty preview when there were bytes, prefix-of-the-file previews, mixed surfaces where one bad file costs nothing else |
| `tests/deps_monorepo.rs` | membership, lock ownership, globs and `claimed`, ignored directories named, one lock with no manifest, 50-member trees |
| `tests/deps_repository.rs` | real `git`: whole-tree scan of a revision, added/removed/modified/corrupted cases through `delta_repository`, a source-only commit producing no rows |
| `tests/deps_bench.rs` | the 500 ms budget and its scaling shape |

## The delta: what the episode did to the graph, and what proves it

Decided by `reco-kvs` (`e2.6`), on top of the surface above.

The delta half of `fathom_analyze::deps` answers the question an episode of dependency change actually poses:
**what is installed now that was not installed before, and what made it so.** It diffs two *resolved
graphs*, not two lockfile texts, and every row it produces carries the evidence for its own claim.

It is a consumer of [the surface](#the-surface-what-a-revision-declares-what-it-resolved-and-what-this-build-could-not-read)
above. The surface is an event-invariant projection — what this revision declares and resolves; the delta is
what the episode did to it. The previous half (`e2.5`) built the before-and-after `Delta` of dependency
files; this half reads those two revisions as graphs and compares them.

This document answers *what moved and what proves it*. It deliberately does not answer the two questions a
reviewer of the same table asks next — *who asked for this dependency*, and *should I be worried* — which
are [`e2.7`'s](#the-risk-who-asked-for-this-and-should-i-be-worried) below: provenance, advisories and
licence policy read this delta's rows and own those answers.

### The model

```rust,ignore
let episode = deps::dependency_episode_repository(&repo, Some(old_tree), Some(new_tree), &Default::default())?;
println!("{}", deps::render_table(&episode, &Default::default()));
```

- **`DependencyState`** — one revision as a resolved graph plus the declarations that requested it.
  Built from either side of the episode: `State::of(&Surface)` for a cold scan, `State::of_delta(&Delta)`
  for the warm path, where only the changed files are in hand.
- **`DependencyDelta`** — the change between two states: one `DepChange` per package per
  thing that happened to it, with counts by attribution, graph-size movement per ecosystem, source and
  licence set deltas, peer conflicts, a pattern label, a sentence, and notes.
- **`FileDiff` / `Hunk`** — the line-level diff of each changed dependency file, and `locate_hunk`, which
  finds, for one claim, the hunk that proves it.
- **`render_table`** — the dependency table for an episode, with its drill-down.

### Direct and transitive are properties of the change

A package is direct when *this repository asks for it*: a manifest declares it, or the lockfile says so
(Gemfile.lock's `DEPENDENCIES`, NuGet's `"type": "Direct"`, npm's importer block). It is transitive when
another resolved package asks for it. Attribution is recorded per change with the evidence it came from —
`lockfile` statement, manifest declaration, resolved edge — because the strength differs and the reader is
entitled to know which they got.

Three rules that exist because the naive version is wrong:

- **A manifest can contradict its own lockfile.** The lockfile is what resolved; the manifest is intent.
  Where both speak, the lockfile wins and the contradiction is a note.
- **A `// indirect` requirement is not a direct dependency.** Go writes indirect requires into `go.mod`,
  which is the manifest; the comment is the only thing distinguishing them, so it is read.
- **Where the graph is not recorded, nothing is called transitive.** `go.sum` is a hash list with no edges,
  and a Gradle lockfile resolves one configuration per line. A package there that no manifest declares is
  `unknown`, with a note naming the file and the reason — an unprovable claim is worse than an absent one.

### Why the graph, and not the lockfile diff

A lockfile diff tells you every line that moved. That is the wrong unit:

- **A mass transitive refresh moves a hundred lines and means one thing** — the lockfile was refreshed.
  A `dependency_bump_storm` is a *pattern*, decided by whether the moves have a direct cause, not by how
  many lines changed. The test that matters: one new direct dependency plus twelve refreshed transitive
  packages the new dependency does not explain is reported as *both*, because they are two facts and
  collapsing them is how a reviewer misses the second one.
- **A version can change without a line changing.** Re-resolving a range may pick a different version while
  the version *string* stays put; the graph comparison also catches integrity, source and licence changes
  for a package whose version never moved.
- **Attribution needs edges.** "Who asked for this" is a question about the graph. The answer is a path —
  `foo → bar → baz` — and it is stored on the row.
- **Two lockfiles can express one graph.** Format churn — lockfileVersion 2 to 3, a reordered dependency
  list — is a large text diff and a zero graph delta.

### Provenance, licences, conflicts

Version changes are one family: **upgrade**, **downgrade**, and **version changed** for the pair that cannot
be ordered (compare with the version grammar's own limits — an unorderable pair is stated, never guessed).
**Source switches** are separate: a package that stopped coming from the registry and started coming from
git is a supply-chain event, not a version change, and the source *identity* deliberately ignores git refs
and commit hashes — moving a commit is a ref move, while naming a new URL, registry or path is a **new
source**, reported on its own line. Integrity changes for an unchanged version are their own row: same
version, different bytes.

Licences are compared as sets, because the interesting fact is *what the repository now ships* — the sum of
the licences newly seen and vanished, with the package that brought each one.

A **peer conflict** is claimed only where the graph contradicts a declared range: something requires
`react >=18` and the resolution contains `react 17.0.1`. An unsatisfiable range that cannot be parsed is
never a conflict; that is the version module's rule, and the delta inherits it.

### The warm path, and the honesty it forces

The episode path has both revisions' changed files and nothing else, which is enough to diff graphs and not
always enough to attribute them. When the changed set holds a lockfile but not the manifest that declares
one of its packages, that package is `unknown` and a note says so. A table that says "3 unattributed, see
notes" is worth more than one that guesses, because the guess is what a reviewer would have to re-check.

### Reading the table

```text
dependency delta — Net +2 packages from 1 new direct dependency (foo 2.1.0); plus a mass refresh of 12 transitive packages nobody asked for; licence set +1.
pattern         mixed (bump storm)
graph           js 13 → 15 (+2)
direct          +1 upgraded 0 downgraded 0 -0
  direct        + foo 2.1.0                js
transitive      +1 upgraded 12 downgraded 0 -0 (refresh of 12)
  explained     1 explained by a direct change, 12 without one
  …
drill-down

  transitive · added bar 1.0.0 · js · package-lock.json
    attribution: transitive — another resolved package asks for it
    asked for by: foo
    @@ -31,76 +32,91 @@
    +    "node_modules/foo": {
    +      "version": "2.1.0",
```

Every printed row has a hunk or says why it has none. The excerpt is cut round the line that proves the
row, not the top of the hunk: in a mass refresh the two are usually a hundred lines apart.

### Where it reaches the product

Three reads, one measurement each, and each one says which of them it is:

| read | entry point | what the pair of trees is |
|---|---|---|
| a cold scan of a revision | `State::of` on a `Surface` | one revision, no delta |
| an episode, from the index | `Supplied::for_episode_with_repo` (`e2.11`'s seam) | the episode's first member's parent and its last member |
| a range someone typed | `fathom check --only dependencies --base A --head B` | the two refs, as written |

The episode read is the one that closed Epic 2's exit criterion — *every episode answers what it now depends
on, with every number traceable* — and it carries three rules that the CLI cannot invent for itself:

- **An empty answer is a measurement.** An episode that touched no manifest produces a measured section
  whose graph was not read, not a `0 → 0` line implying a graph was compared. The section says the file pair
  held nothing dependency-shaped, which is a fact about the episode; `Unavailable` stays reserved for a read
  with no object database, which is a fact about the reader.
- **Every figure carries the command that reproduces it.** The direct count, the graph size, the sources and
  the pattern all point at `fathom check --only dependencies --base <from> --head <to>`, which is the same
  analysis on the same two commits. A reviewer who does not believe a number runs the number.
- **The dependency answer is not added to an episode's churn.** The delta document reports dependency
  movement as its own section; the churn in `what changed` counts it as ordinary file churn with the
  ecosystem named, because a lockfile's lines are churn and pretending otherwise moves a number the reviewer
  is holding.

The roll-up on `fathom episodes` deliberately does *not* carry this answer: it would open one object pair per
row to print one line each, on a hot path with a stated budget (ADR-004 §11).

### What the graph delta will not claim

- The graph is only as good as the parser that read it. Lockfiles that record no edges cannot support
  transitive claims, and say so.
- Version comparison is an ordering over what a human could read. Unknown grammars, wildcards and
  non-numeric versions produce "cannot order", not a guess.
- Resolved-graph size counts resolved entries per ecosystem, so a lockfile that lists one package many
  times over-counts, and a manifest-only member under-counts. It is exact only when the file says it is.
- The peer conflict check is the declared range against the resolved version — real resolution backtracking,
  which npm and pnpm do at install time, is out of scope.

## The risk: who asked for this, and should I be worried?

Decided by `reco-yxi` (`e2.7`), reading the delta above.

### What the risk answer decides

Two questions per episode, and the second one is worthless without the first:

> **Who asked for this dependency — and should I be worried?**

`e2.6` answers what moved in a dependency set: added, removed, upgraded, source switched, licence
changed, with an edge path on every row. That is the *arithmetic*. It is not the review question.
The review question is asked by a person looking at `+ fastparse 2.2.0` in an episode titled "speed
up the parser", and it has two halves with completely different failure modes:

* **provenance** fails by being blank. A reviewer who gets no answer reads that as "nobody
  asked", which is a worse outcome than a tool that said "not recorded, and here is what would
  fill it". Silence is not neutral here; it is a false accusation by omission.
* **risk** fails by being quiet. "No advisories found" about a cache nobody refreshed in seven weeks
  is the most dangerous sentence this tool could ever print, and it is a sentence that *sounds*
  like good news.

So the module's rule for both halves is the same one the siblings use: **an unknown is an answer
with a remedy attached, never an empty field.**

| Question | Answer | Where |
| --- | --- | --- |
| Who asked for this new direct dependency? | the episode under review, else a recorded introduction, else `not recorded` plus the two things that would fill it | `provenance.py` |
| What was the intent behind it? | quoted text with its source and confidence, or which half is missing (`not captured`, `digest only`, `not used`, `guess`) | `provenance.py` |
| Does an advisory apply to this package at this version? | from the local OSV cache, always with its age and its coverage | `osv.py` |
| Does this repository's licence policy allow this licence *here*? | a matrix: copyleft class × (library layer / everywhere else), with the edge path that asked for it | `licence.py`, `policy.py` |
| Should I be worried? | a finding table with severity, anchor, evidence and edge path, plus one deterministic sentence | `delta.py`, `render.py` |

### Who asked for this

The provenance answer is a **ladder**, and it stops at the first rung that has an opinion:

1. **this episode** — the package is new in this episode's delta, so this episode introduced it.
   This is the common case, needs no index, and is the rung most questions are answered on.
2. **a recorded introduction** — an earlier episode's record says it added this package
   (`deps.provenance.search_previous_episodes`, on by default, because the question is usually asked
   weeks after the fact). A record for the same name at a *different* version still answers and
   says so in its evidence, because "who asked for `jwt`" is a question about the name.
3. **not recorded** — printed as the answer, with the remedy: `review the episode that added it, or
   index from farther back`. Never a blank line.

The intent half is a separate verdict, because knowing the episode is not knowing the reason:

| Intent evidence | Stated as |
| --- | --- |
| `Agent-Intent:` trailer | the answer, quoted, with source and confidence — the path that works today |
| harness turn prompt text | the answer, only if intent capture (`e4.2`) stored the text |
| `prompt_sha256` + `prompt_chars` | **`not captured — prompt digest only`**. A digest proves a request happened; it does not say what it was, and hooks v1 stores no text (ADR-005 §2.2) |
| plan item / review note / manual note | the answer *if* the source is listed in `deps.provenance.intent_sources`, and printed as `not used — … is not in deps.provenance.intent_sources` if it is not |
| anything below `deps.provenance.min_confidence` | printed as a labelled `guess`, never silently dropped |
| attribution (`agent codex`) | not an answer by default (`deps.provenance.agent_counts_as_intent = false`). It says who *executed* the change. Turning it on makes `intent: not recorded` impossible to ever see |

### Should I be worried — advisories, with their age attached

Advisory lookup is an **enrichment, never a precondition**. The card renders with no cache, a stale
cache, a corrupt cache, or no network, and in each case says which happened.

**Three gates must agree before a byte is sent.** All of these, and nothing less:

1. `deps.advisories.enabled` is true;
2. `deps.advisories.mode` is `refresh` (the default is `cache`: read the cache, never fetch);
3. `privacy.advisories` is true — **user-scoped**, so a cloned `fathom.toml`, a `--set`, or anything
   a repository controls cannot grant it (ADR-001 §7.5).

`fathom deprisk consent` prints the three gates and the verdict:

```
$ python3 -m recommender.deprisk consent
network consent for advisory lookups
  `deps.advisories.enabled` = true
  `deps.advisories.mode` = cache
  `privacy.advisories` = false (built-in default)
  endpoint: https://api.osv.dev (fixed in code: a config key here would let a clone redirect the fetch)
  verdict: cache only: `deps.advisories.mode = "cache"`, so nothing is fetched. Ask for it with `advisories --refresh`
  default config makes 0 network call(s)
```

Three things were refused on purpose:

* **no `deps.advisories.endpoint` key.** An endpoint read out of a cloned file is an endpoint an
  attacker chose, and the "no network by default" claim would then only have been true of the
  default config. `https://api.osv.dev` is fixed in code, and `_osv_query` refuses anything else.
* **no fetch outside `advisories --refresh`.** No render verb takes the flag. The one call site is
  `cli._osv_query`, imported lazily, and `advisories --plan` prints every request a refresh *would*
  make without making one. Without consent the verb exits **8** (`NETWORK_CONSENT_REQUIRED`) naming
  the gate that refused — at the gate, not at the socket.
* **no unbounded refresh.** `deps.advisories.max_packages` caps the query count per episode and
  `deps.advisories.timeout_ms` bounds each request, because a lookup that can hang is a lookup that
  blocks episode rendering, which the ticket forbids.

**Freshness is a first-class answer.** Every cache read lands in one of five states, and the card
says which:

| State | What the card may claim |
| --- | --- |
| `fresh` | "no advisories found for N package(s) checked" |
| `stale` (past `max_age_hours`) | same claim, plus "past `max_age_hours` — still used, age stated" |
| `expired` (past `stale_limit_hours`) | **the tense changes**: `not checked recently`, never `no advisories found`. A seven-week-old clean result is not a clean result |
| `missing` | "no advisory cache to read at `<path>`", the card renders anyway, and every package is listed as not checked |
| `offline` | "advisories were not consulted (`deps.advisories.mode = "offline"`)"; the cache is not even opened |

A package with no cached entry is **never** counted as clean, and neither is a package whose last
query *failed*: `deps.advisories` stores failed queries as failures, with the message, so the next
render says "the last query failed (timeout after 1500 ms)" instead of implying it came back clean.
Advisories with no upstream severity are printed as `unseverity` rather than as nothing.

The cache is an ordinary file at `deps.advisories.cache` (default `.fathom/advisories.json`, which
`.gitignore` already covers). It is written only by a consented refresh, and
`cache --import-from FILE` fills it offline from recorded OSV responses — the route CI and airgapped
machines use, and the route this document's examples were produced by.

### Should I be worried — the licence matrix

A licence is not one question, it is two: what does it *say*, and what layer is now asking for it.
So the policy has two configurable halves.

The **class tables** are lists of SPDX ids, not baked-in opinions: `deps.license.permissive`,
`deps.license.weak_copyleft`, `deps.license.strong_copyleft`, `deps.license.network_copyleft`, plus
`deps.license.deny` as a veto no matrix cell can out-vote. Expressions are parsed: `AND` is
cumulative so the most restrictive part decides; `OR` is a choice, so the least restrictive option
is taken, named (`taken as MIT`), and the options not taken are printed rather than dropped.

The **matrix** then says what a class may do, and it is the whole point of doing this in config:

| class | in the library layer | elsewhere |
| --- | --- | --- |
| strong copyleft (GPL…) | `deny` | `warn` |
| weak copyleft (LGPL, MPL…) | `warn` | `allow` |
| network copyleft (AGPL, SSPL…) | `deny` | `deny` |
| unknown / undeclared | `warn` (as a stated partial) | `warn` |

`fathom deprisk matrix` prints what is currently in force; `fathom deprisk policy` prints every key
with the layer that won it. `deps.license.library_layers` is matched against `[layers]`, and this
repository dogfoods the key — `fathom.toml` says its library layers are `l0_vocabulary` through
`l3_analysis`, which is why a GPL dependency arriving in `crates/fathom-analyze` is a refusal here
rather than a note in a card nobody reads.

Every licence finding carries the **edge path** that asked for the package, because "who asked" is
the half a reviewer can act on:

```
Licence refused by policy: ring 0.16.8 — GPL-3.0-only · at crates/fathom-analyze/Cargo.toml
  - licence GPL-3.0-only as declared in crates/fathom-analyze/Cargo.toml
  - offending edge path: fathom-analyze → jwt 2.1.0 → ring 0.16.8
```

Four states are distinguished, because "no violation found" means something different in each:
`new` (this episode asked for it), `drift` (the declared licence moved while the version may not
have), `unchanged`, and `removed` (a departing package is never a policy question). A package with
no declared licence is a **stated partial** — `license-unknown` — and never a pass.

### The card

`fathom deprisk demo` renders and audits this exact fixture (`advisory_hot`, a recorded cache):

```
──────── dependency risk · ep-63
  add refresh-token rotation to the auth service
  ep-63 · 3 new direct dependencies (jwt 2.1.0, ring 0.16.8, thiserror 1.0.61); intent
  recorded for all of them; 3 advisories (1 critical, 1 high, 2 with a fix)
  advisory cache 1 h 0 m old (.fathom/advisories.json) · consent: cache only:
  `deps.advisories.mode = "cache"`, so nothing is fetched. Ask for it with `advisories
  --refresh`
  confidence 1.00 · 0 stated partial(s) · 10 finding(s)
who asked for this
  jwt 2.1.0 (rust, direct)
    introduced by: episode ep-63 (commit 7788abc) — added by the episode under review
    intent: "add refresh-token rotation to the auth service"
      source: `Agent-Intent:` trailer (confidence 1.00) · attribution: agent codex
    asked by: fathom-cli → jwt 2.1.0
    declared in: crates/fathom-cli/Cargo.toml
advisories
  cache: .fathom/advisories.json · 1 h 0 m
  !! jwt 2.1.0 · GHSA-7jvh-c3cj-qqh6 critical — jwt: signature bypass when the algorithm
  header is not validated
      fix: 2.1.4 (installed 2.1.0)
  !! ring 0.16.8 · GHSA-h4mh-5fw9-j367 high — ring: buffer overrun in the AES-GCM assembly
  path on 32-bit targets
      fix: 0.16.9 (installed 0.16.8)
   . jwt 2.1.0 · GHSA-4qw4-jh47-jh97 moderate — jwt: denial of service through a deeply
   nested claim set
  no advisories for 1 checked package(s) (cache 1 h 0 m)
  (network: cache only: `deps.advisories.mode = "cache"`, so nothing is fetched.)
licence
    jwt 2.1.0 licence MIT · [permissive · allow] · layer l5_composition (layers) · via
    fathom-cli → jwt 2.1.0
      - rule: permissive outside the library layer, and `deps.license` is `allow`
      - licence MIT as declared in crates/fathom-cli/Cargo.toml
      - resolved from registry:crates.io
findings (10) · critical 1 · high 1 · notable 1 · note 7
  Critical advisory: jwt 2.1.0 · GHSA-7jvh-c3cj-qqh6 · …
```

Six claims are re-read out of the finished lines by `render.audit`, because each is the kind of line
a later refactor drops while every test still passes: the freshness line, every provenance answer
(including a `not recorded` one), every violation's edge path, every finding or its counted
remainder, the stated confidence, and header counts that match `view.counts()`. `card --check` and
`demo` exit non-zero when the audit fails, so the audit runs in CI rather than in a unit test nobody
trusts.

### Verbs

| Verb | Prints |
| --- | --- |
| `fathom deprisk card` | the whole card (`--fixture NAME` \| `--manifest FILE\|-`, `--json`, `--check`) |
| `fathom deprisk summary` | the one sentence |
| `fathom deprisk provenance` | the "who asked for this" rows on their own |
| `fathom deprisk license` | licence rows, drift, and the newly-added licence ids (`--violations`) |
| `fathom deprisk findings` | every finding with evidence, filterable by `--severity` / `--kind` |
| `fathom deprisk advisories` | cache state, freshness, coverage; `--plan` shows a refresh, `--refresh` does one (exit 8 without consent) |
| `fathom deprisk cache` | inspect the cache, or fill it offline with `--import-from` |
| `fathom deprisk consent` | the three network gates and the verdict |
| `fathom deprisk policy` / `matrix` / `rules` / `definition` / `explain` | every key with its winning layer, the matrix, the finding table, the rules as sentences |
| `fathom deprisk score` | the corpus scored against its own bars — the CI gate |
| `fathom deprisk property` | reconciliation on generated episodes |
| `fathom deprisk demo` | this document's card, audited |

### Configuration

Every key is declared in `e0.8`'s schema (`src/recommender/config/schema.py`), appears in
`schemas/fathom.schema.json` and in [config.md](config.md), and is read as a value — no analysis
module reads a file, an environment variable or a clock. `fathom deprisk policy --digest-only` prints
the digest that invalidates a cached card when any of them moves.

The ones worth knowing: `deps.advisories.mode` (`cache` by default), `deps.advisories.max_age_hours`
(24) and `stale_limit_hours` (168, and it must be the larger of the two — the analyzer refuses the
pair otherwise rather than printing a self-contradicting card), `deps.advisories.max_packages` (200),
`deps.advisories.timeout_ms` (1500), `deps.license.deny`, the four class tables,
`deps.license.library_layers`, the six matrix cells, `deps.provenance.intent_sources`,
`deps.provenance.min_confidence`, `deps.provenance.agent_counts_as_intent` (false), and one switch
per finding kind under `deps.findings.*`. Switching a finding off counts it in the card's
*suppressed* line; it does not delete it.

### What this refuses to do

* **No network by default, and no key that changes that.** Verified by tests that replace
  `socket.socket` with a trap and then run every verb.
* **No advisory lookup that can block rendering.** No cache read is on a render path's critical
  path; there is nothing to wait for, and the timeout bounds the one path that could.
* **No invented provenance.** No guessing an introducing episode from dates, authors or commit
  order. Attribution is never intent by default. A prompt digest is never a quote.
* **No "clean" claim the evidence does not license.** Not from an expired cache, not from an
  unchecked package, not from a failed query, not from an undeclared licence.
* **No licence rule that is not config.** The copyleft tables, the matrix, and which of a
  repository's layers are library layers all come from `fathom.toml`; the built-ins are a starting
  point that `fathom deprisk matrix` prints, not a hidden opinion.

### How the risk answer is checked

```
python3 -m recommender.deprisk score      # 28 authored cases, 9 gates, all at 1.0
python3 -m recommender.deprisk property   # reconciliation on 200 generated episodes
python3 -m pytest tests/test_deprisk_contract.py
```

The corpus (`src/recommender/deprisk/corpus.py`) is hand-authored, including recorded OSV payloads,
so no test needs a network or a fake server that would not be the real shape. The gates are
provenance accuracy, intent accuracy, licence precision **and** recall (precision is the trap half:
`dual_licence_ok`, `copyleft_in_application`, `dev_only_copyleft` and `removed_dependency` exist to
catch a checker that fires on everything), advisory recall, freshness accuracy, the finding table,
reconciliation and determinism. `tests/test_deprisk_contract.py` maps the four acceptance criteria
one section each, and the analysis half's purity is asserted by AST — the modules that produce the
answer cannot import `socket`, `urllib`, `os`, `pathlib`, `time` or `datetime`.

---

**Next**: [delta](delta.md) for the document this family's answer is a section of ·
[summary card](summary-card.md) for where the dependency card is printed ·
[paths](paths.md) for the taxonomy that decides which path is dependency surface at all ·
[config](config.md) for the five layers every `deps.*` key is read from ·
[thesis](thesis.md) for the glossary entries these three pages implement.
