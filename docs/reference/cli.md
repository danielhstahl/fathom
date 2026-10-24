# CLI reference

- **Status**: generated from the built binary, and CI-checked (`e7.5`, **reco-60m**).
- **Generated from**: `fathom --help` and `fathom <verb> [<subverb>] -h`, off a **default-feature**
  build of `fathom-cli` — the shipped product, [ADR-002 §6.1](../adr/0002-workspace.md). Nothing in a
  `GENERATED` block below was typed by hand.
- **Regenerate**: `cargo build -p fathom-cli && python3 tools/gen_docs.py write --page cli`. CI runs
  the `check` form in the matrix leg that has already built the workspace, so a flag added without a
  documentation run is a red build.
- **The prose around these flags** — what each verb is for, what its `--json` contract is, and which
  schema file pins it — is [the headless verbs](../concepts/cli.md). This page is the flag list; that
  page is the argument for it.

One binary, eighteen verbs, one exit-code contract. `fathom` with no verb opens the terminal UI if the
`tui` feature was compiled in ([the cheatsheet](cheatsheet.md) is its keyboard reference); every verb
has a headless form, because ADR-006 §6.7 refuses a fact that only exists on a screen.

```text
<!-- BEGIN GENERATED: usage -->
```text
fathom [OPTIONS] [COMMAND]
```
<!-- END GENERATED: usage -->
```

## The verbs

"This build" is a claim about the program, not about the plan: it is read out of the dispatch in
[`crates/fathom-cli/src/verbs.rs`](../../crates/fathom-cli/src/verbs.rs). A verb that is not built yet
is listed, prints its refusal at runtime, and names the plan item that owns it — which is a pointer,
not a wall, and the reason it appears here rather than being quietly hidden until it works.

