# First run — what a clean machine sees, and what every empty pane says

Status: built by `e5.9` (**reco-2uo**). Code: `crates/fathom-cli/src/demo.rs` (the miniature repository
and its unpacking), `crates/fathom-cli/src/guide.rs` (the CLI's progress row),
`crates/fathom-core/src/progress.rs` (the estimate), `crates/fathom-tui/src/empty.rs` (every pane's empty
state), `crates/fathom-tui/src/index.rs` (the index axis those sentences are built on),
`crates/fathom-tui/src/progress.rs` (the shell's clock). Consumed by `fathom --demo`, `fathom index`, and
every pane that has ever had no rows.

The premise is uncomfortable and worth saying plainly: **the first five minutes decide whether anybody
keeps this running.** Everything else in Fathom assumes a reviewer who has already decided to be here. A
person who has just installed an alpha has not. They have a directory, a terminal and a budget of about a
minute before an unexplained wait becomes a closed terminal — and a tool whose answer to "what is this?" is
an empty pane shrugging gets uninstalled before it has shown anything.

So three things are specified rather than left to whoever wrote each view: **the guided first index**
([§3](#3-the-guided-index--a-percentage-with-an-estimate-that-converges)), **the miniature**
([§2](#2---demo--a-repository-that-comes-with-the-binary)) and **the rule every empty state and every error
follows** ([§4](#4-every-empty-state-as-one-batch), [§5](#5-error-copy--every-refusal-names-the-next-action)).
The rule is the part that scales: nine views and forty-odd refusal sites are too much copy to review one at
a time, and copy written one pane at a time says nine different things about the same emptiness.

## 1 The first run, in the order it happens

| Step | What the reviewer sees | Where it is decided |
|---|---|---|
| `fathom --demo` | a miniature repository unpacked into the cache directory, indexed by the ordinary `fathom index`, and the shell open on it | §2 |
| `fathom index` in their own repository | one row that overwrites itself: `indexing 62 % · ~14 s left · 4 102 / 6 552 commits` | §3 |
| The first frame | a ranked queue over their own commits, or the reason there isn't one | §4 |
| Any pane with no rows | the cause, then the action that fills it, in that order | §4 |
| Any refusal | what went wrong and the thing to do about it, on stderr, beside an exit code that means something | §5 |

Two rules govern the sequence and both are inherited, not invented here:

* **a pane prints only what its caller handed it** (ADR-002 §5.2, ADR-006 §10.3). Nothing on the shell's
  side opens the store to find out why it is empty; the composition root reads once and hands over
  [`Facts`](../../crates/fathom-tui/src/index.rs), so an empty pane cannot invent an index state.
* **a sentence may not name a command this binary does not have.** An untypeable remedy is worse than none,
  because it gets typed — the defect ADR-006 §10.3 records, in prose, against an earlier build of this
  table.

## 2 `--demo` — a repository that comes with the binary

`fathom --demo` shows a real review, on a clean machine, with no configuration, in well under a minute.
Every clause of that sentence is load-bearing:

* **A real review, not a mock.** The miniature is an ordinary git repository — 21 commits by two people and
  one agent, containing a rename, a dependency bump in two ecosystems, a deleted test, a CI job, a cycle,
  a side branch and a merge. It is indexed by `fathom index`, stored in an ordinary store, and read by the
  same queries as a reviewer's own history. There is no demo mode below `demo.rs`: the flag unpacks a
  repository, changes the process's working directory to it, and gets out of the way. That is why
  `fathom --demo stats`, `fathom --demo show <episode>` and `fathom --demo --plain` all work without one
  special case anywhere, and why the demo cannot rot into a picture of the product that no longer matches
  it.
* **Clean machine, and reversible.** Nothing is read from the reviewer's configuration; nothing is written
  to their files. The miniature lives under `$XDG_CACHE_HOME/fathom/<digest>/…` (then `$HOME/.cache`, then
  the temp directory — the last for a container with no `HOME`), is marked with `.fathom-demo`, and
  deleting that directory is the complete undo. The guide says so out loud, because a stranger who cannot
  undo a thing will not run it.
* **Under a minute, measured.** Cold, with an empty cache, unpack + index + frame is asserted in CI to
  finish inside ten seconds against the ticket's minute. The bound exists so it stays true: the miniature is
  an asset and could silently grow.
* **No configuration read** — not "none required", which is a claim about defaults, but none *consulted*:
  no `fathom.toml`, no `FATHOM_*`, no interest in the directory it was invoked from.

The miniature is a packed list of tracked files (`crates/fathom-cli/assets/fathom-demo.pack`, ~30 KiB)
embedded in the binary and expanded once per digest. Loose git objects are already deflated, so there is no
decompressor and no `git` invocation in the path. `tools/gen_demo_repo.py` writes the pack and its manifest
with pinned dates and identities so two runs produce byte-identical repositories; `check` fails if either
asset drifts from the generator, the manifest's digest names the cache directory (which makes "already
unpacked" a `stat`, and an old binary's copy merely unused rather than stale), and the digest is printed in
the guide so a bug report can name which miniature somebody saw.

## 3 The guided index — a percentage with an estimate that converges

`fathom index` used to print nothing until it printed its summary. On a large history that is a minute or
more of silence from a process that is working, and a reviewer with no way to tell *working* from *hung*
presses <C-c> — a worse outcome than a slow index, because the next run starts again.

The row is ADR-006 §10.3's, unchanged:

```
indexing 62 % · ~14 s left · 4 102 / 6 552 commits
```

The percentage is arithmetic. The estimate is a claim about the future, which makes it the part that has to
be earned, and [`fathom_core::progress`](../../crates/fathom-core/src/progress.rs) is where the earning is
written down:

| State | The row says | Why it may not say anything else |
|---|---|---|
| fewer than three samples, or under 250 ms elapsed | `indexing 0 % · estimating… · 1 / 1 000 commits` | the first interval contains process start-up; quoting it as a rate teaches the reviewer to ignore the row |
| a total nobody could count before the work began (the spine walk) | `indexing unknown % · 4 s so far · 557 commits` | *unknown* is a value (ADR-006 §0.1), and the wait so far is the one duration a denominator-less phase honestly has |
| a phase whose denominator is not the next phase's denominator | `analysing 71 % · ~9 s left · 596 / 838 commits` | each phase counts its own units: the walk walks commits, the derivation pass runs units per commit, the worktree reads paths. One row per phase, never a percentage that silently changes what it divides |
| recent history | `~14 s left` | the estimate is the **median** rate of a twelve-sample window, so it follows a phase change and ignores one expensive unit |
| the counter has not moved for ten seconds | `stalled — fathom doctor says what it is waiting on` | a countdown that has stopped is a broken clock; the honest statement is that it has not moved, and the honest next action is the diagnostic verb |
| finished | `done` | `~0 s left` is an expired countdown, on screen at exactly the moment the reviewer looks |

The estimator is the one decision here that could have gone another way, so the rejected options are
recorded where the code is and asserted by `crates/fathom-tui/tests/progress.rs`: a whole-run mean cannot
follow the walk-to-analyses phase change (it undershoots the rest of the run, which is the classic "5
seconds remaining" at 95 %); exponential smoothing needs a constant no repository agrees to; and a
short-window mean lets one 2 s merge commit among twelve 4 ms ones predict 170 ms a unit — forty times
over. The tests use synthetic histories whose true per-unit cost is known, including a step change and an
outlier, because "it looked about right on my machine" is not a property.

Three decisions sit beside the arithmetic:

* **stderr, always** (ADR-002 §7.5). stdout carries the artefact, so `--json` silences the row whatever else
  was asked — including `--progress`, because a document reader will not tolerate `indexing unknown %` and a
  refusal that says so beats a silently half-obeyed flag.
* **one row that overwrites itself on a terminal; one line per update on a pipe.** A `\r` in a log file is a
  corrupt line, and forty rows of scrollback bury the summary the reviewer actually came for.
* **two rows a second.** `TICK_INTERVAL` exists because a row updating forty times a second is a spinner
  with digits. A phase change prints immediately whatever the clock says: `walking` becoming `analysing` is
  the fact the reviewer was waiting for, not a rate update.

`--progress` and `--no-progress` are the escape hatches for logs and scripts, and they are **global**
flags rather than `index` flags: `fathom --demo --progress` forces the rows on the indexing the demo
performs on the reviewer's behalf, which is the one run a stranger has no way to opt into otherwise. A plain
`fathom index` needed no new flag at all: it overwrites in place on a terminal and goes quiet on a pipe,
which is what everyone meant when they asked for progress.

## 4 Every empty state, as one batch

Before this, an empty pane printed `` `manifest` — nothing to show yet``: the pane's name, no cause, no
action. Three rules, all three enforced by `crates/fathom-tui/tests/empty_views.rs`, and all copy in one
file — `crates/fathom-tui/src/empty.rs` — because the batch *is* the deliverable:

1. **The reason comes before the remedy.** *"the treemap is one episode's directories, sized by churn — and
   no episode is selected"* teaches something that is still true after the reviewer presses the key.
2. **The action works from the pane you are standing in**: a keystroke in the merged keymap (the test reads
   the shipped keymap and fails on an unbound one), a `fathom` verb this binary has, or the ticket that owns
   what is missing. The last is a last resort and always says it is one.
3. **The index axis wins when the index is the problem** — absent, unreadable, still building, or finished
   with nothing in it. Every pane is empty then, and no per-view chore fills it; the sentence ends at
   `fathom doctor`, which is a verb rather than a verdict.

The inventory, each line written for a specific state:

| View · pane | What its empty state says |
|---|---|
| Queue · worklist | the queue is the ranked worklist, riskiest first — then `t` lists them in time order, `fathom analyze` prints one delta |
| Queue · card, Timeline · card | the card is one episode, every figure cited — `j`/`k` choose one, `<Enter>` opens its card |
| Timeline · rail | the rail is every episode, newest first — `F` edits the filter that hid them, `fathom stats` counts the index |
| Diff · tree, hunks | the selected episode's files, riskiest first; nothing is selected yet — `q` then `j`/`k` picks an episode, then `d` |
| Structure · treemap, detail | one episode's directories, sized by churn; nothing is selected — `q` then `j`/`k`, then `s` again |
| Graph · canvas, detail | one episode's blast radius; no graph snapshot here yet, and `fathom index` writes one |
| Deps · manifest, tree | one episode's dependency delta, then that requirement's transitive closure — `p` re-reads the selection, `<Enter>` opens the tree |
| Ambient · glance | the newest episode, for a second terminal — `q` picks one, `fathom watch` keeps it current |
| Search · results | nothing has been searched for yet, or nothing matches *the query actually typed* — `/` starts again, and fewer letters find more |
| Artefact · preview, scope | the artefact a `fathom export` would write and the range it covers — `o` here, `fathom export --episodes` headless |
| any pane this file has not met | the motion that works — `j`/`k`, `<Tab>` — and never an invented cause |

Two formatting rules fall out of measuring the copy against the panes that print it, and both are worth
stating because they were both learned by looking at a golden and finding half a sentence missing:

* **the keystroke leads the line.** A pane body clips at its right edge and does not wrap, so copy that
  puts the verb second is copy that gets cut — "`p` re-reads the selection; `fathom stats` counts the
  index" survives a 48-column manifest pane, and the version that began by explaining the pane did not.
* **two facts are two lines.** "the treemap is one episode's directories — and no episode is selected"
  lost its second half to the same clip, which is the half the reviewer had to act on; the purpose and
  the state are separate rows now, and the wrapping rail still gets them as one sentence
  ([`Facts::empty_words`](../../crates/fathom-tui/src/index.rs)).
The one place the miniature is offered inside the frame is the *no index at all* state, beside its remedy:
a reviewer staring at an unindexed repository is exactly the person who has not yet decided whether this is
worth setting up, and the answer is a flag rather than a commit. Every other state assumes they have already
indexed and is about their own history.

## 5 Error copy — every refusal names the next action

The rule: **a user-facing failure states a remedy**, in the same message, on stderr, beside the exit code
that means something (ADR-002 §7.3). Not an explanation, not a stack trace — the thing to do. The audit is
executed, not documented: `crates/fathom-cli/tests/copy.rs` drives the built binary through each failure and
fails if a refusal carries no next action, if the next action is a bare ticket number, or if it names a verb
or flag the binary does not have — a list that file reads out of `fathom --help` rather than maintaining by
hand, so the promise and the binary cannot drift apart.

| Failure | What the message ends with |
|---|---|
| a verb whose body another ticket owns | the ticket, and where the plan is |
| a path that is not a repository | the path, and the fact that a working tree is what is needed |
| no store, and one is required | `fathom index` here, then reopen |
| a store that will not open | `fathom doctor` says what is wrong — only it may decide between "not indexed" and "corrupt" |
| no episode matches the handle | the grammar that does resolve, and `fathom stats` for what exists |
| a repository with no commits | that it has none, and `fathom index` after the first commit |
| an unbuilt analysis behind a verb | what this build does instead (`fathom doctor`), with the owner named in the message rather than passed off as the remedy |
| a run stopped by the deadline or the budget | how far it got, and the verb that continues it — never `100 %` (ADR-004 §9.2) |
| a stalled index | `fathom doctor` says what it is waiting on |

Two rows end at `fathom doctor`, and that is the point: it is the one verb whose job is diagnosis, so every
failure that cannot name a fix in its own words points there instead of guessing at one.

## 6 What each claim is proved by

| Claim | Test |
|---|---|
| clean machine to a frame in under a minute; writes only under the cache directory; warm on the second run; usable with a verb | `crates/fathom-cli/tests/demo.rs` |
| the shipped pack and its manifest agree | `the_shipped_pack_and_its_manifest_agree` in the same file, and `tools/gen_demo_repo.py check` in CI |
| the estimate converges, follows a step change, ignores an outlier, and stays honest when finished, stalled or undenominated — including a phase that opens ten seconds into a run, which is not a phase that has been stalled for ten | `crates/fathom-tui/tests/progress.rs` |
| `index --progress` prints a percentage row, `--no-progress` prints none, `--json` prints none and stays parseable | `crates/fathom-cli/tests/copy.rs` |
| the miniature is indexed by the same verb a reviewer runs themselves, so the demo cannot rot into a picture of the product | the same file's progress-row contract, run twice: once on `fathom index`, once on `fathom --demo --progress` |
| every pane has a specific empty state whose action is a bound key or a real verb, and none falls through to the generic shrug | `crates/fathom-tui/tests/empty_views.rs` |
| no empty state or refusal names a command this build lacks | `crates/fathom-tui/tests/empty_state.rs` against the published list; `crates/fathom-cli/tests/copy.rs` against `fathom --help` |
| the index axis distinguishes its four states and never sends a finished index back to `fathom index` | `crates/fathom-tui/tests/empty_state.rs`, `crates/fathom-cli/tests/tui_data.rs` |
| every refusal ends with a next action | `every_refusal_ends_with_a_next_action` in `crates/fathom-cli/tests/copy.rs` |

## 7 What this deliberately does not do

| Not done | Why |
|---|---|
| a first-run wizard | a wizard is a modal the reviewer escapes before using the tool; the miniature *is* the tool, on history somebody else wrote |
| a progress row for every phase of every verb | one row per task in flight, from the driver that knows its own total. A verb with nothing to count gets an elapsed time and no percentage, and inventing a denominator to fill the row is the lie §3 exists to prevent |
| per-pane copy living with its pane | §4's table and `empty.rs` are one file so the voice can be re-read as one voice; a view module that writes its own empty state is a view module nobody re-read |
| a wizard-style migration or "setup" verb for the store | `fathom index` is the setup, and it is already the thing being measured; a second path to the same state would be a second thing to get wrong |
| translating any of it | no locale is promised; the frame already refuses box characters when the locale says they would render as noise |
