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
| `fathom episodes [--root R] [--json] [--limit N] [--offset N] [--author N] [--kind K] [--state S] [--path P] [--since W] [--until W] [--include-dead]` | the **episode list** — the rail's rows: id, time, author kind, `+add`/`−del`, path count, review state, session/turn, plus the three counts (index, selection, page) and a breakdown by review state | [`episodes.schema.json`](../../schemas/episodes.schema.json) |
| `fathom analyze <episode\|rev\|id> [--root R] [--json] [--schema [--check\|--write]]` | the episode's **delta document** — the seven-section model the TUI card, the export and an agent all read; `--schema` prints its JSON Schema | [`episode-delta.schema.json`](../../schemas/episode-delta.schema.json) |
| `fathom narrative <episode\|rev\|id> [--root R] [--json] [--markdown] [--explain] [--top N] [--attribution]` | one episode's **narrative** — the prose a human skims, with a citation on every claim, the self-check verdict printed with it, and (with `--attribution`) whether a model was ever in the path | `fathom-narrative/1`, documented in [the episode narrative](episode-narrative.md) |
| `fathom drift [--root R] [--session S] [--all] [--window N] [--min-overlap F] [--max N] [--dismiss KEY] [--off KIND] [--json] [--fail-on-findings]` | **plan versus execution** across a session: the six contradictions between what the recorded intent said and what the episodes did, each with both sides linked | `fathom-drift/1`, documented in [plan versus execution](plan-drift.md) |
| `fathom why [EPISODE\|rev\|id] [--root R] [--json] [--weight NAME=VALUE] [--tau N] [--calibrate [SET]] [--weights]` | one episode's **risk score and its arithmetic** — every feature, its weight, its contribution, the reasons in order, and what the model could not see; `--calibrate` prints precision/recall of the top decile before and after a retune | `fathom.cli.why/1`, documented in [risk scoring](risk-scoring.md) §4 |
| `fathom stats [ROOT] [--json]` | repo scale, index state, cache hit rate | [`stats.schema.json`](../../schemas/stats.schema.json) |
| `fathom doctor [--root R] [--privacy] [--deep] [--strict] [--json]` | the build, the environment, the repository and the index — each finding with a remedy | [`doctor.schema.json`](../../schemas/doctor.schema.json) |
| `fathom export [--format md\|html\|json] [--out STEM] [--episodes RANGE] [--root R] [--stdout] [--strict] [--json]` | the **review artefact** — one record (summary card, narrative, dependency and architecture deltas, findings with evidence, the operator-action checklist, the sign-off block and the gap list) written as markdown, HTML or JSON, byte-identical when re-exported | [`export.schema.json`](../../schemas/export.schema.json), documented in [the review artefact](export-artefact.md) |
| `fathom status [ROOT] [--socket PATH] [--index] [--json]` | **who is keeping this index current** — the daemon's pid, uptime, hook offset, queue counters and measured CPU share, or, when nothing is running, the lease and heartbeat it fell back to and the endpoint it would have used | [`status.schema.json`](../../schemas/status.schema.json), documented in [the watch daemon](watch-daemon.md) |
| `fathom daemon start [--foreground] [--socket PATH] [--cpu-share-pct N] [--json]` / `fathom daemon stop` | start and stop the **one writer per repository**: the lease, the socket, the budgeted indexing loop | [the watch daemon](watch-daemon.md) |
| `fathom hooks [ROOT] [--check] [--replay] [--tail] [--max N] [--trailer S:T] [--strict] [--json]` | the harness event stream: lag, durable offset, counts by type, why each refused line was refused, and a deterministic replay | [`hooks.schema.json`](../../schemas/hooks.schema.json) |

`fathom analyze` takes an episode id or anything
[`fathom_ingest::revspec`](../../crates/fathom-ingest/src/revspec.rs) resolves, and prints the same document in two forms: rendered sections for a human, one JSON line for a machine ([the delta model](delta-model.md) is the contract, and `schemas/episode-delta.schema.json` is generated by this verb and diffed in CI). Without `--json` it prints the model's own rendering, which is the same text the card's rows come from — one set of words, three surfaces.

`fathom show` takes a revision or an object id — anything
[`fathom_ingest::revspec`](../../crates/fathom-ingest/src/revspec.rs) resolves: `HEAD`, `HEAD~3`, `HEAD^2`,
`main`, `v1.2^{}`, a full oid, or a hex prefix. Episode identity of its own arrives with `e1.6`; until then
one commit is one episode, and the verb says so rather than inventing a grouping.

