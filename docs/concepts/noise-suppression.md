# Noise suppression: what a reviewer should *not* have to read, folded with the reason printed

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-zqf` (`e2.2`)
- **Implements**: the glossary entry [`Noise / noise suppression`](thesis.md) and the
  thesis's §7 rule "never hide silently"; ADR-002 §9 (team 5 owns
  `fathom-analyze/src/noise/` — this is the second analysis epic 2 builds on, after
  `e0.9`'s path half); ADR-004 §5.1 in spirit (every claim here is derived from a pair of
  blobs and nothing else, so it is reproducible and cheap); and the classifier of
  [change-classification.md](change-classification.md), whose per-path label is one of the
  three inputs to the decision.
- **Executes as**: `src/recommender/noise/` (`norm.py`, `reformat.py`, `generated.py`,
  `lockfile.py`, `policy.py`, `episode.py`, `render.py`, `explain.py`, `corpus.py`,
  `eval.py`, `cli.py`), `tests/test_noise_suppression_contract.py`, and
  `python3 -m recommender.noise …`. The Rust home is `fathom-analyze::noise` (team 5).

---

## 1. What this decides

Three questions per episode, then one:

1. **Reformat.** Is this file pair a re-layout and nothing else — `rustfmt`, `prettier`,
   `gofmt`, `black`, `clang-format` rewrote the whitespace and left the program alone?
2. **Generated churn.** Is this generated file's diff the generator's output moving — a
   version stamp, a re-sorted index, a whole-file regeneration — rather than a person
   editing generated code?
3. **Lockfile-only.** Is this lockfile's diff resolved dependency state that moved by
   itself, with no manifest change to review alongside it?
4. **Collapse.** If *every* file in the episode folded under an enabled rule, the episode is
   one line instead of a file list.

And one rule that outranks all four, from [the thesis](thesis.md) §7:

> **Suppression is a view, never a deletion.** The raw content is always one keypress away,
> and the policy that hid it is printed inline.

This is why the artifact of this ticket is not a filter. A filter answers "what is on
screen"; this answers "what is on screen, **who said so, and which key shows the rest**" —
and the second half is not a footer appended to the first. Every fold is an object that
carries its rule, its reason, its evidence, the layer that enabled it and the key that
undoes it; a fold missing any of those does not render, and does not exist
([§5](#5-the-fold-line-and-the-invariant)).

Two refusals follow from that, and both are load-bearing:

- **Nothing is folded that cannot be described.** An unparseable lockfile, a revision the
  tokeniser refuses, a generated file whose change is too small to call churn: all stay on
  screen, each with a printed reason for staying (`undetermined`, `hand-edit-suspected`,
  "no entry structure this tool can read").
- **No fold is inferred from a path alone where content is knowable.** `e0.9` put
  `reformat` in the noise vocabulary but unreachable by a glob
  ([paths.md](paths.md) §9); the reformat claim here comes from comparing two revisions,
  never from a filename.

---

## 2. The comparison: token-normalised, per language

AC 1 asks for "an AST-normalized or token-normalized comparison (per language) rather than a
whitespace flag". A whitespace flag cannot pass that test, and the reason is worth stating
exactly, because it is the difference between a reformat check that is *usually* right and
one a reviewer can rely on.

Consider a call re-wrapped one argument per line:

```python
def render(title, rows, style, show_totals):        def render(
    ...                                          ):
