# The headless verbs

**Status**: shipped by `e1.10` (reco-ld8), on top of the store `e1.2`/`e1.4` and the watcher `e1.9`.
**Audience**: scripts, CI, and the person writing a bug report.

Every verb here answers the same three questions — *what is the scale, what is the state, what is wrong* —
and every one of them has a `--json` form whose shape is checked in under [`schemas/`](../../schemas) and
round-trip-tested in [`crates/fathom-cli/tests/schemas.rs`](../../crates/fathom-cli/tests/schemas.rs). The
TUI is the same facts on a screen (ADR-006 §6.7: "every view has a headless equivalent"); the verbs are
what a machine reads.

## The verbs

| Verb | What it answers | `--json` schema |
|---|---|---|
| `fathom index [ROOT] [--rebuild] [--watch] [--refresh] [--stats] [--committed-only]` | turn git history into the index, and say what the run did | [`index.schema.json`](../../schemas/index.schema.json) |
| `fathom show <episode\|rev> [--root R] [--json]` | one episode: identity, authorship, message, trailers, and the file rows with their taxonomy decisions | [`show.schema.json`](../../schemas/show.schema.json) |
| `fathom stats [ROOT] [--json]` | repo scale, index state, cache hit rate | [`stats.schema.json`](../../schemas/stats.schema.json) |
| `fathom doctor [--root R] [--privacy] [--deep] [--strict] [--json]` | the build, the environment, the repository and the index — each finding with a remedy | [`doctor.schema.json`](../../schemas/doctor.schema.json) |
| `fathom export [--format md\|html\|json] [--out STEM] [--episodes RANGE] [--json]` | the review artefact — **skeleton**: scope and format are validated, the rendering is `e5.7` | [`export.schema.json`](../../schemas/export.schema.json) |

`fathom show` takes a revision or an object id — anything
[`fathom_ingest::revspec`](../../crates/fathom-ingest/src/revspec.rs) resolves: `HEAD`, `HEAD~3`, `HEAD^2`,
`main`, `v1.2^{}`, a full oid, or a hex prefix. Episode identity of its own arrives with `e1.6`; until then
one commit is one episode, and the verb says so rather than inventing a grouping.

`fathom index --rebuild` advances the epoch and re-walks (ADR-004 §6.3, §9.3). The staging file and the
atomic swap are *not* here yet and are not a gap: they exist so a concurrent reader never sees a
half-rebuilt epoch, and until the daemon (`e6.3`) and the TUI (`e1.5`) read the store while a run writes it
there is no concurrent reader. An interrupted rebuild leaves the old epoch's rows on disk and marked dead,
which is recoverable by definition — and `fathom stats` reports a parked epoch as `building`, with the
remedy.

`fathom index --watch` is `fathom watch` after an index pass: one command for a script that wants "index,
then keep this fresh". A binary built without the `watch` feature says so with `FEATURE_ABSENT` rather than
indexing and pretending.

## Exit codes

The published table is [`ADR-002 §7.3`](../adr/0002-workspace.md);
[`fathom_core::exit::ExitCode`](../../crates/fathom-core/src/exit.rs) is the code, and
`docs/adr/0002-workspace.md`'s generated block is the contract. The ticket's four classes map onto it like
this — the table is normative, and the codes below are *its* numbers:

| Class | Code | Name | When a verb leaves with it |
|---|---|---|---|
| ok | 0 | `OK` | the verb answered and the index describes HEAD |
| user error | 1 | `ERROR` | a rev that names nothing, an unimplemented verb (`fathom check`, the `export` skeleton), a rejected argument *value* |
| user error | 2 | `USAGE` | clap rejected the command line: unknown flag, unknown verb, missing argument |
| environment | 3 | `NOT_A_REPOSITORY` | `--root` is not a git repository, or `.git` is unreadable |
| environment | 5 | `CONFIG_INVALID` | `fathom.toml` failed the schema |
| environment | 9 | `FEATURE_ABSENT` | the verb needs a feature this binary was not built with (the `minimal` CI binary) |
| store | 4 | `STORE_CORRUPT` | the store failed `quick_check`/`integrity_check`; the remedy is a rebuild |
| store | 6 | `CANCELLED` | a cancel handle was pulled mid-run |
| **degraded index** | **12** | **`INDEX_DEGRADED`** | see the table below |