`fathom episodes` is the rail with no screen (`reco-11b`), and it exists because a fact that only a terminal
can show is a fact CI cannot check: the acceptance run could say "the rail shows 213 episodes" only by
looking at one. It is the *same read* — one [`EpisodeQuery`](../../crates/fathom-store/src/episode.rs)
against the same store, the same review states, the same order (`last_utc` descending, then id) — which is
what lets `--plain` and a script be given the same rows rather than two stories about them. Three counts are
printed because there are three questions: `index.episodes` is everything the index holds, `page.matched` is
that after the filters, `page.returned` is that after the page; the shell holds at most 500 rows at a time and
this verb is not the shell, so a page cut short is announced rather than left as a short list. `--state`
means what the shell's `u` means (`unread` is "nobody has decided", not "the row says unread"), `--since` and
`--until` take `fathom search since:`'s grammar (`7d`, `2w`, `today`, `2026-10-01`), and `--include-dead`
asks for the ghosts a rewrite left — which the pruner has usually removed, and the notes say either way. A
selection of nothing is exit 0 with the reason: "how many flagged episodes?" asked of a repository with none
is a question, not a failure. There is no clock in the payload, so two runs over the same index are
byte-identical and a CI step can diff them.

`fathom index --rebuild` advances the epoch and re-walks (ADR-004 §6.3, §9.3). The staging file and the
atomic swap are *not* here yet and are not a gap: they exist so a concurrent reader never sees a
half-rebuilt epoch, and until the daemon (`e6.3`) and the TUI (`e1.5`) read the store while a run writes it
there is no concurrent reader. An interrupted rebuild leaves the old epoch's rows on disk and marked dead,
which is recoverable by definition — and `fathom stats` reports a parked epoch as `building`, with the
remedy.

`fathom hooks` reads the ADR-005 stream and writes only its own state under `.fathom/hooks/`. The four
actions are exclusive, and `--check` is the default: it scans the unread region on a **copy** of the cursor,
so a monitoring loop can poll it without consuming the stream or pushing real records out of the quarantine
sidecar with its own re-recorded duplicates. `--replay` is the determinism proof — it rebuilds from byte 0
and prints the turn ledger's digest, which is equal across rebuilds of the same bytes and is what a review
keyed to a turn id survives. `--trailer sess-7:4` prints the `Agent-Session:` / `Agent-Turn:` pair
(ADR-003 §9.1, §18.2 **D5**); the turn id is session-qualified, so two sessions' turn 3 are `sess-7:3` and
`ci-build:3` rather than two things called `3`. Without `--strict` a stream full of refused lines still exits 0
with `ok: false` — a diagnostic that exits non-zero in a script is a script that stops reporting (ADR-005
§11.1 rules its own status payload the same way).

`fathom index --watch` is `fathom watch` after an index pass: one command for a script that wants "index,
then keep this fresh". A binary built without the `watch` feature says so with `FEATURE_ABSENT` rather than
indexing and pretending.

`fathom index`, `fathom watch` and `fathom ambient` all ask the daemon first when one is serving
([the watch daemon](watch-daemon.md)): who writes is a fact about a lock, not about which binary you ran
(ADR-004 §7.1). A deferred `fathom index --json` is `fathom.cli.index.deferred/1` and not
`fathom.cli.index/1`, because the timings, cache table and segmentation of a run report belong to a run this
process did not perform. `--rebuild`, `--refresh` and `--stats` still do the work here: a rebuild moves the
writer epoch, which is not a request anybody else can make on your behalf.

## Exit codes

The published table is [`ADR-002 §7.3`](../adr/0002-workspace.md);
[`fathom_core::exit::ExitCode`](../../crates/fathom-core/src/exit.rs) is the code, and
`docs/adr/0002-workspace.md`'s generated block is the contract. The ticket's four classes map onto it like
this — the table is normative, and the codes below are *its* numbers:

| Class | Code | Name | When a verb leaves with it |
|---|---|---|---|
| ok | 0 | `OK` | the verb answered and the index describes HEAD |
| user error | 1 | `ERROR` | a rev that names nothing, an artefact that could not be written, a rejected argument *value*, an artefact whose own self-check failed |
| user error | 2 | `USAGE` | clap rejected the command line: unknown flag, unknown verb, missing argument |
| environment | 3 | `NOT_A_REPOSITORY` | `--root` is not a git repository, or `.git` is unreadable |
| environment | 5 | `CONFIG_INVALID` | `fathom.toml` failed the schema |
| environment | 9 | `FEATURE_ABSENT` | the verb needs a feature this binary was not built with (the `minimal` CI binary) |
| store | 4 | `STORE_CORRUPT` | the store failed `quick_check`/`integrity_check`; the remedy is a rebuild |
| store | 6 | `CANCELLED` | a cancel handle was pulled mid-run |
| **degraded index** | **12** | **`INDEX_DEGRADED`** | see the table below |

