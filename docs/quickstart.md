# Five minutes

- **Status**: the tour a stranger can finish alone (`e7.5`, **reco-60m**), built on the miniature that
  ships in the binary (`e5.9`, **reco-2uo**).
- **You need**: a terminal, and either the alpha binary or a Rust toolchain. No repository of yours is
  read, written or required for the first four minutes.
- **The promise**: at the end you will have reviewed an episode, signed it off, exported the review, and
  be able to say in one sentence what an **episode** is. That last one is the acceptance criterion, and it
  is the one worth reading for.

Everything below is copied from runs, not from intentions. Where a line is trimmed it says so.

## 0 · Get the binary

Either install the alpha (Linux, macOS):

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/danielhstahl/fathom/releases/download/v0.1.0-alpha.1/fathom-cli-installer.sh | sh
```

…or build it, which works from a clean clone with nothing installed but a toolchain:

```bash
git clone <this repo> && cd fathom && cargo build -p fathom-cli && ./target/debug/fathom --version
```

The build is the honest path today: the installer URL is built from a repository address that is still a
placeholder, so until the public remote exists there is nothing behind it
([release status](concepts/release.md)). `fathom --version` prints the version *and* the compiled-in
feature set, which is the line to paste into a bug report ([features](reference/features.md)).

## 1 · Run the tour

```bash
fathom --demo
```

It unpacks a 21-commit repository into your cache directory, indexes it with the ordinary
`fathom index`, and opens the shell on it. Nothing in your current directory is read or written, no
configuration is read, and deleting the cache directory is a complete undo. Real output, trimmed:

```text
fathom demo · a miniature repository of 21 commits, unpacked to
  /tmp/fth-demo/fathom/13088a3d85a5/signal (pack @ 13088a3d85a5)
  two people and one agent wrote it, so the queue ranks agent work against human work; its history has
  a rename, a dependency bump in two ecosystems, a deleted test, a CI job, a cycle and a merge
  indexing it now, with `fathom index` exactly as you would run it on your own history
  …/.fathom/index.sqlite3: 21 commit(s) walked, 21 new, 56 analyse unit(s) computed in 66 ms
  intent: 0 recorded (0 from the harness), 0 digest-only, 21 none recorded · harness: no stream at .fathom/events.jsonl
  `?` lists the keys · `q` queue · `t` timeline · `d` diff · `s` structure · `g` graph · `p` dependencies · `o` export
  nothing here is yours: it is a copy under your cache directory, and deleting that directory is a complete undo
```

Sixty-six milliseconds for the index. On a repository with real history the same verb prints a row that
overwrites itself — `indexing 62 % · ~14 s left · 4 102 / 6 552 commits` — because a silent wait is what
makes people close a terminal ([first run](concepts/onboarding.md)).

With no terminal to take (CI, a pipe, a screen reader), add `--plain`, and `--lines` for reading order
instead of a grid. Every fact on the screen has a text form; ADR-006 §6.7 makes that a rule rather than a
hope.

## 2 · Look at the queue

Press `q` for the queue — the ranked worklist, which is the product's home screen — then `j`/`k` to move,
`?` for the keymap (rendered from the shipped bindings, not from this page), and `,` for the palette, which
lists every command with its current binding.

The full keyboard reference is [the cheatsheet](reference/cheatsheet.md), and it is generated from the same
table the overlay uses, so it cannot lie about a key that moved.

## 3 · Learn the noun

Every episode in Fathom is **the change produced by one agent turn** — the ordered set of commits that turn
produced, and optionally its uncommitted working-tree state. Not a commit (an episode can have several), not
a branch, not a pull request, not a session. Everything reviewable is keyed to it, including the sign-off
you are about to write. The [concept page](concepts/episode.md) is the short version of why it is defined
that way; [ADR-003](adr/0003-episodes.md) is the long version.

To see one, in another shell against the same miniature:

```bash
fathom --demo --plain show HEAD~1
```

```text
episode d367b98 — HEAD~1
  message   add toml to the dependency surface
  author    Dana Stahl <dana@example.com>  2025-04-30T18:00:00Z
  scale     1 file(s), +1 -0, 0 binary
  grouping  ep-06cf9f46eaf35e6b071c · 1 commit(s) · boundary time-gap 0.60 · author human 0.50
  intent    no recorded intent · no harness events for this repository, and no `Intent:` or `Task:` trailer …
  attribution  human · — · 0.50 via identity
  files     1
    modify      +1    -0  unknown      Cargo.toml
```

That `grouping` line is the episode: this commit is one member of it, and `ep-06cf…` is the address that
survives a rebase. The `boundary time-gap 0.60` beside it names the rung of the cascade that drew the line
([episode](concepts/episode.md)), and the `0.50` is how sure the tool is about who wrote it — printed as a
number because "we think a person did" is a claim, not a fact.

## 4 · Read one, and ask why it ranked

```bash
fathom --demo --plain narrative ep-06cf9f46eaf35e6b071c   # the prose, with [cN] citations
fathom --demo --plain why ep-06cf9f46eaf35e6b071c         # the risk score as an arithmetic
```

The narrative opens by putting the ranking where the reader will see it:

```text
ep-06cf — add toml to the dependency surface