### What counts as a degraded index

Every row is a finding, printed with a remedy, and asserted in
[`crates/fathom-cli/tests/cli.rs`](../../crates/fathom-cli/tests/cli.rs):

| Finding code | State | Code | Remedy printed |
|---|---|---|---|
| `index_missing` | no store file at all | 12 | `fathom index` |
| `index_stale` | HEAD has no commit row: the newest work is in no answer | 12 | `fathom index` |
| `index_incomplete` | commits still owe an analysis (a run died after writing them) | 12 | `fathom index` |
| `index_run_unclean` | a run is still `running`, or ended `crashed`/`aborted` | 12 | `fathom index` |
| `index_building` | the epoch is marked `building` — a rebuild was started and not finished | 12 | `fathom index --rebuild` |
| `index_empty` | the repository has commits and the index has none | 12 | `fathom index` |
| `store_corrupt` | the file is not trustworthy | 4 | `fathom index --rebuild` |
| `store_unusable` | the store will not open at all | 4 | `fathom index --rebuild` |
| `not_a_repository` | `--root` is not a repository | 3 | point `--root` at one |
| `repository_unreadable` | it is a repository, but `.git` cannot be read | 3 | check the permissions, or point `--root` at the work tree |
| `feature_absent` | this binary was built without a SQLite backend | 9 | rebuild with `--features sqlite-bundled` |

An index that is empty because the *repository* has no commits is not degraded. That is ADR-006 §10.3's
empty state, printed with its fixed copy: `no episodes yet — run fathom index`.

**Which verb exits for what.** `fathom stats` and `fathom show` exit with the worst finding they print — a
script asking "is this repo indexed?" should not have to parse prose. `fathom doctor` exits **0** by
default and prints its findings: it is the command you run *because* something is wrong, and a diagnostic
that exits non-zero makes the diagnostic the thing that is wrong. `fathom doctor --strict` is the CI form
of the same report and exits with the worst finding's code. When two findings apply, the verb leaves with
the higher number (a plain directory is both `NOT_A_REPOSITORY` and `INDEX_DEGRADED`; the index verdict is
the one with more to do about it, and both are printed).

## Output discipline

- **stdout is the answer, stderr is the diagnosis.** `fathom --json` prints exactly one JSON value on
  stdout and nothing else there, so it pipes into `jq` (ADR-002 §7.5).
- **Every error line comes with a hint and a code**: `error:` / `hint:` / `code: NAME (n)` on stderr, which
  is what a CI log needs to be actionable without a shell function to decode numbers.
- **A stated absence, never a blank.** `hit_rate: null` means "nothing was asked", not 0%. An absent store is
  `index.present: false` with a path and a remedy, not `commits: 0`. ADR-006 §10.3 calls this "a value, not
  a blank", and the headless surface is where it matters most, because a script cannot notice a blank.
- **Paths are repository-relative or `~`-relative, credentials are stripped, and config values are never
  printed** — a config layer is reported as a path, a size and an mtime. That is what makes
  `fathom doctor --json` safe to paste into a bug report (`e1.10`'s third criterion), and the test that
  asserts it plants a token in a config file under a fake `$HOME` and a password in a remote URL.
- **A recovered session is a warning, not a failure.** `e1.11` prints its way out of damage on stderr —
  the store that was not SQLite and is now under `.fathom/quarantine/`, the row that did not decode, the
  `--fresh` run that is starting from defaults — and exits 0, because the exit code answers "did fathom do
  the thing", and it did the thing. The one case that exits non-zero is the one where it cannot: a store it
  opened and cannot overwrite, which says so instead of minting a second index.

## Reading the cache numbers

`fathom index --stats`, `fathom stats` and `fathom doctor --json` all read the store's `counter` table
through the same function (`fathom_analyze::index::stats_of`), which is the only way the three can quote one
hit rate instead of three approximations (ADR-004 §11). Two of the fields are easy to misread:

- `puts` counts **rows written**; a `--refresh` recompute writes a row after a hit.
- `computes` counts **misses** (`lookups − hits − negative_hits`): the work the repository actually made
  the analyzers do. A warm cache with a low `hit_rate` and `bumped > 0` did not break — something's version
  or config moved (§5.6).
