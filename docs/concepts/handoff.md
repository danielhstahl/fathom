# The exits out of the reader

- **Status**: shipped by `reco-dmw` (`e5.8`).
- **In code**: the plans are [`fathom_core::handoff`](../../crates/fathom-core/src/handoff.rs) and
  [`fathom_core::links`](../../crates/fathom-core/src/links.rs); the running is
  [`fathom_cli::handoff`](../../crates/fathom-cli/src/handoff.rs),
  [`fathom_cli::open`](../../crates/fathom-cli/src/open.rs) and
  [`fathom_cli::links`](../../crates/fathom-cli/src/links.rs). The keys inside the diff view are
  `E` in [`keymap.rs`](../../crates/fathom-tui/src/keymap.rs), described in
  [the diff viewer §7](diff-viewer.md#7-motion-and-the-three-ways-out).
- **Read this if**: you want to know what `fathom open` will run before it runs it, why a link did not
  appear, or where the boundary of the product is drawn.

Fathom is a reader. The reviewer reads the episode here and then goes somewhere else to act on it —
an editor, a difftool, a browser. That boundary is
[the thesis's own refusal to be an IDE](thesis.md): no buffer, no formatter, no write into the working
tree. A refusal of that kind is only worth having if the exits across it are good, so this page is about
three exits and the one rule they share: **the plan is printed before it is run, and the absence of a
tool is a stated path rather than a broken one.**

## 1 The plan lives below the IO line

An editor handoff is three facts — which program, which arguments, why those — and none of them needs a
process to answer. So they are computed in `fathom-core`, which reads no environment, opens no file and
spawns nothing ([ADR-002 §3](../adr/0002-workspace.md)), and the answer is a value:

```rust
pub enum Editor {
    Open(Command),                       // program, args, and a `why` sentence
    Unavailable { why: String },         // "no editor", said as a path forward
}
```

`fathom-cli` is the composition root: it reads `$FATHOM_EDITOR` and the user config, asks `PATH` what
exists, and hands the result to the planner as an `Env` of two closures — a variable lookup and a
"can I run this" answer. That split buys the whole test matrix: `Os` is a parameter, so
vim/neovim/helix/code/zed/sublime/emacs/idea/nano on Linux, macOS and Windows are asserted in one
`cargo test` on one machine, on a fake `PATH` that has exactly the programs a named case needs
([`tests/handoff.rs`](../../crates/fathom-core/tests/handoff.rs)). The `fathom-tui` diff view installs
the same `Env` from the CLI at startup and produces the same `Command` for the same keystroke, so the
shell verb and the key cannot disagree about what "open this file at this line" means.

`Command::why` is not a debug string. "It opened at the top" and "nothing happened" are the two failures
a reviewer cannot diagnose from outside, so the sentence names both halves of the decision — where the
program came from, and which line syntax that program documents:

```
  asked     commit b999c15
  target    app.py at line 1
  editor    nvim +27 src/core.py
  why       from $EDITOR · `nvim` takes `+LINE` before the path
```

## 2 `fathom open`: the jump

```bash
fathom open                 # HEAD's episode — "what did I just change"
fathom open HEAD~3          # or a commit: the same handle grammar as `show`, `why`, `narrative`
fathom open ep-4f2b9c1 --path src/store.rs
fathom open @ --print       # the plan, and nothing run
fathom open @ --list        # every file the episode touched, with the line each would jump to
fathom open @ --json        # the same decision as a contract, for a script that has no eyes
```

Three decisions, each printed:

* **Which file.** An episode that touched eleven files is not one jump. `--path` matches exactly or by
  a unique suffix, and an ambiguous suffix is a usage error that prints the candidates rather than
  guessing. With no `--path`, the choice is the biggest change to a file a review would read: not a
  deletion, not a binary, not a surface the path taxonomy has already discounted (`dependency`,
  `generated`, `vendored`, `binary`). That ranking asks [`risk::classify`](../concepts/paths.md), the
  same precedence chain `fathom why` ranks with, so "the file this commit is about" is one definition
  with one implementation — and the chosen surface is printed beside the path, because a ranker that
  shows its reasoning is a ranker a reviewer can overrule.
* **Which line.** The first `@@` header of the file's first hunk, new-side start. That is the line the
  hunk pane's `]` walks to, so the two surfaces agree by construction rather than by two parses. A
  file with no recorded hunks opens at the top and says so; `--line N` overrides, and the override is
  named in the printed plan.
* **Which program.** Below, in §4.

The editor's exit code is the verb's. `vim :cq` is a reviewer who cancelled, and a `fathom open` that
reported success after that would be reporting a review that did not happen. A signal is said out loud
too (`killed by SIGKILL`, usually an OOM), because "it exited 1" would blame the editor.

**The hand-off is the terminal, so a run with no terminal has nothing to hand.** What the editor is given
is this process's own stdin, stdout and stderr — which is how `vim` gets to draw, and the reason a
`fathom open` whose streams are pipes (`| tee`, a CI step, a command substitution) can sit there forever:
the editor polls a stream that is not a terminal for input that will never arrive, and the verb waits for
the editor. That is what `--print` is for. It prints the identical command, runs nothing, and works with
nothing installed and no terminal at all; the line it prints is then yours to run where the terminal is.
An editor that needs none — `code`, `zed`, anything that starts a GUI and returns — is unaffected.

Nothing in the working tree is written, ever (§1 of the diff viewer's rules applies here too: ADR-001 §7.5). The path handed to the editor is the file as checked out, absolute, so a relative `$PWD` never
makes the jump land somewhere else.

## 3 `fathom difftool`: the heavyweight diff

`difftastic`, `meld`, `kdiff3`, `nvim -d` and friends all want two files on disk. Fathom has the two
sides of every file already — the object ids the index recorded — so it writes them:

```bash
fathom difftool HEAD~1                    # one file, one tool, already staged
fathom difftool @ --path src/core.py --tool meld
fathom difftool @ --print --keep          # the plan, and the files stay behind
```

The pair goes into `$TMPDIR/fathom-difftool-<pid>-<oid>/` as `before-core.py` and `after-core.py`, with
the file's **real extension kept**, because difftastic chooses its grammar by suffix and a
`store.before` gets no grammar at all. The directory is removed when the verb ends unless `--keep` (or
`--print`) says otherwise, and where the pair is gets printed either way.

Each tool is asked in the shape *it* documents, from a closed table
([`DIFFTOOLS`](../../crates/fathom-core/src/handoff.rs)): `difftastic` runs as `difft A B`, `meld` and
`kdiff3` and `bcompare` take the two files and nothing else, the vi family gets its diff flag
(`nvim -d A B`, `vim -d A B`), VS Code and its forks get `code --diff A B`, WinMerge gets `/e` so it does
not lock the files. The list is closed on purpose: an argument shape guessed at is a tool that opens the
wrong side twice. Anything else is one variable away from working, because an unrecognised name is used
as the program itself — arguments included — with the two files last:

```bash
fathom difftool @ --tool "mydiff --click --wait"     # mydiff --click --wait A B
```

**A difftool that is not installed is a stated fallback, not a substitution.** The refusal names the tool
that was asked for, lists the ones that were looked for, points at the keys that change it, and prints
where the two files are — so the review continues in whatever viewer exists. That is the acceptance
criterion this page exists to satisfy: *no editor or no difftool is a normal, well-explained path*. A
machine with nothing installed is not a broken machine; `fathom analyze` and the `d` pane read the same
bytes with nothing to install at all.

## 4 Where a program name may come from

```text
--editor / --tool                     what was asked for on this command line
$FATHOM_EDITOR, $VISUAL, $EDITOR      fathom's own knob, then git's precedence for the shell's
$FATHOM_DIFFTOOL                      the difftool's own knob
~/.config/fathom/config.toml          [ui] editor, [diff] difftool
the platform default                  `vi`, or the first candidate on PATH
```

`$FATHOM_EDITOR` exists so that setting an editor for Fathom does not rename the shell's editor for
everything else; `[ui] editor` in the user config is the same knob one layer down. `--editor` is
described as *"asked for"* rather than as a variable, because a plan that says `from $EDITOR` about a
value nobody exported is the beginning of a bug report that cannot be finished.

Which line syntax the chosen program wants is its own decision, from the same kind of table: the vi
family (including `kak`, `nano`, `micro`) takes `+LINE` before the path, `hx`/`zed`/`subl` take
`path:LINE`, the VS Code family takes `--goto path:LINE`, the JetBrains family takes `--line LINE path`,
`emacs*` takes `+LINE`, and a program none of these is *gets the path alone and opens at the top* — with
that fact printed in the plan, so the reviewer learns it before the editor is up rather than after.

**A repository's `fathom.toml` is not in that list.** [ADR-001 §4.6](../adr/0001-product-thesis.md) treats
clone-provided config as hostile input, and a clone naming a program your machine executes is a worse
version of a clone choosing a theme your eyes have to read. A repository that tries is told why:

```
fathom: fathom.toml's `[ui] editor = "curl"` was ignored: a clone does not get to name a program
        this process runs. Put it in ~/.config/fathom/config.toml, or in the environment, instead.
```

The attempt is reported rather than suppressed, for the reason
[config §5](config.md#5-a-cloned-fathomtoml-is-hostile-input) already gives: a silently ignored value
looks like a bug in the layering. See [configuration](config.md#32-the-user-config).

## 5 OSC-8 hyperlinks, and the two questions that decide them

A path in Fathom's output is text a reviewer should be able to *click*, and a commit should be able to
open in a browser. OSC-8 (`ESC ] 8 ; params ; URI ST`) is the mechanism, and its failure mode is
nastier than not doing it: a terminal that does not interpret the introducer prints its bytes inline,
which corrupts the very line the reviewer was reading. So the rule is conservative and it is written
down in the output:

> **Emit only when the terminal has said it wants them.**

There are two separate questions, and conflating them is how this feature goes wrong:

1. **Does the terminal advertise OSC-8?** Either `TERM_PROGRAM` names one with a version floor —
   iTerm2 ≥ 3.0, VS Code's terminal ≥ 1.57, Konsole ≥ 22.12, WezTerm/Ghostty/contour/Hyper with no floor
   because every version they have shipped does — or `VTE_VERSION` ≥ 0.36 covers the whole VTE family
   (gnome-terminal, xfce4-terminal, terminator) by version-parsing a dotted number rather than by a name
   list, or a marker variable is present (`KITTY_WINDOW_ID`, `ALACRITTY_WINDOW_ID`, `WEZTERM_PANE`,
   `GHOSTTY_RESOURCES_DIR`, `WT_SESSION`). Absence of advertising is absence of support: there is no
   probe that asks, because a query would need the screen handed back.
2. **Is the thing being written to a terminal at all?** The environment describes the session; only the
   caller knows whether *this* stdout reaches it. `fathom show | fzf`, `fathom show > notes.md` and a CI
   log all sit inside a terminal that advertises, and each of them would receive introducers as payload.
   A stream that is not a terminal gets plain text, whatever it advertises.

Between them sits the multiplexer, which is a second veto rather than a second opinion: tmux forwards
hyperlinks from 3.3 and GNU screen never has, so an advertisement on the far side of either is not a
route. tmux does not always say which version it is, and an unknown tmux is refused with its version
named in the sentence — the case worth a line on stderr, because there the reviewer's expectation was
reasonable and only the route was missing. `fathom doctor` carries the whole verdict as
`.build.terminal.hyperlinks`: `emit`, `advertised`, and `why`.

`FATHOM_HYPERLINKS` is the reviewer's own override, and it beats both questions: `always` emits even
where nothing advertised and even into a pipe (for the pipe a person is reading), `never` closes it,
`auto` (the default) asks. An unrecognised value is reported as unrecognised rather than read as `auto`.
The verdict is inspectable without a terminal emulator at hand:

```bash
fathom doctor --json | jq '.build.terminal.hyperlinks'
# { "emit": false, "advertised": true, "why": "the terminal advertises OSC-8, but this is not a terminal, …" }
```

**Where links appear.** Anywhere a path or a commit is *printed as a line the reviewer reads*: `fathom
show`'s file list, `fathom open --list`, the `asked`/`target` lines of the two handoff verbs, and a
commit's short oid when there is an `origin` remote to point at (the forge URL is built from the remote
with its credentials stripped; a repository with no remote gets seven characters and no link, which is
what it always got). Links are **not** drawn inside the alternate-screen frame: the frame's cells carry
style attributes owned by the diff engine, and a link that changes colour is a lie about what the theme
decided ([theming](theming.md)). In the frame the exits are keys — `y`, `Y`, `E` — and the OSC-8 story is
a story about text output.

The visible text never changes. A link is the same bytes with something wrapped around them, which is
what makes `--json`, a pipe, and a terminal that said no all print the same path — and it is what the
tests assert, because "the text is unchanged" is the property every other part of this depends on.

## 6 How it is tested

* [`fathom-core/tests/handoff.rs`](../../crates/fathom-core/tests/handoff.rs) — the editor matrix (four
  editors the ticket names and five more, four `Os` values, one process), the difftool argument shapes,
  "asked for and missing" naming what was tried, the empty-`Env` case that is a sentence and not a panic,
  and the line-syntax rules for editors that have none.
* [`fathom-core/tests/links.rs`](../../crates/fathom-core/tests/links.rs) — the advertisement probe per
  terminal family, the version floors, tmux and screen with and without passthrough, the pipe inside an
  advertising terminal, the override, and the URL builders including the credential-stripping case.
* [`fathom-cli/tests/handoff_verb.rs`](../../crates/fathom-cli/tests/handoff_verb.rs) — the shipped
  binary against a fixture repository it indexed itself: the jump line asserted against the `@@` header
  rather than against a substring, an editor that really runs (`fathom --version` is the stand-in: it
  exists on all three platforms because we built it, and exits non-zero on a bad flag, which is how
  "the editor's exit is the verb's" gets tested without a TTY), the two refusals, the refused clone
  config, the staged difftool pair's bytes read back out of the object store, and the escape sequences
  present under the knob and absent without it.
* [`fathom-tui/tests/diff_behaviour.rs`](../../crates/fathom-tui/tests/diff_behaviour.rs) — `E`'s plan
  per OS, through the same planner the verbs use.
