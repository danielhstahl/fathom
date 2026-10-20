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

One binary, sixteen verbs, one exit-code contract. `fathom` with no verb opens the terminal UI if the
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
| **[`attribution`](#fathom-attribution)** | Report or correct who Fathom thinks wrote a commit (e4.1) | shipped | [*episode*](../concepts/episode.md) · [*cli*](../concepts/cli.md) |
| **[`review`](#fathom-review)** | The review ledger: sign-offs that outlive the history they were written about (e4.6) | shipped | [*review-ledger*](../concepts/review-ledger.md) · [*cli*](../concepts/cli.md) |
| **[`analyze`](#fathom-analyze)** | Print one episode's delta document — the model the TUI, the export and an agent all read | shipped | [*delta*](../concepts/delta.md) · [*delta-model*](../concepts/delta-model.md) |
| **[`narrative`](#fathom-narrative)** | Read one episode: the narrative every claim in it cites its evidence from (e4.7) | shipped | [*episode-narrative*](../concepts/episode-narrative.md) · [*episode*](../concepts/episode.md) |
| **[`drift`](#fathom-drift)** | What the plan said against what the session did — the contradictions, both sides linked (e4.8) | shipped | [*plan-drift*](../concepts/plan-drift.md) · [*session-state*](../concepts/session-state.md) |
| **[`why`](#fathom-why)** | Print one episode's risk score with every feature, its weight and its contribution (e4.4) | shipped | [*risk-score*](../concepts/risk-score.md) · [*risk-scoring*](../concepts/risk-scoring.md) |
| **[`stats`](#fathom-stats)** | Report repo scale, index state and cache hit rate | shipped | [*cli*](../concepts/cli.md) · [*0004-storage*](../adr/0004-storage.md) |
| **[`check`](#fathom-check)** | Run the checks CI cares about; findings above the threshold exit 7 | *refuses: names its plan item* | [*gate-a-pipeline*](../recipes/gate-a-pipeline.md) · [*cli*](cli.md) |
| **[`doctor`](#fathom-doctor)** | Describe this build, its environment and its index, with a remedy per finding | shipped | [*troubleshooting*](../troubleshooting.md) · [*cli*](../concepts/cli.md) |
| **[`export`](#fathom-export)** | Write a review artefact for a pull request or an audit | shipped | [*export-artefact*](../concepts/export-artefact.md) · [*export-for-a-pr*](../recipes/export-for-a-pr.md) |
| **[`mcp`](#fathom-mcp)** | Speak MCP to a coding agent | *refuses: names its plan item* | [*0005-harness-contract*](../adr/0005-harness-contract.md) · [*ask-an-agent*](../recipes/ask-an-agent.md) |
| **[`watch`](#fathom-watch)** | Watch the repository and index as it changes | shipped | [*run-beside-the-agent*](../recipes/run-beside-the-agent.md) · [*session-state*](../concepts/session-state.md) |
| **[`hooks`](#fathom-hooks)** | Report on the harness event stream: lag, offset, rejections, replay (e6.1) | shipped | [*README*](../harness/README.md) · [*0005-harness-contract*](../adr/0005-harness-contract.md) |
| **[`tui`](#fathom-tui)** | Print the terminal UI's keymap, or open the UI itself | shipped | [*cheatsheet*](cheatsheet.md) · [*0006-tui*](../adr/0006-tui.md) |
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
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
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
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.show -->

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
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.stats -->

## `fathom check`

The CI gate verb. **Not in this build**: it answers with a refusal naming `e6.6`, and
[the pipeline recipe](../recipes/gate-a-pipeline.md) is the set of gates that work today
(`fathom review status`, `fathom export --strict`, `fathom drift --fail-on-findings`).

<!-- BEGIN GENERATED: help.check -->
```text
Run the checks CI cares about; findings above the threshold exit 7

Usage: fathom check [OPTIONS]

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
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
  -h, --help                 Print help (see more with '--help')
  -V, --version              Print version
```
<!-- END GENERATED: help.export -->

## `fathom mcp`

The agent-facing surface. **The server is not in this build**: `fathom mcp serve` refuses with the plan
item that owns it (`e6.4`), and [the recipe for an agent that wants to ask](../recipes/ask-an-agent.md)
uses `fathom analyze --json` instead, which is the same document through a different door.

<!-- BEGIN GENERATED: help.mcp -->
```text
Speak MCP to a coding agent

Usage: fathom mcp [OPTIONS] <COMMAND>

Commands:
  serve  Serve the tool surface over stdio, which opens no socket
  help   Print this message or the help of the given subcommand(s)

Options:
      --plain              Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)
      --no-alt-screen      Draw in place instead of taking the alternate screen, which keeps the shell's scrollback
      --fresh              Ignore the session this repository has stored, and start from defaults (`e1.11`)
      --mouse              Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.mcp -->

### `fathom mcp serve`

<!-- BEGIN GENERATED: help.mcp.serve -->
```text
Serve the tool surface over stdio, which opens no socket

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
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.mcp.serve -->

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
      --worktree                     Also re-index uncommitted paths when only the index moved
      --no-status-file               Do not write `.fathom/watch/status.json`
      --quiet                        Print nothing to stdout but errors; the state machine still goes to the status file
  -h, --help                         Print help (see more with '--help')
  -V, --version                      Print version
```
<!-- END GENERATED: help.watch -->

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
      --show-theme         Print the theme this run would paint with, and exit, taking no terminal (`e5.6`)
      --audit              With `--show-theme`: treat a failed contrast or separation floor as an error exit (exit 7)
      --lines              With `--plain`: print the frame in reading order, one block per pane, instead of side by side
      --list-themes        Print every theme Fathom can see — the built-ins and the files in your theme directories — and exit, taking no terminal (`e5.6`)
      --theme <NAME|PATH>  Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)
      --progress           print the index's progress row even when stderr is not a terminal (`index` and `--demo`)
      --no-progress        never print the index's progress row, even on a terminal
  -h, --help               Print help (see more with '--help')
  -V, --version            Print version
```
<!-- END GENERATED: help.tui -->

---

**Next**: [the cheatsheet](cheatsheet.md) for the keys · [the detector catalogue](../detectors/index.md)
for the findings those verbs report · [the feature set](features.md) for what a given binary was compiled
with.
