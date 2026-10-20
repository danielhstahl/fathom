# The diff viewer

Status: built by `reco-0q7` (stage **reconcile**). Code: `crates/fathom-tui/src/diff/`.
Surface: `frame.pane(kind: diff, panes: [tree, hunks])` in `crates/fathom-tui/src/frame.rs`,
bindings in `src/recommender/tui/keymap.py` (the source of truth, generated into
`crates/fathom-tui/src/keymap.rs` and `docs/adr/0006-tui.md`).

The diff viewer is where a recommendation is checked. The recommender says *this file, this episode,
because of these signals*; the diff is the evidence, and a reviewer who cannot read the evidence
cannot disagree with the ranking. Everything below follows from one rule: **the pane never lies and
never goes quiet.** A file with nothing to show says what is missing and what to do about it; a file
whose text is huge, binary, deleted or merely absent gets a sentence, not a blank rectangle.

## 1 What it reads, and what it does not

The viewer never opens the object store, never shells out to `git`, never reads a file, a variable or a
`PATH`, and never starts a process: `fathom-tui` is IO-free (ADR-002 §3, lint `W1`). Patch bytes arrive
through one trait, implemented wherever the bytes actually live — today that is `fathom-cli`, reading
the store on the episode's behalf (`reco-0kw`); a future adapter would shell out to `git diff`:

```rust
pub trait PatchSource {
    fn origin(&self) -> String;                    // names itself: "fathom diff --episode 0:12"
    fn open(&self) -> Result<Box<dyn PatchReader>, Unavailable>;
}
pub trait PatchReader {
    fn read_at(&mut self, offset: u64, want: usize) -> Result<Vec<u8>, Unavailable>;
    fn len(&self) -> Option<u64>;
}
```

`read_at` is positioned, so a window over a 400 MiB patch reads the window and nothing else. Two
implementations exist today: `BytesSource` (bytes already in memory — the shell's own, and tests') and
`NoSource` (there is nothing to read, and the pane prints why instead of blanking). The reason the
product path is in-memory is one seam up: a `PatchProvider` (`frame::Options::patches`) turns *the
episode under review* into a `PatchSource`, and `fathom-cli` installs one — `crates/fathom-cli/src/patches.rs`
resolves the selected episode to its member commits in the index, renders the net old→new blob pair
per file, and hands the pane a patch whose hunks carry `X-Fathom-Class`, `X-Fathom-Noise` and
`X-Fathom-Skip` headers. The bytes are the ones the store's `git_commit_file` rows point at, so `d`
reads the episode, not a re-run of `git` (`reco-0kw`). A `git diff` source would be the same shape.

`Unavailable` is a typed reason with an optional `hint`, and the hint is part of the type, not a
string someone remembers to append: `not available offline — episode predates the local packfile`
printed without `run: fathom fetch --episode 0:12` is a dead end, and ADR-006 §6.3 forbids dead ends.

The same inversion applies to everything the pane would rather not do itself. The environment the copy
and editor plans are written against arrives as an `Env` of two closures — a variable lookup, and "can
this program run here" — installed by `fathom-cli` at startup (`Options::handoff_env`). With none
installed a copy plan falls through to OSC 52 and says so, which is a stated answer rather than a wrong
one.

The viewer also does not compute anything about the file: taxonomy class, episode state, evidence
spans and review state arrive as arguments (`ClassAt`, `StateAt`) from the shell that already read
them through `fathom_analyze::insight`. The TUI still holds no store handle (§11 of ADR-006).

## 2 The budget, and why 50 000 lines is not a special case

`Budget` (`budget.rs`) is one struct of documented caps. Every one of them is a decision, not a guess
at a machine:

| Cap | Default | What it bounds | Why |
|---|---:|---|---|
| `chunk_bytes` | 128 KiB | one read from the source | big enough to cover a pane's worth of hunks in one syscall, small enough that a bad guess costs 128 KiB |
| `max_line_bytes` | 8 KiB | one *decoded* line held at a time | past this a line is data, not a line: it is cut, marked `⋯`, and the pane says so |
| `index_bytes` | 16 MiB | the whole index (file rows + hunk rows + retained annotations) | the number that keeps a large patch openable on a small machine |
| `window_bytes` | 1 MiB | one rendered window's decoded text | a frame builds in a frame budget; a window is text that must be drawable in one pass |
| `window_lines` | 4096 | source lines one window build may read | a backstop against pathological hunk shapes, not the normal limit |
| `max_files` | 20 000 | file rows | beyond it the pane is a list nobody can read |
| `max_hunks` | 20 000 | hunk rows | same, for the file with 8 000 one-line hunks |
| `pair_run` | 64 | how long a −/+ run is held before split alignment is given up on | aligning is worth a few lines, not a whole hunk |
| `decode_bytes` | 8 MiB | the decode cache: decoded hunk text held between frames | the number that makes "open the same episode again and it is instant" cost a bounded amount instead of everything (`materialize.rs`) |
| `max_in_flight` | 32 | decode jobs queued *plus running* at one moment | the cap behind "rapid scrolling cannot queue unbounded work"; see [materialization](materialization.md) §3 |
| `prefetch_hunks` | 8 | hunks decoded just past the visible window | one `]` should already be there; two pages ahead is guessing |
| `prewarm_hunks` | 24 | hunks decoded for an episode that has not been opened yet | the first frame of the next episode should read nothing |