### What counts as a degraded index

Every row is a finding, printed with a remedy, and asserted in
[`crates/fathom-cli/tests/cli.rs`](../../crates/fathom-cli/tests/cli.rs). The repository shapes that produce
them — a shallow clone, a filtered clone, a corrupt loose object, a pruned one, three hundred branches, a
force-push rewrite, a `.git` that lives elsewhere — are fixtures of their own in
[`crates/fathom-cli/tests/resilience.rs`](../../crates/fathom-cli/tests/resilience.rs), which is also where
the promise that a degraded run never loses the review ledger is pinned.

| Finding code | State | Code | Remedy printed |
|---|---|---|---|
| `index_missing` | no store file at all | 12 | `fathom index` |
| `index_stale` | HEAD has no commit row: the newest work is in no answer | 12 | `fathom index`, or the object-database remedy below when HEAD is one of the unreadable commits |
| `index_incomplete` | commits still owe an analysis (a run died after writing them) | 12 | `fathom index` |
| `index_run_unclean` | a run is still `running`, or ended `crashed`/`aborted` | 12 | `fathom index` |
| `index_building` | the epoch is marked `building` — a rebuild was started and not finished | 12 | `fathom index --rebuild` |
| `index_empty` | the repository has commits and the index has none | 12 | `fathom index` |
| `object_unreadable` | the object database would not serve these commits; each one is named with the reason the read gave, and the rest of the index is intact | 12 | **the reason decides** (see below): a deeper fetch for a graft, a relaxed filter for a filtered clone, `git fsck --full` only when nothing explains the object — then `fathom index`, because fathom never fetches or repairs |
| `history_shallow` | the clone is shallow, so the history stops partway back | 12 | `git fetch --unshallow` (yours to type), then `fathom index` |
| `partial_clone` | the clone was filtered (`--filter=blob:none` and friends), so some objects were never fetched | 12 | re-clone without the filter, or fetch what is missing, then `fathom index` |
| `broken_symref` | a symbolic ref points at a ref that does not exist | 3 | `git update-ref -d <ref>`, or point it at a real target |
| `worktree_missing` | the working tree this repository points at is not there — a linked work tree that was moved or deleted out from under its registration | 3 | restore the directory, or drop the stale registration with `git worktree prune`; the commits are still indexable from a bare or linked checkout |
| `repository_untrusted` | the repository belongs to another user, so git read it at arm's length and fathom read it the same way | 3 | `git config --global --add safe.directory <path>`, if you trust it |
| `store_corrupt` | the file is not trustworthy | 4 | `fathom index --rebuild` |
| `store_unusable` | the store will not open at all | 4 | `fathom index --rebuild` |
| `not_a_repository` | `--root` is not a repository | 3 | point `--root` at one |
| `repository_unreadable` | it is a repository, but `.git` cannot be read | 3 | check the permissions, or point `--root` at the work tree |
| `feature_absent` | this binary was built without a SQLite backend | 9 | rebuild with `--features sqlite-bundled` |

### The remedy for a missing object is the reason for it

"Could not read this commit" has three causes and they want three different commands, and printing the wrong
one costs a reviewer an afternoon. A shallow clone — the normal first clone on CI, and what
`tools/fetch_g3_corpus.py` makes on purpose — has commits whose objects are on the far side of a graft. That
is not damage: nothing in the store contradicts anything else, the objects were simply never fetched. Telling
that reviewer to run `git fsck --full` sends them to a repair tool for a healthy repository; they find
nothing, and file the index as broken. So the reason the read gave picks the sentence:

| Why the object is missing | Remedy |
|---|---|
| behind a shallow graft (`.git/shallow` names the boundary) | `git fetch --deepen <n>`, or `git fetch --unshallow` for the whole history |
| left out by the clone's `--filter` | `git fetch --refetch` from the promisor remote, or re-clone without the filter |
| nothing in the repository explains it | `git fsck --full` — the only case where a repair tool is the thing to type |