```

More than whitespace moved — text crossed line boundaries. A line-oriented whitespace
comparison sees changed lines whose stripped forms differ and calls it content. It is
wrong: the program is the same program. Only a comparison that ignores line structure sees
it. So `norm.normalize(path, text)` produces a **token stream** per language and the claim
is `stream(before) == stream(after)`.

The normaliser is deliberately weak in exactly one direction: it may erase only what a
formatter is entitled to write.

| Strategy | Languages | What is normalised away |
| --- | --- | --- |
| `c` (brace scanner) | Rust, C, C++, C#, Java, Kotlin, Swift, Scala, PHP, Go, JS/TS/TSX, CSS | indentation, line breaks, trailing commas before a closing bracket, `use`/`import`/`package` block order, comment placement, line-ending style |
| `py` (`tokenize`) | Python | the above plus logical-vs-physical line structure (INDENT/DEDENT/NEWLINE dropped), so `black`'s wrapping and magic trailing commas vanish |
| `line` | TOML, INI, YAML, Markdown, shell, JSON, snapshots | per-line word tokens (structural punctuation split), so realignment and padding vanish while a value's text stays verbatim |
| `verbatim` | fenced Markdown blocks, multi-line strings, Jest snapshot bodies | nothing: compared as text, because reflowing a command in a code fence *is* changing the command |
| *(none)* | anything else | the strong claim is unavailable; the pair cannot be called a reformat |

Three properties keep this honest:

- **Strength is recorded, not implied.** A fold says `tokens` (streams identical) or
  `whitespace` (no token model available *and* every changed line differs only in
  whitespace). `whitespace` is the weaker claim and the render says which one fired — a
  reviewer who assumed token-level proof is told they did not get it.
- **A refusal is an answer.** An unterminated string literal makes the scanner refuse
  rather than emit a plausible prefix; the claim is `undetermined` and the diff stays
  visible. A silent truncation would have been a fold, and a wrong one.
- **Order is only erased where a formatter owns it.** Import blocks are canonically sorted
  on **both** sides of the pair (that is what `reorder_imports`, `goimports` and `isort`
  do); statement order is never erased. A test asserts both halves of that sentence.

Every claim carries receipts: token and comment counts, a pair digest, the normalisation
kinds that fired ("indentation (3 line(s))", "line wrapping (10 → 13 lines)"), and the
formatter the pattern is consistent with. `fathom noise explain` prints them; so does the
fold line under the collapse.

### What a reformat is *not*

A real change wearing whitespace's clothes — `1 + 2` → `1 + 3`, one character of a
snapshot's expected value, a reflowed command inside a fenced block — is content, and the
corpus contains each of those traps deliberately. The dangerous direction is the false
fold, not the missed one: a missed reformat costs a reviewer a scroll, a false fold costs
them the change they were hired to read.

---

## 3. Generated-file churn

`e0.9` says a path is generated; that is a fact about the path, not about the diff. The
generated rule therefore asks what moved:

| Kind | Shape | Folded |
| --- | --- | --- |
| `stamp` | every changed line is a generator stamp (`@generated`, version lines, `Code generated … DO NOT EDIT`) | yes |
| `resort` | the same set of generated lines, re-ordered (≥ `RESORT_MIN_LINES`) | yes |
| `regen` | bulk regeneration: ≥ `BULK_RATIO` of the file **and** ≥ `BULK_MIN_LINES` changed lines | yes |
| `hand-edit-suspected` | a small change inside a generated file | **no** |

The last row is the one that makes this rule trustworthy. A person who patches a
generated file — the classic `sed` on a checked-in protobuf — is doing something
dangerous and wants it reviewed, or needs it caught. Bulk thresholds exist so the rule fires
on regeneration, not on a hand patch that happened to land in a large file. When the small
case fires, the file stays visible and a note says why: the note is the finding.

The detector also records what it was told (`path_class`, confidence band, the generator
marker it saw, e.g. `protoc-gen-go`) so the fold line can name the generator rather than
asserting authorship by the machine.

---

## 4. Lockfile-only edits

A lockfile is a machine-written statement of resolved versions. Reviewing it line by line is
waste; whether the *dependency decision* happened is not waste, and the difference is
whether the declaring manifest moved.

- **Manifest did not move** → the lockfile folded to one counted line: `Cargo.lock: 2
  dependencies upgraded (no manifest change)`. Counted from parsed entries, not from line
  counts: twelve formats over thirteen filenames — `Cargo.lock`, `package-lock.json` (v1
  `dependencies` and v2/v3 `packages`) and `npm-shrinkwrap.json`, `pnpm-lock.yaml`,
  `yarn.lock`, `poetry.lock`, `Pipfile.lock`, `go.sum`, `Gemfile.lock`, `mix.lock`,
  `composer.lock`, `packages.lock.json` and pinned `requirements.txt`.
- **Manifest moved too** → the rule stands aside entirely. A `Cargo.toml` + `Cargo.lock`
  pair is a decision; the diff is the evidence for it, and hiding the evidence is the
  opposite of this ticket.
- **Unparseable** → not folded. The fold line would have to count entries, and a line that
  says "no entry structure this tool can read" is not a reason to hide anything.

`go.sum` is read as `(module, version)` pairs with the `/go.mod` line treated as its own
entry: a module and its go.mod can move independently, and collapsing them would report one
move where two happened.

---

## 5. The fold line, and the invariant

The whole UI contract fits in six lines, and `render.audit` fails a render that breaks any
of them:

```
ep-1 · 1 file · 0 semantic changes · 1 folded (reformat-only change ×1; formatter: rustfmt ×1) · collapsed
⊘ [press b to show it] · policy: built-in default · episode folded (nothing but noise)
  — 0 semantic changes: 1 file(s) rewritten by rustfmt ×1 with no token-level
  change: the tokens, the comments and every literal are identical before and after
  · 1 file(s) folded: src/thing.rs (reformat)
  · 6 changed line(s) hidden behind one line
  · Rust: 66 token(s), 1 comment(s) identical across the pair (pair 713a4815ec7e)