<!-- BEGIN GENERATED: verbs -->
| Verb | What it answers | This build | Documented in |
|---|---|---|---|
| **[`index`](#fathom-index)** | Index the repository so every other verb has something to read | shipped | [*onboarding*](../concepts/onboarding.md) · [*cli*](../concepts/cli.md) |
| **[`show`](#fathom-show)** | Show one episode in full | shipped | [*episode*](../concepts/episode.md) · [*cli*](../concepts/cli.md) |
| **[`episodes`](#fathom-episodes)** | List the episodes — the rail's rows, headless (reco-11b) | shipped | [*cli*](../concepts/cli.md) · [*timeline*](../concepts/timeline.md) |
| **[`attribution`](#fathom-attribution)** | Report or correct who Fathom thinks wrote a commit (e4.1) | shipped | [*episode*](../concepts/episode.md) · [*cli*](../concepts/cli.md) |
| **[`review`](#fathom-review)** | The review ledger: sign-offs that outlive the history they were written about (e4.6) | shipped | [*review-ledger*](../concepts/review-ledger.md) · [*cli*](../concepts/cli.md) |
| **[`analyze`](#fathom-analyze)** | Print one episode's delta document — the model the TUI, the export and an agent all read | shipped | [*delta*](../concepts/delta.md) · [*delta-model*](../concepts/delta-model.md) |
| **[`narrative`](#fathom-narrative)** | Read one episode: the narrative every claim in it cites its evidence from (e4.7) | shipped | [*episode-narrative*](../concepts/episode-narrative.md) · [*episode*](../concepts/episode.md) |
| **[`drift`](#fathom-drift)** | What the plan said against what the session did — the contradictions, both sides linked (e4.8) | shipped | [*plan-drift*](../concepts/plan-drift.md) · [*session-state*](../concepts/session-state.md) |
| **[`why`](#fathom-why)** | Print one episode's risk score with every feature, its weight and its contribution (e4.4) | shipped | [*risk-score*](../concepts/risk-score.md) · [*risk-scoring*](../concepts/risk-scoring.md) |
| **[`stats`](#fathom-stats)** | Report repo scale, index state and cache hit rate | shipped | [*cli*](../concepts/cli.md) · [*0004-storage*](../adr/0004-storage.md) |
| **[`check`](#fathom-check)** | Run the checks CI cares about; findings above the threshold exit 7 | shipped | [*ci-gate*](../concepts/ci-gate.md) · [*gate-a-pipeline*](../recipes/gate-a-pipeline.md) |
| **[`doctor`](#fathom-doctor)** | Describe this build, its environment and its index, with a remedy per finding | shipped | [*troubleshooting*](../troubleshooting.md) · [*cli*](../concepts/cli.md) |
| **[`export`](#fathom-export)** | Write a review artefact for a pull request or an audit | shipped | [*export-artefact*](../concepts/export-artefact.md) · [*export-for-a-pr*](../recipes/export-for-a-pr.md) |
| **[`mcp`](#fathom-mcp)** | Speak MCP to a coding agent | shipped | [*0005-harness-contract*](../adr/0005-harness-contract.md) · [*ask-an-agent*](../recipes/ask-an-agent.md) |
| **[`watch`](#fathom-watch)** | Watch the repository and index as it changes | shipped | [*run-beside-the-agent*](../recipes/run-beside-the-agent.md) · [*session-state*](../concepts/session-state.md) |
| **[`ambient`](#fathom-ambient)** | The glance pane for the terminal beside the agent: latest episode, queue, index, one reason (reco-8bx) | shipped | [*ambient-mode*](../concepts/ambient-mode.md) · [*0006-tui*](../adr/0006-tui.md) |
| **[`hooks`](#fathom-hooks)** | Report on the harness event stream: lag, offset, rejections, replay (e6.1) | shipped | [*README*](../harness/README.md) · [*0005-harness-contract*](../adr/0005-harness-contract.md) |
| **[`daemon`](#fathom-daemon)** | The one process per repository that watches, tails the harness stream and indexes (e6.3) | shipped | [*watch-daemon*](../concepts/watch-daemon.md) · [*cli*](../concepts/cli.md) |
| **[`status`](#fathom-status)** | Who is keeping this index current, what it costs, and what is left (e6.3) | shipped | [*watch-daemon*](../concepts/watch-daemon.md) · [*cli*](../concepts/cli.md) |
| **[`tui`](#fathom-tui)** | Print the terminal UI's keymap, or open the UI itself | shipped | [*cheatsheet*](cheatsheet.md) · [*0006-tui*](../adr/0006-tui.md) |
| **[`open`](#fathom-open)** | Open an episode's file at its line in $EDITOR — jump out, do not read around it (e5.8) | shipped | [*handoff*](../concepts/handoff.md) · [*diff-viewer*](../concepts/diff-viewer.md) |
| **[`difftool`](#fathom-difftool)** | Show one file's two sides in your own difftool — difftastic, meld, kdiff3, vim -d (e5.8) | shipped | [*handoff*](../concepts/handoff.md) · [*diff-viewer*](../concepts/diff-viewer.md) |
| **[`cmp`](#fathom-cmp)** | Compare any two episodes or revisions: the composed delta over a range (e5.3) | shipped | [*0006-tui*](../adr/0006-tui.md) · [*delta*](../concepts/delta.md) |
| **[`search`](#fathom-search)** | Search the whole index from one box, in the shell's own ranking (e5.2) | shipped | [*0006-tui*](../adr/0006-tui.md) · [*timeline*](../concepts/timeline.md) |
| **[`architecture`](#fathom-architecture)** | The architecture scorecard: five dimensions, one number, and the drift since your last review (e3.9) | shipped | [*scorecard*](../concepts/scorecard.md) · [*cli*](cli.md) |
| **[`ratchet`](#fathom-ratchet)** | The ratchet: the committed allowance ledger, which tightens on its own and widens only when told to (e3.10) | shipped | [*ratchet*](../concepts/ratchet.md) · [*cycle-detection*](../concepts/cycle-detection.md) |
| **[`context`](#fathom-context)** | What this repository knows about a path or an episode: the recorded decisions and the open questions (e6.5) | shipped | [*review-memory*](../concepts/review-memory.md) · [*review-ledger*](../concepts/review-ledger.md) |
| **[`note`](#fathom-note)** | The review memory: notes, decisions, questions and follow-ups, committed under .fathom/reviews/ (e6.5) | shipped | [*review-memory*](../concepts/review-memory.md) · [*cli*](../concepts/cli.md) |
<!-- END GENERATED: verbs -->

## Global flags

Accepted before or after the verb, and printed verbatim from `fathom --help`. Two of them
(`--plain`, `--lines`) are the accessibility path ADR-006 §6.7 promises; two (`--progress`,
`--no-progress`) are the index's progress row, which is on when stderr is a terminal and off when it
is not, so a log gets one line per update and a pipe gets none.

<!-- BEGIN GENERATED: globals -->
```text
--plain
          Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7).
          
          Chosen automatically when stdout is not a terminal, so a redirected run degrades by itself.

      --no-alt-screen
          Draw in place instead of taking the alternate screen, which keeps the shell's scrollback

      --fresh
          Ignore the session this repository has stored, and start from defaults (`e1.11`).
          
          The escape hatch for a session that came back wrong: a filter that hides the episode you are looking for, a cursor on a row that moved. The stored state is not destroyed — a run that changes nothing writes nothing — it is simply not applied to this one.

      --mouse
          Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1).
          
          Off by default, because capturing the pointer takes the terminal's own text selection away for the length of the run. Every gesture it adds is also a key, and the status line says `mouse on` while it is armed.

      --lines
          With `--plain`: print the frame in reading order, one block per pane, instead of side by side.
          
          Two panes drawn next to each other is the right shape for eyes and the wrong shape for a log, a pipe or a screen reader (ADR-006 §6.7). Same facts, same order, one per line.

      --theme <NAME|PATH>
          Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`).
          
          `dark`, `light` and `ansi` are built in, and all three are measured — for contrast against the background they paint, and for how far the added / removed / violation pairs stay apart when protanopia and deuteranopia are simulated. Anything else is `<name>.toml` in `$FATHOM_THEME_DIR` or `~/.config/fathom/themes/`, or a path. `$FATHOM_THEME` and `[theme] name` in your own config are the same knob one layer down; a repository's `fathom.toml` is deliberately not a layer at all, because a clone does not get to repaint your terminal. With colour off — `NO_COLOR`, or a terminal that has none — the theme is not applied and every state keeps its shape and its word. `fathom tui --list-themes` lists what Fathom can see; `--show-theme` prints the one in force with its measurements, and `--show-theme --audit` exits 7 if a palette fails a floor.

      --demo
          Review a miniature repository that ships inside this binary, then say what to do next (`e5.9`).
          
          The five-minute tour: Fathom unpacks a 21-commit repository into its cache directory, indexes it with the same `fathom index` you would run on your own history, and opens on it. Nothing in the directory you ran it from is read or written, no configuration is read, and deleting `$XDG_CACHE_HOME/fathom` is a complete undo. With a verb the verb runs against the miniature; with none you get the shell.

      --progress
          Print the index's progress row even where stderr is not a terminal (`e5.9`).
          
          On a terminal the row is on by default, one row overwriting itself; this asks for it in a log, a CI step or a `watch -n 1` wrapper, where one line per update is the point. It applies to `fathom index` and to the indexing `fathom --demo` does on your behalf — no other verb has a row to print — and `--json` refuses it, because that stream belongs to the run report and a row beside a machine-readable document is how a parser dies.

      --no-progress
          Never print the index's progress row, even on a terminal (`e5.9`).
          
          The other half of the same choice: a script with its own progress line, or a run whose output is being tee'd somewhere, turns it off explicitly instead of discovering it on.

      --no-watch
          Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`).
          
          An open shell watches its repository by default, so a commit made in another terminal arrives by itself. This turns that off, and the frame then says `snapshot` and what would move it rather than letting a still rail be read as a quiet repository. The other way to get the same picture, cheaper, is to run `fathom watch` in a second terminal and let it own the indexing.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
<!-- END GENERATED: globals -->

## Exit codes

The published contract, parsed from
[`fathom_core::exit::ExitCode`](../../crates/fathom-core/src/exit.rs) — the same enum
[ADR-002 §7.3](../adr/0002-workspace.md) renders. `7` is the number a pipeline branches on; `12` is
the one that means "index first"; `9` is what a `minimal` build answers with when you ask it for the
terminal.

<!-- BEGIN GENERATED: exit -->
| Code | Name | Means |
|---|---|---|
| `0` | `OK` | did the work, nothing to report |
| `1` | `ERROR` | uncategorised failure; the human-readable cause is on stderr with a code |
| `2` | `USAGE` | clap rejected the arguments (bad flag, unknown verb, missing value) |
| `3` | `NOT_A_REPOSITORY` | `<root>` is not a git repository, or `.git` is unreadable |
| `4` | `STORE_CORRUPT` | store failed its integrity check; `fathom doctor` names the rebuild |
| `5` | `CONFIG_INVALID` | `fathom.toml` failed the schema, or a value is the wrong type |
| `6` | `CANCELLED` | user interrupt (ctrl-c) or the writer lease was lost mid-run |
| `7` | `CHECK_VIOLATIONS` | `fathom check` found findings above the configured threshold (CI reads this) |
| `8` | `NETWORK_CONSENT_REQUIRED` | a compiled-in `net-*` feature was asked to run without consent |
| `9` | `FEATURE_ABSENT` | the verb needs a feature that is not compiled in; the message names it |
| `10` | `UNSUPPORTED_PLATFORM` | platform capability missing (ADR-005 §10's ABI probe refused) |
| `11` | `BUDGET_EXCEEDED` | a perf/RSS budget in `benchmarks/budgets.json` was blown |
| `12` | `INDEX_DEGRADED` | the index is missing, incomplete or stale; the verb names the remedy |
<!-- END GENERATED: exit -->

## Reading `--json`

Every verb's machine form is one JSON document on stdout and nothing else on stdout — the reason
`--json` refuses `--progress`. The shapes are checked in under
[`schemas/`](../../schemas) and round-trip-tested in
[`crates/fathom-cli/tests/schemas.rs`](../../crates/fathom-cli/tests/schemas.rs):
`index.schema.json`, `show.schema.json`, `episode-delta.schema.json`, `stats.schema.json`,
`doctor.schema.json`, `hooks.schema.json`, `export.schema.json`, plus `common.schema.json` for the
shared types and `fathom.schema.json` for the config.

Two rules hold across all of them, and both are ADR-001 §7's "no unknown printed as a zero":

* a figure that could not be computed is **absent or `null`, with a gap line naming who owns it** —
  never `0`, never empty-string;
* a document that self-checks says so: `fathom analyze --json` resolves every evidence pointer in
  itself and prints the verdict ([the delta model](../concepts/delta-model.md)).

---

## Each verb, flag by flag

## `fathom index`

Turn git history into the index. Everything else reads that index, so this is the verb that has to run
first — and `fathom --demo` runs it for you on the miniature
([first run](../concepts/onboarding.md)).

<!-- BEGIN GENERATED: help.index -->
```text
Index the repository so every other verb has something to read

Usage: fathom index [OPTIONS] [ROOT]

Arguments:
  [ROOT]  The repository to index [default: .]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --stats              Print the per-analyzer cache table (ADR-004 §4.8)
      --json               Print the run report as JSON on stdout, and nothing else there
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --refresh            Look up every stored unit, and recompute the ones that hit too
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --rebuild            Throw the epoch away and rebuild every analysis from scratch
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --watch              Index once, then keep watching and index as the repository changes
      --committed-only     Do not analyse uncommitted paths (ADR-004 §6.4)
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --prune              Remove the commit rows no ref can reach — what a rebase, an `--amend` or a force-push leaves behind
      --dry-run            With `--prune`: print what would be removed and remove nothing
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.index -->

## `fathom show`

One episode: identity, authorship, message, trailers, and the file rows with their taxonomy decisions.
The handle grammar — episode id, `HEAD~3`, `main`, `v1.2^{}`, an oid prefix — is shared by `show`,
`analyze`, `narrative` and `why`, because a reviewer who has an episode should not have to learn which
verb wants which spelling of it.

<!-- BEGIN GENERATED: help.show -->
```text
Show one episode in full

Usage: fathom show [OPTIONS] <EPISODE>

Arguments:
  <EPISODE>  The episode to show: a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the episode as JSON on stdout, and nothing else there
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.show -->

## `fathom episodes`

The reviewer's main list, without a screen: one row per episode — id, time, author kind, `+add`/`−del`,
path count, review state, session/turn — as text or as `--json` ([`fathom.cli.episodes/1`](../../schemas/episodes.schema.json)).
`fathom show` answers a question about one episode; this answers the question the rail asks about the whole
index, which is what ADR-006 §6.7 means by "every view has a headless equivalent" (`reco-11b`).

The filters are the rail's own — the store's `EpisodeQuery`, the shape the shell filters with — so a count
taken here is the count the shell drew, and `fathom --plain` and this verb are the same read rendered twice.
Three counts are printed and they are three questions: `index.episodes` is what the index holds, `page.matched`
is that filtered, `page.returned` is that paged. A page shorter than the selection is announced, never
silent. `--limit 0` is "every row the filters select"; the shell holds at most 500 rows at a time and this
verb is not a shell, so it says so when a list is longer than a frame could hold.

An empty selection exits 0 with the reason in its notes — a script must be able to ask "how many flagged
episodes?" of a repository with none. No index at all is exit 12 with `fathom index` as the remedy.

<!-- BEGIN GENERATED: help.episodes -->
```text
List the episodes — the rail's rows, headless (`reco-11b`)

Usage: fathom episodes [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the list as one JSON value on stdout (`fathom.cli.episodes/1`), and nothing else there
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --limit <N>          How many rows to print, newest first; `0` is every row the filters select [default: 500]
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --offset <N>         Skip this many rows before printing, for paging with `--limit` [default: 0]
      --author <NAME>      Only episodes by this author, exactly as git recorded the name; repeatable
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --kind <KIND>        Only episodes whose author kind is this — `agent`, `human` or `unknown`; repeatable
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --state <STATE>      Only episodes in this review state: `unread`, `read`, `accepted`, `flagged`, `dismissed`, `snoozed`
      --path <FRAGMENT>    Only episodes that touched a path containing this fragment
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --since <WHEN>       Only episodes whose last commit is at or after this date — `7d`, `2w`, `today`, `yesterday`, `2026-10-01`: the same grammar `fathom search since:` takes
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
      --until <WHEN>       Only episodes whose last commit is before this date, in the same grammar as `--since`
      --include-dead       Include episodes no live commit belongs to — the ghosts a rebase, an amend or a force-push left
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.episodes -->

## `fathom attribution`

Who Fathom thinks wrote a commit, with the evidence, and the correction that is learned
([attribution and authorship](../concepts/episode.md#author-kind-is-a-label-you-can-overrule)). A
correction is keyed to an **identity**, not to an episode, because episode ids move when a lineage is
rewritten and the identity is what you meant.

<!-- BEGIN GENERATED: help.attribution -->
```text
Report or correct who Fathom thinks wrote a commit (`e4.1`)

Usage: fathom attribution [OPTIONS] <COMMAND>

Commands:
  correct  Store a correction: these identities were written by an agent / by a person
  list     List the stored corrections, which are the keys the classifier honours
  forget   Forget one stored correction
  help     Print this message or the help of the given subcommand(s)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.attribution -->

### `fathom attribution correct`

<!-- BEGIN GENERATED: help.attribution.correct -->
```text
Store a correction: these identities were written by an agent / by a person

Usage: fathom attribution correct [OPTIONS] --kind <KIND> [TARGET]

Arguments:
  [TARGET]  The commit whose identities are being corrected: a rev, an oid prefix, or `-` for nothing

Options:
      --plain                Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>          The repository to write to [default: .]
      --kind <KIND>          `agent`, `human` or `unknown`
      --no-alt-screen        Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --agent <AGENT>        Which agent the identity is, when `--kind agent`
      --fresh                Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse                Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --note <NOTE>          Why the label was overruled, for whoever reads it next
      --identity <IDENTITY>  Correct one identity key directly (`identity:<address>`), instead of deriving them from a commit
      --lines                With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --json                 Print the result as JSON on stdout, and nothing else there
      --theme <NAME|PATH>    Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress             print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress          never print the index's progress row, even on a terminal
      --no-watch             Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help                 Print help (see more with '--help')
  -V, --version              Print version
```
<!-- END GENERATED: help.attribution.correct -->

### `fathom attribution list`

<!-- BEGIN GENERATED: help.attribution.list -->
```text
List the stored corrections, which are the keys the classifier honours

Usage: fathom attribution list [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the corrections as JSON on stdout, and nothing else there
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.attribution.list -->

### `fathom attribution forget`

<!-- BEGIN GENERATED: help.attribution.forget -->
```text
Forget one stored correction

Usage: fathom attribution forget [OPTIONS] <IDENTITY>

Arguments:
  <IDENTITY>  The identity key to forget, as `fathom attribution list` prints it

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to write to [default: .]
      --json               Print the result as JSON on stdout, and nothing else there
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.attribution.forget -->

## `fathom review`

The [review ledger](../concepts/review-ledger.md): sign-offs that outlive the history they were written
about. `fathom review` with no subverb is `review status`, which is the question a pipeline asks.

<!-- BEGIN GENERATED: help.review -->
```text
The review ledger: sign-offs that outlive the history they were written about (`e4.6`)

Usage: fathom review [OPTIONS] [COMMAND]

Commands:
  status  The verdict: is every episode covered by a sign-off that still matches (exit `7` when it is not)
  sign    Sign an episode off, appending one entry to `.fathom/reviews/ledger.md`
  sync    Render the queue's acts that the ledger has never seen
  list    List the ledger's entries with their verdicts
  verify  Re-hash every entry against its own `digest:` line — tampering, and nothing else
  help    Print this message or the help of the given subcommand(s)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.review -->

### `fathom review status`

<!-- BEGIN GENERATED: help.review.status -->
```text
The verdict: is every episode covered by a sign-off that still matches (exit `7` when it is not)

Usage: fathom review status [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json               Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.review.status -->

### `fathom review sign`

<!-- BEGIN GENERATED: help.review.sign -->
```text
Sign an episode off, appending one entry to `.fathom/reviews/ledger.md`

Usage: fathom review sign [OPTIONS] <EPISODE>

Arguments:
  <EPISODE>  The episode to sign off, or an unambiguous prefix of its id

Options:
      --decision <DECISION>  What the decision is; `accepted` is the one that means *reviewed* [default: accepted]
      --plain                Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json                 Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)
      --no-alt-screen        Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --note <NOTE>          A note, committed with the entry and folded onto one line
      --fresh                Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --identity <IDENTITY>  Who is signing, when git config and `fathom.toml` do not say
      --agent <AGENT>        Record the act as an agent's verdict on someone's behalf, not as a human acceptance
      --mouse                Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines                With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>    Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress             print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress          never print the index's progress row, even on a terminal
      --no-watch             Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help                 Print help (see more with '--help')
  -V, --version              Print version
```
<!-- END GENERATED: help.review.sign -->

### `fathom review sync`

<!-- BEGIN GENERATED: help.review.sync -->
```text
Render the queue's acts that the ledger has never seen

Usage: fathom review sync [OPTIONS]

Options:
      --dry-run            Report what would be rendered, write nothing
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json               Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.review.sync -->

### `fathom review list`

<!-- BEGIN GENERATED: help.review.list -->
```text
List the ledger's entries with their verdicts

Usage: fathom review list [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --state <STATE>      Show only entries in this state: `fresh`, `stale`, `orphan` or `unknown`
      --json               Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.review.list -->

### `fathom review verify`

<!-- BEGIN GENERATED: help.review.verify -->
```text
Re-hash every entry against its own `digest:` line — tampering, and nothing else

Usage: fathom review verify [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json               Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.review.verify -->

## `fathom analyze`

One episode's [delta document](../concepts/delta.md) — the seven-section, evidence-carrying model the
TUI card, the export and an agent all read, unchanged. `--schema` prints the contract itself, and
`--schema --check` is how CI keeps the checked-in schema from drifting from the types
([the version policy](../concepts/delta-model.md)).

<!-- BEGIN GENERATED: help.analyze -->
```text
Print one episode's delta document — the model the TUI, the export and an agent all read

Usage: fathom analyze [OPTIONS] [EPISODE]

Arguments:
  [EPISODE]  The episode to analyse: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix. Omitted with `--schema`, which prints the contract instead of an answer

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the delta document as JSON on stdout, and nothing else there
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --schema             Print `schemas/episode-delta.schema.json`, the JSON Schema for the document, and stop
      --check              Check the checked-in schema against what this build generates; the CI form of `--schema`
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --write              Write the generated schema to `schemas/`, replacing the checked-in copy
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.analyze -->

## `fathom narrative`

The prose a human skims, with a citation behind every claim and a watermark saying no model wrote it
([episode narrative](../concepts/episode-narrative.md)). `--attribution` answers the question a reader
always ends up asking: was a language model in this path? The honest answer is no, and the flag prints
the gates that keep it that way.

<!-- BEGIN GENERATED: help.narrative -->
```text
Read one episode: the narrative every claim in it cites its evidence from (`e4.7`)

Usage: fathom narrative [OPTIONS] [EPISODE]

Arguments:
  [EPISODE]  The episode to read: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix. Omitted with `--attribution`, which answers a question about the build instead of an episode

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the claims, citations and weights as JSON on stdout, and nothing else there
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --markdown           Print the Markdown artefact body — the exact text `fathom export` will carry
      --explain            Print the ranking beside the prose: every claim's weight, so the order can be argued with
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --top <N>            How many claims the *Read this first* paragraph holds (1 to 6)
      --attribution        Print whether a model wrote any of this, and the five gates a polish path must clear. Needs no index and no episode
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.narrative -->

## `fathom drift`

What the session's recorded intent said, against what its episodes did — six kinds of contradiction,
each printed as a **pair** with both sides linked
([plan versus execution](../concepts/plan-drift.md)). A dismissal is keyed to the rule and the item, so
it survives the rewrite that would otherwise eat the finding.

<!-- BEGIN GENERATED: help.drift -->
```text
What the plan said against what the session did — the contradictions, both sides linked (`e4.8`)

Usage: fathom drift [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --session <S>        Compare one harness session only — its id, or `lineage:refs/heads/main` for the episodes no session was named for
      --all                Read every episode in the index instead of the newest ones. A long read, and a longer report
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --window <N>         How many episodes past a plan item to look for the change that did it
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --min-overlap <F>    The token overlap at which a plan item counts as addressed (0.05 to 1.0)
      --max <N>            The cap on the report
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --dismiss <KEY>      A suppression key already argued about; its contradiction is counted, not re-reported. Repeatable
      --off <KIND>         Switch one kind off, in the published spelling (`plan-unimplemented`, …). Repeatable
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --json               Print the report as JSON on stdout, and nothing else there
      --no-progress        never print the index's progress row, even on a terminal
      --fail-on-findings   Exit 7 when anything was found, so a CI step can gate on it
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.drift -->

## `fathom why`

The risk score as an auditable sum: every feature, its weight, its contribution, the reason behind
each, and the axes the model could not measure ([risk score](../concepts/risk-score.md)).
`--weight NAME=VALUE` is a what-if, `--calibrate` is the corpus that turns retuning from a feeling into
a number.

<!-- BEGIN GENERATED: help.why -->
```text
Print one episode's risk score with every feature, its weight and its contribution (`e4.4`)

Usage: fathom why [OPTIONS] [EPISODE]

Arguments:
  [EPISODE]  The episode to score: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix. Omitted with `--calibrate`, which reports on the weights instead of on an episode

Options:
      --plain                Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>          The repository to read [default: .]
      --json                 Print the same answer as JSON, for a machine
      --no-alt-screen        Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --calibrate [<SET>]    Print the calibration report — precision and recall at the top of the queue, as shipped and as retuned — instead of a score. With a path, that JSON set is measured; without one, the set shipped with the binary
      --fresh                Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse                Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --weight <NAME=VALUE>  Override one weight for this run only: `--weight findings=0.25`. Repeatable. Nothing is written to `fathom.toml`; the score says which weights were overridden
      --lines                With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --tau <TAU>            Override the flag line for this run only, 0 to 100
      --cut <FRACTION>       How deep the queue is, as a fraction, for `--calibrate`. Defaults to 0.1 — the top ten percent
      --theme <NAME|PATH>    Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --weights              Print the feature names this build can score, and stop
      --progress             print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress          never print the index's progress row, even on a terminal
      --no-watch             Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help                 Print help (see more with '--help')
  -V, --version              Print version
```
<!-- END GENERATED: help.why -->

## `fathom stats`

Repository scale, index state and cache hit rate — the three numbers that tell you whether what you are
looking at is a small answer or a missing one.

<!-- BEGIN GENERATED: help.stats -->
```text
Report repo scale, index state and cache hit rate

Usage: fathom stats [OPTIONS] [ROOT]

Arguments:
  [ROOT]  The repository to report on [default: .]

Options:
      --json               Print the same report as JSON, for a machine
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.stats -->

## `fathom check`

The CI gate: six checks over a `base..head` range, one exit code, five formats. Findings at or above
`[check] fail_on` exit `7`; the policy is `[check]` in `fathom.toml`, and `--explain` prints what is in
force. [The CI gate page](../concepts/ci-gate.md) is the design, and
[the pipeline recipe](../recipes/gate-a-pipeline.md) is the fifteen-minute YAML.

<!-- BEGIN GENERATED: help.check -->
```text
Run the checks CI cares about; findings above the threshold exit 7

Usage: fathom check [OPTIONS]

Options:
      --plain                   Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>             The repository to check [default: .]
      --base <REV>              The far end of the range: what the change is compared against
      --no-alt-screen           Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh                   Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --head <REV>              The near end of the range. Defaults to `[check] head`, then to `HEAD`
      --mouse                   Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --range <BASE..HEAD>      The range as one argument, `base..head` — what a reviewer pastes into a message
      --format <FMT>            The output format: `human`, `json`, `sarif`, `github` (step annotations) or `markdown` (job summary) [default: human]
      --lines                   With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --json                    Print the JSON document — the same thing as `--format json`
      --theme <NAME|PATH>       Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --out <PATH>              Write the artefact to this file instead of to stdout
      --progress                print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --summary <PATH>          Where to append the markdown job summary. Defaults to `$GITHUB_STEP_SUMMARY` when the runner set one
      --no-progress             never print the index's progress row, even on a terminal
      --no-summary              Do not write a job summary at all, even with `$GITHUB_STEP_SUMMARY` set
      --no-annotations          Do not print GitHub step annotations beside the report
      --no-watch                Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
      --fail-on <LEVEL>         The severity at or above which the run exits 7: `note`, `warning`, `error` or `never`
      --severity <CHECK=LEVEL>  Set one check's volume for this run, e.g. `--severity tests=off`. Repeatable
      --only <CHECK>            Run only these checks. Repeatable; the others are reported as off, not omitted
      --skip <CHECK>            Do not run these checks. Repeatable
      --require <CHECK>         Checks that must *run*: one that cannot is a failure rather than a notice. Repeatable
      --ignore <CHECK[:RULE]>   Silence one finding or family for this run: `tests`, or `layering:no_upward`. Repeatable
      --max <N>                 Cap the findings listed per check (the counts stay whole). Overrides `[check] max_findings`
      --explain                 Print the policy in force — every check's volume, every threshold — and exit 0 without checking
  -h, --help                    Print help (see more with '--help')
  -V, --version                 Print version
```
<!-- END GENERATED: help.check -->

## `fathom doctor`

The build, the environment, the repository and the index, each finding with a remedy. This is the page
to run first when something is off ([troubleshooting](../troubleshooting.md)), and `--json` is the one
object that is safe to attach to a bug report.

<!-- BEGIN GENERATED: help.doctor -->
```text
Describe this build, its environment and its index, with a remedy per finding

Usage: fathom doctor [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --privacy            Print the network-feature and consent table
      --json               Print the same report as JSON, for a machine
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --deep               Run the whole-database `integrity_check` rather than `quick_check` (ADR-004 §9.6)
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --strict             Exit with the code of the worst finding instead of 0 — what CI and a pre-flight check want
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --root <ROOT>        The repository to report on [default: .]
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.doctor -->

## `fathom export`

The [review artefact](../concepts/export-artefact.md): one record, rendered as markdown, HTML or JSON,
byte-identical when re-exported. `--strict` exits `7` while an operator-action item is open, which is
the half a merge gate can use.

<!-- BEGIN GENERATED: help.export -->
```text
Write a review artefact for a pull request or an audit

Usage: fathom export [OPTIONS]

Options:
      --format <FORMAT>      The artefact format: markdown, html or json [default: markdown]
      --plain                Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen        Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --out <OUT>            The file stem to write, or a file name that already carries the format's extension [default: fathom-review]
      --episodes <EPISODES>  The episode range to export, e.g. `HEAD~9..HEAD`, one commit, or one episode id [default: HEAD]
      --fresh                Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse                Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --root <ROOT>          The repository to read [default: .]
      --json                 Print the report as JSON on stdout, and nothing else there
      --lines                With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --stdout               Write the artefact to stdout instead of a file; the report goes to stderr
      --theme <NAME|PATH>    Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --strict               Exit 7 while the artefact's checklist still has an open item — the form a CI gate wants
      --progress             print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress          never print the index's progress row, even on a terminal
      --no-watch             Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help                 Print help (see more with '--help')
  -V, --version              Print version
```
<!-- END GENERATED: help.export -->

## `fathom mcp`

The agent-facing surface. `fathom mcp serve` speaks MCP over **stdio** — no socket, so no consent key — and
answers the same questions the rail answers, from the same versioned delta model, through six read tools:
`explain_episode`, `changes_between`, `architecture_delta`, `dependency_delta`, `risk_report`,
`search_history`. Every answer carries `cost_ms`, `bytes_returned`, `degraded` and `unknowns`; a page that
will not fit in `mcp.max_response_bytes` is cut and handed back with a cursor ([the contract is
ADR-005 §4](../adr/0005-harness-contract.md)). No file body and no secret leaves a response: the same
redaction table that runs at ingest runs again on the way out, and a body-shaped field is refused rather
than trimmed. [the recipe for an agent that wants to ask](../recipes/ask-an-agent.md).

The two write tools — `mark_reviewed`, `add_review_note` — are **off by default** and absent from
`tools/list` while off, so a client never reaches for a door that will not open. An operator turns them on
with `mcp.allow_writes = true`; from then on every call to them appends an audit line naming the tool, a
digest of its arguments and its outcome. `mark_reviewed` additionally needs a token that only one change can
use, minted by `fathom mcp approval-token` out of a secret in your own config — an agent cannot sign off its
own work, and a token it obtained for one change does not open the next.

<!-- BEGIN GENERATED: help.mcp -->
```text
Speak MCP to a coding agent

Usage: fathom mcp [OPTIONS] <COMMAND>

Commands:
  serve           Serve the tool surface over stdio, which opens no socket (`e6.4`)
  tools           Print the tool surface and its bounds as this repository's config resolves them
  approval-token  Mint the token that authorises `mark_reviewed` for exactly one change
  help            Print this message or the help of the given subcommand(s)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.mcp -->

### `fathom mcp serve`

<!-- BEGIN GENERATED: help.mcp.serve -->
```text
Serve the tool surface over stdio, which opens no socket (`e6.4`)

Usage: fathom mcp serve [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.mcp.serve -->

What it offers, and what the config made of it, without starting a session:

### `fathom mcp tools`

<!-- BEGIN GENERATED: help.mcp.tools -->
```text
Print the tool surface and its bounds as this repository's config resolves them

Usage: fathom mcp tools [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository whose `[mcp]` config and index to read [default: .]
      --json               The surface as one JSON document on stdout (`fathom.mcp.surface/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.mcp.tools -->

Each tool is listed with the arguments it takes, its page bound and whether this repository's config offers
it at all. `--json` is the same surface for a script, and it is the one place that says whether an approval
secret is configured — which is why it is a flag and not the default.

### `fathom mcp approval-token`

<!-- BEGIN GENERATED: help.mcp.approval-token -->
```text
Mint the token that authorises `mark_reviewed` for exactly one change

Usage: fathom mcp approval-token [OPTIONS] <CHANGE_ID>

Arguments:
  <CHANGE_ID>  The change to bind the token to: an episode id or a commit

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository whose `[mcp]` config holds the approval secret [default: .]
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.mcp.approval-token -->

Prints the token for exactly one change, derived from `mcp.approval_token` in **your** config (never in the
repository, so a clone cannot mint one). Hand it to the agent, or paste it into the call yourself;
`mark_reviewed` refuses anything else, including a token minted for a different change.

## `fathom watch`

Index as commits land — the mode that makes the second terminal stay current while the agent works
([run Fathom beside the agent](../recipes/run-beside-the-agent.md)). `fathom index --watch` is the same
thing as a one-shot with an initial index, which is the form a script wants.

<!-- BEGIN GENERATED: help.watch -->
```text
Watch the repository and index as it changes

Usage: fathom watch [OPTIONS] [ROOT]

Arguments:
  [ROOT]  The repository to watch [default: .]

Options:
      --json                         Print one JSON object per state change on stdout, and nothing else there
      --plain                        Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen                Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --once                         Index once, then exit: what a script or a CI check wants from `watch`
      --fresh                        Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --max-cycles <MAX_CYCLES>      Stop after this many looks at the filesystem
      --deadline <DEADLINE>          Stop after this many seconds
      --mouse                        Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --backend <BACKEND>            Which change source to use: `auto`, `notify` or `poll` [default: auto]
      --lines                        With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --interval-ms <INTERVAL_MS>    The debounced polling interval in milliseconds
      --theme <NAME|PATH>            Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --debounce-ms <DEBOUNCE_MS>    The quiescence window in milliseconds: how long the filesystem must be quiet before a change is read
      --heartbeat-ms <HEARTBEAT_MS>  The liveness beat in milliseconds: how often the state is rewritten while nothing changes
      --progress                     print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --chunk-units <CHUNK_UNITS>    Analysis units per index chunk — the watcher's slice of the CPU [default: 2000]
      --no-progress                  never print the index's progress row, even on a terminal
      --no-watch                     Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
      --worktree                     Also re-index uncommitted paths when only the index moved
      --no-status-file               Do not write `.fathom/watch/status.json`
      --quiet                        Print nothing to stdout but errors; the state machine still goes to the status file
  -h, --help                         Print help (see more with '--help')
  -V, --version                      Print version
```
<!-- END GENERATED: help.watch -->

## `fathom status`

Who is keeping this index current, what it costs, and what is left — the headless half of
[the watch daemon](../concepts/watch-daemon.md). With a daemon running it is that process's own health
document (pid, uptime, protocol, the queue's `pushed`/`coalesced`/`shed`/`served`, the measured CPU share
against its budget, the harness stream's offset and lag, the pass counters by origin); without one it is the
writer lease and the watcher's heartbeat instead, with `daemon.source` saying which of the four things the
answer is (`daemon`, `lease`, `heartbeat`, `none`). `--index` asks for a pass first and reports what that pass
did, which is why the health document describes the state *after* it. Exits `12` for a lease left behind by a
writer that is gone, and `0` for a repository that is merely idle: `daemon.running` is the field to branch on.

<!-- BEGIN GENERATED: help.status -->
```text
Who is keeping this index current, what it costs, and what is left (`e6.3`)

Usage: fathom status [OPTIONS] [ROOT]

Arguments:
  [ROOT]  The repository to report on [default: .]

Options:
      --json               Print the report as one JSON line (machine contract `fathom.cli.status/1`)
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --socket <PATH>      Ask the daemon at this endpoint instead of the derived one
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --index              Ask for a pass and wait for it, then report the state after it — the way to make `fathom status` mean "up to date" rather than "as of whenever"
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.status -->

## `fathom daemon`

The one writer per repository: it takes the writer lease, watches, tails the harness stream and indexes on a
budget, and answers the other verbs over a local socket instead of each of them starting a second copy of the
same work (ADR-004 §7.1). Bare `fathom daemon` is a status report; `start` and `stop` are the two things a
supervisor needs. `--foreground` runs it in this process, which is the form systemd, a container `init` and a
CI step want — stdout stays attached and SIGTERM leaves by the same path `fathom daemon stop` uses. A second
`start` refuses and names the holder rather than starting a second writer.

<!-- BEGIN GENERATED: help.daemon -->
```text
The one process per repository that watches, tails the harness stream and indexes (`e6.3`)

Usage: fathom daemon [OPTIONS] [COMMAND]

Commands:
  start   Take the writer lease and watch this repository from this process, or from a detached one
  stop    Ask the daemon to finish its pass, checkpoint and release the lease
  status  The daemon's own health report (`fathom status` is the same question at the top level)
  help    Print this message or the help of the given subcommand(s)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.daemon -->

### `fathom daemon start`

The budget flags are the ticket's: `--cpu-share-pct` is the ceiling the loop is measured against (25 % of one
core by default, 5 % is the watcher's quiet number), `--slice` and `--yield-slice` are the work it takes per
pass while nothing is asking and while a client has just been heard, `--queue-depth` bounds how many distinct
askers may wait before new ones are refused with a reason, and `--hook-bytes` caps the harness stream read per
pass — the lag it produces is reported as `hooks.lag_bytes` rather than dropped (ADR-005 §6.5). Without
`--foreground` the parent re-executes detached and waits for the socket to answer before printing.

<!-- BEGIN GENERATED: help.daemon.start -->
```text
Take the writer lease and watch this repository from this process, or from a detached one

Usage: fathom daemon start [OPTIONS]

Options:
      --plain                          Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>                    The repository to watch and index [default: .]
      --json                           Print the health document as JSON instead of lines, and print it once the first pass has run
      --no-alt-screen                  Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh                          Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --quiet                          Print nothing but the health document; the heartbeat file is still written
      --foreground                     Run in this process, so a supervisor can supervise it (`e6.3`, ADR-004 §7.1)
      --mouse                          Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines                          With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --socket <PATH>                  Where to listen, instead of the endpoint derived from the repository (ADR-004 §7.2)
      --theme <NAME|PATH>              Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --require-socket                 Refuse to start rather than run with no IPC endpoint
      --progress                       print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --supervisor <SUPERVISOR>        Who is supervising this daemon, recorded in the health document (`systemd`, `docker`, `ci`)
      --cpu-share-pct <CPU_SHARE_PCT>  The CPU ceiling, as a percent of one core. 25 is ADR-004 §7.2's background default [default: 25]
      --no-progress                    never print the index's progress row, even on a terminal
      --no-watch                       Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
      --slice <SLICE>                  Analysis units per pass while nothing is talking to the daemon [default: 2000]
      --yield-slice <YIELD_SLICE>      Analysis units per pass while a foreground client has just been heard — the yield slice [default: 50]
      --queue-depth <QUEUE_DEPTH>      How many distinct index requests may wait before new ones are refused rather than queued [default: 64]
      --hook-bytes <HOOK_BYTES>        Harness-stream bytes per pass — the back-pressure knob of ADR-005 §6.5 [default: 1048576]
      --backend <BACKEND>              Which change source to use: `auto`, `notify` or `poll`. The daemon is a watcher, so it takes the watcher's flags for the same meaning [default: auto]
      --interval-ms <INTERVAL_MS>      The debounced polling interval in milliseconds
      --debounce-ms <DEBOUNCE_MS>      The quiescence window in milliseconds
      --heartbeat-ms <HEARTBEAT_MS>    The liveness beat in milliseconds, for the heartbeat file and the lease
      --worktree                       Also re-index uncommitted paths when only git's index moved
      --no-status-file                 Do not write `.fathom/watch/status.json`
      --deadline <DEADLINE>            Stop after this many seconds — the supervisor's `TimeoutStopSec` seam and a test's
      --max-cycles <MAX_CYCLES>        Stop after this many loop passes. A test seam, and the way to use the daemon as a one-shot indexer
  -h, --help                           Print help (see more with '--help')
  -V, --version                        Print version
```
<!-- END GENERATED: help.daemon.start -->

### `fathom daemon stop`

Finish the current pass, checkpoint the store, blank the lease record, remove the socket, exit 0. This is the
same exit path Ctrl-C and SIGTERM take; the difference is only who asks.

<!-- BEGIN GENERATED: help.daemon.stop -->
```text
Ask the daemon to finish its pass, checkpoint and release the lease

Usage: fathom daemon stop [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository whose daemon to stop [default: .]
      --json               Print the outcome as one JSON line (`fathom.cli.daemon.stop/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --socket <PATH>      Ask the daemon at this endpoint instead of the derived one
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --reason <REASON>    Who is asking, recorded in the daemon's last line [default: "fathom daemon stop"]
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.daemon.stop -->

### `fathom daemon status`

The same report as `fathom status`, reachable as a subverb so `fathom daemon` and `fathom daemon status`
cannot disagree about which repository or which endpoint was asked.

<!-- BEGIN GENERATED: help.daemon.status -->
```text
The daemon's own health report (`fathom status` is the same question at the top level)

Usage: fathom daemon status [OPTIONS] [ROOT]

Arguments:
  [ROOT]  The repository to report on [default: .]

Options:
      --json               Print the report as one JSON line (machine contract `fathom.cli.status/1`)
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --socket <PATH>      Ask the daemon at this endpoint instead of the derived one
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --index              Ask for a pass and wait for it, then report the state after it — the way to make `fathom status` mean "up to date" rather than "as of whenever"
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.daemon.status -->

## `fathom ambient`

The glance pane on its own, for the second terminal beside an agent: latest episode and its risk, how much
is waiting, whether the index is current, and the one reason to look now — in a pane that fits 80×12 and
refuses below 40×3 ([ambient mode](../concepts/ambient-mode.md),
[ADR-006 §5.7](../adr/0006-tui.md)). It redraws only when what it would say has changed, so a quiet
repository costs a `stat` and a ref read per poll rather than a screenful of bytes; `--self-check` prints
the polls, the redraws and the measured CPU share of the run and exits **11** if it blew the 1 %-of-a-core
budget. `<Enter>` hands the terminal to the full view with that episode selected; `--json` streams one
object per change under the `fathom.cli.ambient/1` contract, and `--once` prints the same facts once with
no terminal taken.

<!-- BEGIN GENERATED: help.ambient -->
```text
The glance pane for the terminal beside the agent: latest episode, queue, index, one reason (`reco-8bx`)

Usage: fathom ambient [OPTIONS]

Options:
      --root <ROOT>                The repository to read [default: .]
      --json                       Print the pane as one JSON object per change (machine contract `fathom.cli.ambient/1`)
      --no-alt-screen              Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh                      Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --once                       Print the pane once and exit; never take the screen
      --mouse                      Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --plain                      Draw the pane as text with no terminal taken (ADR-006 §6.7)
      --interval-ms <INTERVAL_MS>  How often to look, in milliseconds. 400 is what meets the "under a second" promise at near-zero cost [default: 400]
      --lines                      With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --deadline <DEADLINE>        Stop after this many seconds
      --theme <NAME|PATH>          Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --no-index                   Never index from the pane: read whatever the last index run left, whoever ran it
      --chunk-units <CHUNK_UNITS>  Episodes to index per slice when this pane is the one keeping the index current [default: 200]
      --progress                   print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress                never print the index's progress row, even on a terminal
      --width <WIDTH>              Render at this width instead of the terminal's — for a pipe, a test, or a tmux status line
      --no-watch                   Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
      --self-check                 Run to the deadline and print what the pane cost: polls, redraws and this process's own CPU share. Exits `11` if it blew the budget of 1 % of a core
  -h, --help                       Print help (see more with '--help')
  -V, --version                    Print version
```
<!-- END GENERATED: help.ambient -->

## `fathom hooks`

The harness event stream: lag, durable offset, counts by type, why each refused line was refused, and a
deterministic replay ([ADR-005](../adr/0005-harness-contract.md),
[the adapters](../harness/README.md)). `--check` is the default and does not consume the stream, so a
monitor loop can poll it.

<!-- BEGIN GENERATED: help.hooks -->
```text
Report on the harness event stream: lag, offset, rejections, replay (`e6.1`)

Usage: fathom hooks [OPTIONS] [ROOT]

Arguments:
  [ROOT]  The repository whose `.fathom/events.jsonl` to read [default: .]

Options:
      --events <EVENTS>         Read this events file instead of the configured one (ADR-005 §3.1's first choice)
      --plain                   Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --check                   Report the stream and change nothing. This is the default; the flag exists so a script can say what it means
      --no-alt-screen           Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh                   Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --replay                  Rebuild everything from byte 0 and rewrite the offset: what an index rebuild does first
      --mouse                   Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --tail                    Read forward from the durable offset, advancing it, and print what arrived
      --lines                   With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --max <MAX>               With `--tail`: print at most this many records
      --theme <NAME|PATH>       Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --trailer <SESSION:TURN>  Print the `Agent-Session:` / `Agent-Turn:` trailer pair for `<session>:<turn-index>`
      --json                    Print the same report as JSON on stdout, and nothing else there
      --progress                print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --strict                  Exit 7 when the stream is not healthy — the form a CI gate wants
      --no-progress             never print the index's progress row, even on a terminal
      --no-watch                Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help                    Print help (see more with '--help')
  -V, --version                 Print version
```
<!-- END GENERATED: help.hooks -->

## `fathom tui`

Either open the terminal UI, or print what it would do: `--show-keys` prints the merged keymap
(defaults plus your `keys.overrides`), `--show-session` the stored session state
([session state](../concepts/session-state.md)), `--show-theme` / `--list-themes` the palette in force
and its measurements ([theming](../concepts/theming.md)). All four take no terminal, which is what makes
them safe to run from inside a bug report.

<!-- BEGIN GENERATED: help.tui -->
```text
Print the terminal UI's keymap, or open the UI itself

Usage: fathom tui [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --show-keys          Print the merged keymap — defaults plus your `keys.overrides` — and exit, taking no terminal
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --show-session       Print the session state stored for this repository, and exit, taking no terminal (`e1.11`)
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --json               With `--show-session`: the rows as JSON on stdout, and nothing else there
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --stats              Print the run's own frame numbers — draws, events, p99, worst, over-budget — as one JSON object on stdout when the shell gives the screen back (`reco-08r`)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --show-theme         Print the theme this run would paint with, and exit, taking no terminal (`e5.6`)
      --audit              With `--show-theme`: treat a failed contrast or separation floor as an error exit (exit 7)
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --list-themes        Print every theme Fathom can see — the built-ins and the files in your theme directories — and exit, taking no terminal (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.tui -->

## `fathom open`

The exit out of the reader: one file of one episode, opened at its line in `$EDITOR`
([the exits out of the reader](../concepts/handoff.md)). The handle grammar is the one `show`, `analyze`
and `why` use, with no argument meaning `HEAD`'s episode. Which file, which line and which program are
all decided before anything is started and all three are printed — the line comes from the file's first
`@@` header, the file from the largest change on a surface a review reads (with the surface printed, so
a surprising choice is a visible one), and the program from `--editor`, then `$FATHOM_EDITOR`/`$VISUAL`/`$EDITOR`,
then `[ui] editor` in the user config. A repository's `fathom.toml` gets no vote: a clone does not name a
program this process executes. With no editor installed this is a stated path, not a crash, and `--print`
prints the plan either way; the editor's own exit code is the verb's, because `:cq` is a reviewer who
cancelled.

<!-- BEGIN GENERATED: help.open -->
```text
Open an episode's file at its line in `$EDITOR` — jump out, do not read around it (`e5.8`)

Usage: fathom open [OPTIONS] [EPISODE]

Arguments:
  [EPISODE]  The episode to jump into: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix — the same grammar `fathom show`, `analyze` and `why` accept. With no argument, `HEAD`

Options:
      --plain                     Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>               The repository to read [default: .]
      --no-alt-screen             Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --path <PATH>               Which of the episode's files to open, by path or by a unique suffix of it
      --fresh                     Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --line <N>                  Which line, overriding the line the first `@@` header says
      --editor <PROGRAM [ARGS…]>  Which editor to use for this run, overriding `$FATHOM_EDITOR`, `$VISUAL` and `$EDITOR`
      --mouse                     Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines                     With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --print                     Print the command and the reason for it, and run nothing
      --list                      List every file the episode touched with the line each would jump to, and open nothing
      --theme <NAME|PATH>         Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --json                      Print the decision as JSON on stdout, and nothing else there
      --progress                  print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress               never print the index's progress row, even on a terminal
      --no-watch                  Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help                      Print help (see more with '--help')
  -V, --version                   Print version
```
<!-- END GENERATED: help.open -->

## `fathom difftool`

The heavyweight diff, for the reviewer who wants two real files in their own tool rather than the pane.
The two sides come out of the object store into `$TMPDIR/fathom-difftool-…/` with the file's real
extension kept (difftastic picks its grammar by suffix), and the tool is asked in the shape it documents
— `difft`, `meld`, `kdiff3`, `nvim -d`, `code --diff`, WinMerge with `/e`, or any program you name, which
gets the two files as its last two arguments. A tool that is not installed names itself, lists what was
looked for, and leaves the pair on disk, so the review continues in whatever viewer exists
([the exits out of the reader](../concepts/handoff.md#3-fathom-difftool-the-heavyweight-diff)).

<!-- BEGIN GENERATED: help.difftool -->
```text
Show one file's two sides in your own difftool — difftastic, meld, kdiff3, `vim -d` (`e5.8`)

Usage: fathom difftool [OPTIONS] [EPISODE]

Arguments:
  [EPISODE]  The episode whose file to show: the same handle grammar as every other verb. With no argument, `HEAD`

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --path <PATH>        Which of the episode's files to show, by path or by a unique suffix of it
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --tool <NAME>        Which difftool to use, overriding `$FATHOM_DIFFTOOL` and your user config
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --print              Print the command and the reason for it, and run nothing. The two files stay on disk
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --list               List every file the episode touched with the line each would jump to, and show nothing
      --keep               Leave the two staged files behind and say where they are
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --json               Print the decision as JSON on stdout, and nothing else there
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.difftool -->

## `fathom cmp`

Any two episodes or revisions, side by side in one answer: the delta **composed over the range between**
them, from the same episode documents `fathom analyze` prints — not a second diff engine
([the delta model](../concepts/delta.md)). The endpoints are the same handles every verb takes, so a
comparison is written the way it was found: `HEAD~9..HEAD`, `77aa0c..e8f1a2`, `main@1a2b3c..feat/x@9f8e7d`.
`--save NAME` keeps it, `--note` says why, and the name is then a spec in the next process, in the shell's
`,` palette, and in `fathom cmp NAME` ([the shell's scope rules](../adr/0006-tui.md)).

<!-- BEGIN GENERATED: help.cmp -->
```text
Compare any two episodes or revisions: the composed delta over a range (`e5.3`)

Usage: fathom cmp [OPTIONS] [SPEC]

Arguments:
  [SPEC]  The comparison: `HEAD~9..HEAD`, `77aa0c..e8f1a2`, `main@1a2b3c..feat/x@9f8e7d`, or a saved name

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the comparison as one JSON line on stdout (`fathom.compare/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --save <NAME>        Keep this comparison under a name, so the next person says `since-the-refactor` instead of typing the endpoints
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --note <NOTE>        Why the name exists, kept with it
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --list               List the saved comparisons and exit
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.cmp -->

## `fathom search`

The same box `/` is in the shell, in a pipe: one query over episodes, paths, symbols, dependencies and
findings, ranked by the scorer the shell uses, with the filter grammar (`author:`, `since:`, `risk>`,
`path:`, `dep:`) and the same list of what it could not read. A kind the build cannot search is named in
the report rather than left out, because "nothing matched" and "nothing was searched" are different
sentences ([the rail and the box](../concepts/timeline.md)). `--save NAME` keeps a query for the palette;
`--list` prints what this repository has.

<!-- BEGIN GENERATED: help.search -->
```text
Search the whole index from one box, in the shell's own ranking (`e5.2`)

Usage: fathom search [OPTIONS] [QUERY]...

Arguments:
  [QUERY]...  The query, with the same filter tokens `/` takes (`author:`, `since:`, `risk>`, `path:`, `dep:`, …)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the report as one JSON line on stdout (`fathom.search/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --limit <LIMIT>      How many hits to print [default: 20]
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --save <NAME>        Keep this query under a name, so the `,` palette can offer it again
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --list               List the saved searches and exit
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.search -->

## `fathom architecture`

The five-dimension architecture scorecard, in a pipe: boundaries, coupling, acyclicity, API hygiene and
hotspot pressure, combined by an arithmetic whose printed terms add to its printed total, plus the trend
over the index's scored revisions and the delta since *your* last review — anchored on the revision your
own review ended at, from `review_state`, not from a timestamp somebody guessed at
([the scorecard](../concepts/scorecard.md)). A dimension that cannot be measured refuses, with its reason,
and its weight leaves the denominator: no layering rules is not a clean bill of rules. `alert` is a reported
flag, never an exit code — `fathom check` owns pass and fail. An index with no module graph exits 12 with the
remedy, because a score of 0 computed from nothing would be a false claim.

<!-- BEGIN GENERATED: help.architecture -->
```text
The architecture scorecard: five dimensions, one number, and the drift since your last review (`e3.9`)

Usage: fathom architecture [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the report as one JSON line on stdout (`fathom-architecture/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --revision <TREE>    Score this revision rather than the newest the graph holds: a full tree oid or an unambiguous prefix
      --backfill <N>       Score this many unscored revisions first, newest first, before reporting (`0`: report what is stored) [default: 0]
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --history [<N>]      How many scored revisions the trend reads; bare, or `0`, is every row the index holds [default: 0]
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --refresh            Forget every stored scorecard first, and re-derive what this run needs
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --read-only          Do not write anything: read the stored rows and score only what this run needs
      --explain            Print the knobs in force — the weights, the curves and their published constants — and exit
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.architecture -->

## `fathom ratchet`

The ratchet, in the shell: what the committed ledger at `.fathom/ratchet.toml` forgives, which of it is
running out, and the news that no allowance covers. The ledger tightens on its own — a baselined violation
that has gone away has its row deleted by the next bare run — and it never widens on its own: raising it is
a separate act that requires `--update-baseline`, a non-empty `--reason` and a named owner, and it leaves a
diff and a review-memory note behind, so the exception is argued rather than absorbed
([the ratchet](../concepts/ratchet.md)). An expiring allowance is warned about while it still has days; an
allowance whose day has gone by is debt back due. `ratchet.mode = deny` is what makes an uncovered
violation exit 7, and `warn` and `off` report without failing. An index with no module graph exits 12:
with no graph there is no list of violations to read the ledger against, and a run that reported "nothing
uncovered" would be reporting a fact it never checked.

<!-- BEGIN GENERATED: help.ratchet -->
```text
The ratchet: the committed allowance ledger, which tightens on its own and widens only when told to (`e3.10`)

Usage: fathom ratchet [OPTIONS]

Options:
      --plain                 Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>           The repository to read [default: .]
      --json                  Print the report as one JSON line on stdout (`fathom-ratchet/1`)
      --no-alt-screen         Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh                 Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --update-baseline       Raise the baseline: write the violations that have no allowance as new rows
      --mouse                 Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --reason <REASON>       Why the baseline is being raised — required by `--update-baseline`, recorded in the file, the diff and the review memory
      --lines                 With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --owner <OWNER>         Who owns the new allowances; bare, the reviewer's own identity (`--identity`, `$FATHOM_REVIEWER`, `git config user.name`)
      --granted-by <WHO>      Who is asking, recorded beside the owner as `granted_by`; bare, the same identity as `--owner`
      --theme <NAME|PATH>     Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --expires <DATE|never>  The last day an allowance is live, `YYYY-MM-DD`, or `never` for a row with no expiry
      --key <KEY>             Grant only this key (repeatable): a violation key as the report prints it, or `kind=key`
      --progress              print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --dry-run               Report what would change and write nothing — not even the automatic tightening
      --no-progress           never print the index's progress row, even on a terminal
      --no-watch              Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
      --revision <TREE>       Judge this graph revision rather than the newest the index holds: an oid or an unambiguous prefix
      --no-note               Do not write a `decision` note into the review memory when the baseline is raised
  -h, --help                  Print help (see more with '--help')
  -V, --version               Print version
```
<!-- END GENERATED: help.ratchet -->

## `fathom context`

Answer *"what does this repository already know about this?"* for a path or an episode: the recorded
decisions with their reasons, the questions and follow-ups still open, and the ledger's dispositions over
the same episodes — each with its author, its date and the evidence it cites. See
[the review memory](../concepts/review-memory.md).

<!-- BEGIN GENERATED: help.context -->
```text
What this repository knows about a path or an episode: the recorded decisions and the open questions (`e6.5`)

Usage: fathom context [OPTIONS] <TARGET>

Arguments:
  <TARGET>  The path or episode to ask about. A path may be a file or a directory fragment

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the answer as one JSON line on stdout (`fathom.cli.context/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --all                Include answered and withdrawn notes in the lists, not just the live ones
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.context -->

## `fathom note`

The review memory: notes, decisions, questions and follow-ups, appended as markdown blocks under
`.fathom/reviews/notes.md` and mirrored into the index. Appending is the only edit: an answer names the
note it answers, a withdrawal names the note it retracts, and every block hashes itself.

<!-- BEGIN GENERATED: help.note -->
```text
The review memory: notes, decisions, questions and follow-ups, committed under `.fathom/reviews/` (`e6.5`)

Usage: fathom note [OPTIONS] [COMMAND]

Commands:
  add       Record a note, decision, question or follow-up against an episode or a path
  list      List what is recorded, oldest first
  show      Show one note with its evidence and its link verdict
  answer    Answer a question or close a follow-up by appending the note that closes it
  withdraw  Take a recorded decision back, by appending the note that retracts it
  verify    Re-hash every note against its own `digest:` line — tampering, and nothing else
  help      Print this message or the help of the given subcommand(s)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --root <ROOT>        The repository to read [default: .]
      --json               Print the report as one JSON line on stdout (`fathom.cli.note/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.note -->

### `fathom note list`

<!-- BEGIN GENERATED: help.note.list -->
```text
List what is recorded, oldest first

Usage: fathom note list [OPTIONS] [TARGET]

Arguments:
  [TARGET]  Only notes attached to this episode or touching this path

Options:
      --kind <KIND>        Only this kind: `decision`, `question`, `follow-up`, `annotation`
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json               Print the report as one JSON line on stdout (`fathom.cli.note/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --status <STATUS>    Only notes in this status: `open`, `answered`, `withdrawn`
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --limit <LIMIT>      How many to print, oldest first; `0` is every note [default: 0]
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.note.list -->

### `fathom note add`

<!-- BEGIN GENERATED: help.note.add -->
```text
Record a note, decision, question or follow-up against an episode or a path

Usage: fathom note add [OPTIONS] [TEXT]

Arguments:
  [TEXT]  The text of the note. One line in the file either way: paragraph breaks are folded, never wrapped

Options:
      --episode <EPISODE>  The episode to attach it to, or an unambiguous prefix of its id
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json               Print the report as one JSON line on stdout (`fathom.cli.note/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --path <PATH>        The path to attach it to, optionally `path:line`. Resolved to the newest episode that touched it when the index knows one
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --kind <KIND>        What kind of note this is: `decision`, `question`, `follow-up`, `annotation` (`note` and `flag` are accepted as `annotation`) [default: annotation]
      --author <AUTHOR>    Who is writing, when git config and `fathom.toml` do not say
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --agent <AGENT>      Record the note as an agent's act on someone's behalf, not as a human's
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --about <ABOUT>      The note this one answers, by `nt-…` id
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.note.add -->

### `fathom note show`

<!-- BEGIN GENERATED: help.note.show -->
```text
Show one note with its evidence and its link verdict

Usage: fathom note show [OPTIONS] <ID>

Arguments:
  <ID>  The note id (`nt-…`)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json               Print the report as one JSON line on stdout (`fathom.cli.note/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.note.show -->

### `fathom note answer`

<!-- BEGIN GENERATED: help.note.answer -->
```text
Answer a question or close a follow-up by appending the note that closes it

Usage: fathom note answer [OPTIONS] <ID> <TEXT>

Arguments:
  <ID>    The question or follow-up being answered (`nt-…`)
  <TEXT>  The answer

Options:
      --author <AUTHOR>    Who is answering
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --agent <AGENT>      Record the answer as an agent's act on someone's behalf
      --json               Print the report as one JSON line on stdout (`fathom.cli.note/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --kind <KIND>        The kind of the answering note: a `decision` is an answer that settles it, an `annotation` is an answer that informs it [default: decision]
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.note.answer -->

### `fathom note withdraw`

<!-- BEGIN GENERATED: help.note.withdraw -->
```text
Take a recorded decision back, by appending the note that retracts it

Usage: fathom note withdraw [OPTIONS] <ID> <TEXT>

Arguments:
  <ID>    The note being retracted (`nt-…`)
  <TEXT>  Why it no longer holds

Options:
      --author <AUTHOR>    Who is retracting it
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --agent <AGENT>      Record the retraction as an agent's act on someone's behalf
      --json               Print the report as one JSON line on stdout (`fathom.cli.note/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.note.withdraw -->

### `fathom note verify`

<!-- BEGIN GENERATED: help.note.verify -->
```text
Re-hash every note against its own `digest:` line — tampering, and nothing else

Usage: fathom note verify [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --json               Print the report as one JSON line on stdout (`fathom.cli.note/1`)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
      --no-watch           Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.note.verify -->

---

**Next**: [the cheatsheet](cheatsheet.md) for the keys · [the detector catalogue](../detectors/index.md)
for the findings those verbs report · [the feature set](features.md) for what a given binary was compiled
with.