When one store has both kinds of hole the finding counts them separately and gives both commands. The reason
travels into the index as text (the store's `commit_unreadable.*` rows) and `fathom_ingest::error::Absence::of` re-reads the case
off it, which is why the wording of each case in `fathom-ingest` is asserted to carry its marker: a rewording
that dropped "shallow graft" would quietly turn every CI clone back into a broken repository.

The finding is built once and printed by every surface that reports it — `fathom index`, `fathom stats`,
`fathom doctor`, `fathom analyze`'s footer and the TUI's notice list — with the same message and the same
remedy, because three near-identical sentences drift, and one of them will be the wrong one
(ADR-006 §10.3).

### The one finding that is untidy rather than degraded

`fathom stats` exits 12 for everything above because in each case an answer is missing or wrong. There is
one finding that is not like that, and printing it with code 12 anyway would make a healthy repository look
broken:

| Finding code | State | `stats` | `doctor` | `doctor --strict` | Remedy printed |
|---|---|---|---|---|---|
| `index_dead_rows` | commit rows for commits that no longer exist: a `rebase`, a `commit --amend` or a force-push replaced them, and `git gc --prune=now` has usually deleted the objects | 0 | reported, exit 0 | **12** | `fathom index --prune` |

The index is not wrong in this state — HEAD is indexed, the queue is segmented over the history the refs
still reach, and every episode answer is the answer. What is wrong is that the store *counts* as the
repository the rows a rewrite left behind: `fathom stats` used to print `505 commit(s)` for a 500-commit
repository and `fathom doctor` said "nothing found". So the size the report prints is what a ref can still
reach, the rows it left out are printed as their own line with the verb that removes them,
and the exit code of `--strict` — the pre-flight posture, the one that asks "is there anything here somebody
should fix" — is where the difference has to be paid.

The pruner is the only thing that deletes (ADR-004 §8.1), and it refuses rather than guesses:

* a row whose death the store cannot prove stays. A shallow clone's graft, or a parent the walk could not
  read, means the rows below that break might be ancestors rather than ghosts — `fathom stats` prints them
  as `reach … could not place`, never as dead;
* a dead commit whose episode carries a review, or a mirrored sign-off, stays, because a prune may never
  lose a review (ADR-004 §8.2) and the relink ladder needs the episode it carried a decision from. The prune
  says how many it left alone instead of reporting a smaller number.

**The keys.** `fathom stats --json` carries the pair in `index`: `commits` is what the refs reach — the number
the text report prints — and `commits_retained` is what the file holds. Between them are the dead set:
`dead_commits` and `dead_files` (the rows, and the file rows they carry), `dead_sample` (up to twelve of
them as `{commit}`, newest first), and `dead_unjudged` with `dead_unjudged_reason` for the rows the store
could place in neither set. `fathom index --prune --json` adds a `prune` object — `dry_run`,
`commits_removed`, `files_removed`, `episodes_removed`, `withheld_reviewed`, `withheld_undecidable`, `refusal`
and `sample` — and prints `prune: null` when `--prune` was never asked for, because "nothing was asked" is
not the same statement as "nothing was dead".

A commit the object database refuses to serve is *not* a failed run. The run closes `ok` with the commits it
could read, and the hole is recorded against each commit id — `fathom doctor` and `fathom stats` name those
commits and their reasons until a later run reads them, and `fathom show`/`fathom analyze` on one of them
says so instead of telling you to `fathom index` a commit the repository cannot produce. A partial index is
a usable index; the missing part is said, not hidden.

Other repository facts are stated as notes rather than findings, because they change what the history is
rather than whether fathom could read it — a bare repository, a detached HEAD, an unborn HEAD, more than a
hundred local branches, a `.git` on a different device from the work tree, a moved git directory, a
worktree pointing at a git directory that is gone. `fathom stats` prints them under `repository.notes` and
`fathom doctor` prints them as `note:` lines, each with what to do about it, and none of them move the exit
code.

An index that is empty because the *repository* has no commits is not degraded. That is ADR-006 §10.3's
empty state, printed with its fixed copy: `no index here yet — run \`fathom index\` here, then reopen
fathom`. The shell prints the same sentence from the same read (the empty state the MVP walkthrough
filed): `fathom stats` says the
index is missing, and the TUI says it too rather than guessing.

**Which verb exits for what.** `fathom stats` and `fathom show` exit with the worst finding they print — a
script asking "is this repo indexed?" should not have to parse prose. `fathom doctor` exits **0** by
default and prints its findings: it is the command you run *because* something is wrong, and a diagnostic
that exits non-zero makes the diagnostic the thing that is wrong. `fathom doctor --strict` is the CI form
of the same report and exits with the worst finding's code. When two findings apply, the verb leaves with
the higher number (a plain directory is both `NOT_A_REPOSITORY` and `INDEX_DEGRADED`; the index verdict is
the one with more to do about it, and both are printed). `fathom episodes` and `fathom search` sit one step
apart from both of those: with no index at all they exit 12, because they have no answer to print, and with
an index that is merely stale they print the list and exit 0 — the rows they hold are the rows the store
has, and a list verb that failed because the watch daemon was not running would be a list verb that cannot
be used next to a running one.

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