```

Order inside the fold header is not cosmetic: the marker, the **key** and the **policy**
lead, and the descriptive part trails, because a pane that runs out of width must lose the
description and never the affordance. Fold headers are therefore never wrapped — at 40
columns the reason and the evidence wrap, the header does not, and a test asserts the key
survives at 40, 60 and 200 columns.

The invariant, as four mechanically-checked claims:

1. every fold line names the expand key (`[press b to show it]`);
2. every fold line names the policy and the layer that produced it
   (`policy: repo fathom.toml`, `policy: session keystroke b`);
3. every fold prints a reason line (`— …`) and at least one evidence line (`· …`), in the
   same block, indented under it;
4. expanding an episode loses no file — every path in the change set is on screen after
   the keypress, with the rule that *would* have folded it still printed beside it.

Enforcement is in three places, because one is not enough:

- **At construction.** `Suppression.__post_init__` raises `InvisibleSuppression` if a fold
  has no reason, no evidence or no expand key. A fold that violates the invariant is not
  bad-looking; it does not exist.
- **At render.** `render.audit(lines, view=view)` audits the produced text — the same
  function the CLI runs on its own output — and `audit_view` adds the structural claims
  (including "expanding loses nothing").
- **As a golden.** `tests/test_noise_suppression_contract.py::test_the_rendered_fold_is_the_blessed_text`
  compares both render states, byte for byte, against blessed text, and
  `test_the_auditor_fails_a_render_that_hides` is its negative control: the audit is
  handed a render with the reason line removed, with the key removed, with the policy
  removed, and must complain each time. An auditor that cannot fail is decoration.

---

## 6. The policy: per repository, per user, per session

Eight keys, all declared in `e0.8`'s schema so they layer, validate and explain like every
other key (`fathom config explain --key noise.suppress.lockfile` prints the chain):

| Key | Default | Rule it gates |
| --- | --- | --- |
| `noise.suppress.reformat` | `true` | fold a token-identical pair |
| `noise.suppress.generated_churn` | `true` | fold stamp / re-order / bulk regeneration |
| `noise.suppress.lockfile` | `true` | fold a lockfile without its manifest |
| `noise.suppress.vendored` | `true` | fold vendored code |
| `noise.suppress.user_glob` | `true` | fold a path `noise.globs` claimed |
| `noise.collapse.noise_only` | `true` | collapse an episode whose every file folded |
| `noise.keep` | `[]` | globs suppression may never fold, whatever the rules say |
| `noise.expand` | `b` | the key that shows a folded change again |

Precedence is `e0.8`'s, and every boolean merges `replace` rather than `extend` on purpose:
a merge that could only ever add `true` would make it impossible for a repository to say
*no*. A fold prints which layer won:

```
lockfile   off   noise.suppress.lockfile   [b] — repo layer (fathom.toml:6)
```

**Per session, one keystroke.** `Session` is the session layer, and the keystroke is
`noise.expand`'s value (`b`, the key the TUI already binds as `diff.noise_toggle` in
`e1.7`). Pressing it turns folding off for this process; pressing again restores the
repository's policy. It writes nothing — a test snapshots the tree's bytes around the
override and diffs them — and it is asymmetric in the only safe direction: pressing the key
cannot *unblock* a rule a repository deliberately turned off. The rendered header says
`session override active (b pressed: nothing is folded)` so an override is never mistaken
for a bug.

`noise.keep` is the reviewer's "always show me this file" list and beats every rule,
including a repository's own assertion: the only thing that outranks "hide it" is "show it".
`noise.expand` is configurable because the fold lines are *rendered from* it — a fold still
saying `[press b]` after a rebind would be a lie.

---

## 7. Where a reviewer reaches it

| Surface | What it shows |
| --- | --- |
| `fathom noise explain <path> --repo A --old-repo B` | the claim, its strength, the language model, what was normalised, what was refused, the counts and the pair digest |
| `fathom noise episode --manifest m.json [--expanded] [--no-suppress]` | the fold lines, or the expanded state with the policy still printed |
| `fathom noise policy [--repo …] [--no-suppress]` | every rule, its config key, on/off, the winning layer, the expand key |
| `fathom noise languages` | the token model: languages, tokeniser, formatter, what each normalises |
| `fathom noise score` / `bench` | the accuracy/precision/recall table and the cost budget |
| `⊘` lines in the diff pane (`e1.7`) | the fold itself, with `b` to expand — the same `Suppression.render()` string, not a re-translation |
| the `noise folded` figure on an episode's delta document (`e2.11`) | how much of the episode the totals exclude, per class — the same fold the `⊘` lines show, on the document every surface reads |

---

## 8. The labelled corpus, and the gates

`corpus.py` is 51 file pairs, 5 generated cases, 14 lockfiles and 10 episodes, each labelled
by hand against the built-in tables. It contains every trap listed above (a real change
inside whitespace, a reflowed shell command in a fence, a one-character snapshot change, an
unterminated literal, an unparseable lockfile, a hand patch inside a protobuf) and the
formatter shapes it must not trip on (quote-character changes, trailing commas, import
re-order, `black`'s magic trailing comma, JSX re-wrap, TOML realignment).

`fathom noise score` prints four numbers and gates four:

| Gate | Bar | Why that bar |
| --- | --- | --- |
| pair accuracy | ≥ 0.95 | same convention as `e2.1` |
| fold **precision** | **= 1.0** | a false fold hides a real change; this is the one forbidden failure, so the bar is absolute and each false fold is also named individually |
| fold recall | ≥ 0.90 | a missed reformat costs a scroll, not the review |
| episode collapse | = 1.0 | "a reformat-only commit is one line" is the ticket's headline |

Cost, because this runs per file per repaint: ~0.14 ms per pair, ~1.5 ms per episode,
~0.02 ms per lockfile in the Python prototype (`fathom noise bench`), with a test bounding
a single pair well under a frame.

---

## 9. Ownership, and what this document refuses

This package owns: the token model, the three detectors, the fold object and its renderer,
the policy resolution and the verb surface. It does not own: the path taxonomy (`e0.9`), the
change classifier (`e2.1`), the config layering (`e0.8`), or where the fold lines are drawn
(`e1.7`'s diff pane, which calls `Suppression.render()` rather than inventing its own
wording).

It refuses three things: folding anything it cannot describe; folding on a path rule where
content evidence is available; and letting a session override undo a repository's deliberate
`false`. Each refusal has a test with the same sentence as its name.

## 10. The Rust seam

`fathom-analyze::noise` (team 5, ADR-002 §9) is the Rust home; the normaliser's per-language
table and the policy keys are the two things both implementations must agree on byte for
byte, since a fold line printed by one and expanded by the other would be the same
contradiction in a different language. The golden text in
`tests/test_noise_suppression_contract.py` is the first artifact the Rust port must reproduce.