The last four are `e1.8`'s, and they are the ones that only mean something together: a prefetch with
no cap is how a scroll turns into a disk stall.

The claim this buys, and the one worth reading twice: **the index is O(files + hunks), never
O(lines)**. Per file the index keeps the path, a kind, counts, class, state, evidence spans and the
byte range of each hunk; per hunk a label, those two offsets, the row count and four anchor rows.
Nothing per line. That is what the 50 000-line case actually means: scanning it reads the patch once
in `chunk_bytes` pieces, keeps `files + hunks` of metadata under `index_bytes`, and renders it the
same way as a nine-line diff.

For a window build the working set is smaller still: about `height` source lines plus one
`pair_run`, because the planner reads lazily and stops at the bottom of the window. The limit is
passed in as an argument (`doc::decode`'s `limit`), not left to be remembered: a pane asks for the rows
between the top of its viewport and the end of the hunk, and the decoder stops there.
`a_window_build_reads_only_the_lines_it_draws` in `tests/diff_behaviour.rs` asserts exactly that
against a 10 000-line hunk — if a change ever makes the read grow with the file instead of the
window, that test fails and the guarantee is visibly broken rather than quietly gone.

A worker decoding *for the cache* passes no limit, and that asymmetry is deliberate: a cached hunk has
to serve every window that later intersects it, so it is decoded whole (up to `window_lines`) and off
the UI thread. The inline path — no workers, `--plain`, a test that wants to see the answer in the
same call it made — keeps the window limit, because it is paying for the read in the frame that asked.

When a cap *is* hit the scan stops and says so — `stopped: "the 16 MiB index budget ran out at file
312"` — and every later pane repeats the sentence. A truncated index that looks complete is a lie; a
truncated index that says which 311 files it saw is a partial view you can act on.

## 3 What one pass keeps

`scan.rs` reads the patch once and keeps small things:

* annotations from before the first `diff --git` — `X-Fathom-Class`, `X-Fathom-Evidence`,
  `X-Fathom-Skip`, `X-Fathom-Patch-Skip`. They ride in the patch so the viewer can classify, attribute
  and explain without touching a store. The names are ours, so no real patch can collide with them;
  a real `git diff` without them classifies by path.
* per file: path(s), kind, added/removed counts, byte offsets of each hunk, and why it has no text.
* per hunk: label, byte range, row count, and the row each of the four shapes draws it at.

Everything else is re-read from the source when a window is drawn: `lines.rs` seeks to the hunk's
offset and yields lines until the window is full, then stops. Byte offsets are kept instead of text
precisely so this works when line text is unavailable: the offsets still describe hunks whose bodies
were never decoded.

`plan.rs` is the one planner used twice — once by the scan (to record anchors) and once by the
renderer (to draw). They cannot drift, which is what makes a row number worth calling: `]` jumps to a
row the planner produced in both shapes, and the hunk count in the tree equals the number of `@@`
lines that open (a hunk with no body, as git can emit for a mode-only change, counts as zero).

## 4 The tree

Rows are grouped by taxonomy class first (config, docs, test, source, other, generated, vendored,
noise, unknown — the `e2.1` classes plus `unknown` when the patch carries nothing), then by directory
inside each class, because the question a reviewer asks is "is this review about config or about
source", and only then "which file". Rows:

```
▸6 files · +14 −9
▾ test  +7 −3  (1 f)
  crates/fathom-analyze/src/  +7 −3
  score.rs ~ +7 −3 ✓
▾ unknown  +4 −3  (2 f)
  assets/  +4 −3
  logo.png bin +0 −0 48 120 B
```

* counts are the sum of the subtree, so a collapsed group still tells you where the weight is;
* the marker after the path is the change (`~` modified, `new`, `del`, `ren`, `bin`, `mode …→…`,
  `big`), never colour alone, and the class name is on the group row — a screen-reader transcript
  says `modified`, not "yellow" (ADR-006 §10.2);
* `✓` read, `●` partial (`d`), `*` dirty — `*` wins, because a dirty file's read state describes
  bytes that no longer exist;
* rows with nothing to draw still appear (`logo.png`, `deploy.sh`) — the change is real even when
  there is no text, and a tree that drops it teaches the reviewer to distrust the tree;
* noise files are hidden by default and named by the count row (`noise: 1 hidden by
  `generated+bulk` (b)`) so the suppression is visible and `b` reverses it;
* a collapsed directory shows one file, marked `(+3 more)` — the collapsed subtree must not vanish
  from the count;
* the tree is built whole and unclipped, then laid out with the same middle-truncation as the
  recommendation list, because a path whose end is invisible is not searchable;
* structure and text are separate objects (`tree.rs`): `TreeShape` is the grouping, sorting and
  totals — one O(files) pass, rebuilt only when the index or the noise rule changes — and a `Tree`
  borrows it and draws the rows it is asked for. The pane asks for its 22 rows and gets 22 rows, so a
  5 000-file episode costs the same as a nine-file one ([materialization](materialization.md) §4).

## 5 The hunk pane

Four shapes from one planner: unified as-written (`\`), unified ignoring whitespace (`=`), split
as-written (`/`), split ignoring whitespace (`|`). One row is planned once — a context row spans both
sides of a split, a wrapped row is one row, a folded pair is one row — and each shape projects it.

The gutter is `old new sign text`, both sides printed per row, because when a new-side number is
missing that *is* the fact that the line is gone. Tabs expand to four columns (tab-size is not a
preference here, it is the only way columns line up), and CR is shown as `␍` and never as a blank
column: the whole point of `=` on a CRLF churn is to make the invisible visible, and hiding `\r`
would be hiding the diff.

The pane does not decode what it cannot see, and does not wait for what it can. `doc::build` asks a
`Hunks` for the rows of each hunk that intersects the window: inline that source reads the rows it is
allowed to draw; with workers running it is a bounded cache, and a hunk that has not arrived is one
row that says so —

```
⋯ hunk 11 (25 row(s)) is being decoded — this window is not complete
```

— placed at the row the hunk starts at, so the window does not shrink or jump when the rows land, and
the footer counts what is missing (`2 hunk(s) in this window still decoding`). Nothing is ever drawn
from a hunk outside the requested window, and nothing is drawn from an old shape: the cache key is
(origin, path, shape, hunk), which is what makes a `S`/`=`/`W` toggle a cache miss rather than a
stale frame. [materialization](materialization.md) is the whole story.

Syntax highlighting (`highlight.rs`) is regex-ordered (comment, then string, then number, then keyword,
then type/call) over a language picked from the extension plus two content probes (a `<?php` line wins
a `.js` file; a shebang wins anything). It is deliberately small: the ticket is a *tree plus pane*, and
a full incremental highlighter is a project. Nothing about it carries a fact — colour is decoration,
and `--plain` produces the identical layout with the styles stripped. The header line prints the language
it assumed as a word (`score.rs ~ +7 −3 rust`), because under `--plain`, on a monochrome terminal, or
through a screen reader the colour was the only place that assumption lived, and "the C file was
highlighted as shell" should be readable rather than guessed.

A line longer than `max_line_bytes` is cut and marked `⋯`, with `1 line(s) clipped at the 8 192 B cap`
in the footer. A line whose bytes are not UTF-8 is replaced, marked, and named the same way: a file
that is 90 % text is not rendered as binary.

## 6 Every degenerate case, and what the pane says

This is the part of the ticket that is easy to skip and the part the ticket is actually about.

| Case | What the tree row says | What the hunk pane says |
|---|---|---|
| binary | `logo.png bin +0 −0 48 120 B` | `no line text for this change` / git's `Binary files … differ` line |
| mode-only | `deploy.sh mode 00644→00755 +0 −0` | `mode-only change · 00644 → 00755 · nothing else changed` |
| deleted | `gone.py del +0 −3` | the hunk pane draws it; a deleted file with no body says so |
| renamed with edits | `renamed.py ren +3 −3` | the pane shows the rename (`old → new`, similarity) then the hunks at the new path |
| oversize | `vendor/big.js big 9 123 456 B` | `over the 5 242 880 B materialisation cap` + `this file is 9 123 456 — its text is not decoded` |
| whole patch offline | one row, the reason | `not available offline — episode predates the local packfile` + `run: fathom fetch --episode 0:12` |
| one file offline | the row carries the reason | `not available offline — blob not in the local object store` + `run: fathom fetch --blob a2a2a2a` |
| CRLF churn | counts as written | as-written shows every line changed with `␍`; `=` folds the pair and says `≈ 1 whitespace-only pair (= shows)` |
| whitespace-only change | as above | folded to one row, counted, with `2 pair(s) folded · = shows` in the footer |
| empty patch | `the patch is empty — 0 bytes` | same |
| not a patch | `not a unified diff: no file headers in these N lines` | same |
| source gone | `no patch is loaded` | the same sentence, plus the toast that came back from the read |
| hunk pane too narrow for split | — | stays unified, states it, and names the rule (`DiffView::toggle_layout`, `diff::model::SPLIT_MIN_COLS`) |
| too many files / hunks | `stopped: …` in the index | the sentence is repeated in the pane's notes |

`unavailable.diff`, `file_unavailable.diff`, `oversize.diff`, `binary.diff`, `mode_only.diff`,
`rename_edits.diff`, `deleted.diff`, `crlf.diff`, `whitespace.diff`, `long_line.diff`, `empty.diff`
and `not_a_diff.diff` are fixtures under `crates/fathom-tui/tests/fixtures/`, and
`every_degenerate_shape_says_what_it_is` fails if any of them ever renders as a blank pane.

## 7 Motion, and the three ways out

`j`/`k`/`g`/`G` and PageUp/PageDown scroll the pane whose frame is focused; `]`/`[` walk hunks in
both layouts by asking the planner which row each hunk starts at, and a failed jump says "no hunk
below this row" rather than staying silent. `<Enter>` on a directory row folds or unfolds it. `S`
toggles split (refusing out loud when the pane is under `SPLIT_MIN_COLS`), `W` word-diff, `=`
whitespace, `b` noise.

The three exits are the ones a reviewer's hands reach for, and none of them is a key that
"belongs to another view":

* `y` copies the path under the cursor;
* `Y` copies the raw patch text of the hunk under the top of the window — header line included, so it
  pastes as a patch and not as a suggestion;
* `E` opens the file at the line the pane is showing, in the editor the reviewer chose — `$FATHOM_EDITOR`
  if set, then `$VISUAL` and `$EDITOR` as the shell would read them, then `[ui] editor` in the user
  config, then the platform's default. The same planner answers for `fathom open` on the command line,
  so the key and the verb cannot disagree about which program or which line argument
  ([the exits out of the reader](handoff.md)); which of the four the answer came from is in the toast.

Nothing is run by the view. Running a program or writing an escape sequence needs the screen handed
back, and only the frame loop owns the screen it took, so a keystroke queues a `Handoff` — `Run(command)`
or `Write(sequence)` — names it in the toast, and the loop carries it out through
`FrameSink::run_external` / `FrameSink::write_terminal` before the next frame, the sink's own outcome
replacing that toast. A sink with no terminal to hand back — the headless one, a `--plain` run — refuses
in words, and the refusal is what the reviewer reads. That is also why all three are testable without a
TTY and without three machines: the assertion is on the command that was planned.

Copy resolution: `FATHOM_CLIPBOARD` (any OS, escape hatch first) → macOS `pbcopy` → Linux
`wl-copy`, `xclip -selection clipboard`, `xsel --clipboard --input` in that order → Windows
`clip.exe`, then `powershell Set-Clipboard`. Which of them is *on this machine* is an `Env` answer that
the host supplies; with nothing runnable the plan is OSC 52, the one clipboard that works over `ssh`,
and the toast names `FATHOM_CLIPBOARD` as the thing to set. OSC 52 goes to the terminal and to nothing
else — escape bytes in a pipe would claim to have copied something. Editor invocation is per family
(`vim`/`nvim`/`vi`/`kak`/`nano`/`micro` get `+L:C`, `hx`/`zed`/`subl` get `path:L`, the VS Code family
gets `--goto path:L`, the JetBrains family gets `--line L path`, `emacs` gets `+L:C`, and anything else
gets the path alone and opens at the top — said out loud rather than discovered at the top of a file), and
the variables that name the editor are read by `fathom-cli` for the same reason the `PATH` answer is: this
crate reads neither.

`fathom-diff://path:line` is ADR-006's handoff and nothing else emits it: `E` runs the editor
directly instead of printing a URI nobody asked for.

## 8 How it is tested

* `tests/diff_golden.rs` — golden text for every shape in §6 plus the ordinary review, at a fixed
  110 × 24 frame and fixed 44 / 64-column panes, with the scenario in the file header. Re-bake with
  `FATHOM_GOLDEN=write cargo test -p fathom-tui --test diff_golden`.
* `tests/diff_behaviour.rs` — behaviour: hunk motion landing on the planner's rows, the tree's
  counts and markers, noise suppression, split refusal, budgets stopping and saying so, the copy and
  editor plans per OS, and every degenerate fixture.
* `tests/layout.rs` and `tests/keymap.rs` — still pass against the generated tables; the diff view's
  bindings are the ones in ADR-006 §7 plus `=` for the whitespace toggle, and `tools/tui_tables.py`
  owns all of it.
* `tests/diff_materialize.rs` and `tests/materialize.rs` — `e1.8`: the four performance criteria, and
  the queue and the cache underneath them. [materialization](materialization.md) §6 lists which test
  answers which criterion, with the measured numbers.

Goldens are text, so a rendering change is reviewable in a diff of a `.txt` file; behaviour tests
press keys through the same chord machine the real terminal uses.