Read this first:
  The ranking puts it at `spine-1 11 = surfaces 10 + size 1`[c1].
```

`narrative` composes its paragraphs out of the [delta document](concepts/delta.md) and refuses to state
anything it cannot cite; the watermark line at the bottom (`fathom-template/1 verified: 29 claims, 31
citations, 6 checks`) is the self-audit's verdict, and no model was in the path
([episode narrative](concepts/episode-narrative.md)). `why` prints the score as a sum — feature, weight,
contribution, and the axes it could not measure, each with the ticket that owns it
([risk score](concepts/risk-score.md)). An absent axis leaves the denominator instead of pretending to be a
`0`, which is the single most important rule in the product.

## 5 · Sign it off

```bash
fathom --demo --plain review sign ep-06cf9f46eaf35e6b071c --note "read it; the Cargo.toml row is right"
```

```text
signed    ep-06cf9f46eaf35e6b071c  accepted · looprs <looprs@example.com> · 1 commit(s), 1 file(s)
written   .fathom/reviews/ledger.md
  · identity from git config
still open  20 unsigned · 0 stale · 0 orphan · 0 tampered
```

The identity came from `git config`, and the verb says so rather than letting it look like a given: a
sign-off with no identity in it is not a sign-off.

Then ask whether the review is complete:

```bash
fathom --demo --plain review status
```

```text
ledger    1 file(s) · 1 entries · 1 fresh · 0 stale · 0 orphan · 0 tampered
episodes  21 live · 1 with a fresh sign-off · 20 without
verdict   NOT ALL REVIEWED
  blocks: 20 of 21 live episodes have no fresh sign-off
  blocks: `.fathom/reviews/ledger.md` is ignored by git: a sign-off git will not add is one clean from
          never having happened — un-ignore the directory (`!.fathom/reviews/`, the line ADR-004 §12's
          stanza requires)
exit code 7
```

`7` is the "not yet" a pipeline can branch on, and the message names every blocking reason instead of just
refusing ([the ledger](concepts/review-ledger.md)). The second objection is the miniature teaching you
something: its `.gitignore` ignores all of `.fathom/`, so the sign-off you just wrote is not committable —
which is exactly the failure the ledger exists to prevent. In your own repository the `.gitignore` stanza
ADR-004 §12 requires un-ignores `.fathom/reviews/`, and `fathom review status` will keep saying so until it
does. The sign-off itself is markdown in `.fathom/reviews/` — committed, reviewable in a PR, and re-attached
to your work after a rebase by a ladder of digests rather than by luck.

If you sign something that has no diff in the index (a merge commit, for instance) the verb refuses and tells
you why: *a sign-off against it could never be checked for staleness*. It writes nothing.

## 6 · Say it to someone else

```bash
fathom --demo --plain export --format md --stdout | head -20
```

One record, three renderings — markdown for the pull request, HTML for an audit, JSON for a program — from
the same bytes, so they cannot disagree, and byte-identical when re-exported
([the review artefact](concepts/export-artefact.md)). `--strict` exits `7` while an operator-action item is
open, which is the half a merge gate can use.

## 7 · Now your own repository

```bash
cd <your repository>
fathom index            # prints a progress row on a terminal, nothing on a pipe
fathom                  # the UI; `fathom --plain` for the text frame
```

Expect the first index to take a moment and every later one to be fast — that is the cache doing its job, and
`fathom stats` prints the hit rate if you want to see it. If the queue is empty, the pane says why and what
to type; if it is wrong, `fathom doctor` is the verb with a remedy per finding
([troubleshooting](troubleshooting.md)).

Optional, and the point of the whole tool: open the second terminal, run `fathom watch` plus `fathom`, press
`a`, and get on with something else ([ambient mode](concepts/ambient-mode.md)).

## What you should be able to say

Three sentences, and they are the acceptance criterion for this page:

1. **An episode is the change one agent turn produced** — a set of commits, addressable by a content digest
   that survives a rebase, and the thing every finding, score and sign-off is keyed to.
2. **A delta is a claim with citations**, and a section that could not be measured says which ticket owns the
   answer instead of printing zero.
3. **A sign-off is a committed fact**, not a store column, and the tool tells you when the thing you signed
   has since changed.

## Where to go next

| I want | Go |
|---|---|
| the keys | [cheatsheet](reference/cheatsheet.md) |
| the verbs and flags | [CLI reference](reference/cli.md) |
| what the findings are | [detector catalogue](detectors/index.md) |
| a task, step by step | [recipes](recipes/) |
| the six nouns | [episode](concepts/episode.md) · [delta](concepts/delta.md) · [risk score](concepts/risk-score.md) · [review ledger](concepts/review-ledger.md) · [ratchet](concepts/ratchet.md) · [ambient mode](concepts/ambient-mode.md) |
| why it is built this way | [the thesis](concepts/thesis.md) and [the ADRs](adr/) |
| it went wrong | [troubleshooting](troubleshooting.md) |
