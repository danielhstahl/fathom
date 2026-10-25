# The timeline

Status: built by `reco-bjo` (stage `e1.6`). Code: `crates/fathom-tui/src/timeline/`.
Surface: view `Timeline` (`t`) in `crates/fathom-tui/src/views.rs`, panes `rail` → `card`; the
state lives in `crates/fathom-tui/src/timeline/mod.rs` (`Timeline`) and is held by `App` next to the
diff viewer's own state. Bindings: `src/recommender/tui/keymap.py` (the source of truth, generated
into `crates/fathom-tui/src/keymap.rs` and `docs/adr/0006-tui.md` §7).

The timeline is the screen a reviewer arrives at first and returns to whenever they lose the thread:
*what happened, in what order, and which of it have I not read?* It is a list of episodes — one row per
episode — with grouping, composable filters, quick jumps, and a details pane about the row under the
cursor. Everything below follows from the same rule the diff viewer plays by: **the rail never lies and
never goes quiet.** A row with no author prints `unknown`; a risk nothing could rank prints `?`, and a
risk the spine did rank prints its number with `w` beside it
([risk-ranking.md](risk-ranking.md)); a group that is collapsed says how many rows are behind it; a
rail with nothing loaded names the command that fills it.

## 1 What it reads, and what it refuses to read

`fathom-tui` is IO-free (ADR-002 §3, lint `W1`), so the timeline opens no store, no repository, no file,
and no clock. Rows arrive as a **batch**, from whoever has them:

```rust
pub struct Batch {
    pub origin: String,          // "index epoch 41", "fixture index" — the strip prints it verbatim
    pub episodes: Vec<Episode>,  // newest first, exactly as the index returned them
    pub total: Option<u64>,      // the index's own count, when a page was handed over
    pub why_empty: Option<String>,
    pub index: IndexState,
}
```

`total` is the difference between an honest page and a misleading one. A rail that was given 500 of
3 412 episodes and prints "500 episodes" tells the reviewer the repository was busy; the strip prints
`500 of 3 412 · index holds 3 412 (index epoch 41)` instead (ADR-001 §7.2). `Timeline::load` is the one
way rows enter, and it is also the way a **re-index** enters — same method, same relink arithmetic (§6).

The batch reaches the shell through `frame::Options::episodes`, which `fathom-cli` fills from the spine
(`crates/fathom-cli/src/episodes.rs`, the read `reco-glv`'s first P1 asked for): the live episodes,
newest first, one page of them, with the review markers, the paths each one touched, and the per-file
rollup the ranking reads. That read needs no delta model — identity, authorship, scale, session/turn
and review state are rows `e1.3` already writes, and the per-file sums are `git_commit_file` rows —
which is why the rail has rows in an Epic 1 build while `insight::episodes` is still `e2.11`'s to
answer. What the store cannot say stays `None` and prints `?`, so a filled rail is not a claim that the
delta model landed: the risk column carries the spine-only ranking's number and its reasons
([risk-ranking.md](risk-ranking.md)) while the card's `delta` row is still the stated absence (§8). The
toast line at open
says `500 episodes from index epoch 1 (ready) · index holds 4 000`, because a rail that is merely full
does not otherwise tell the reviewer how much of the index it was given.

That read has a headless form, and deliberately so (`reco-11b`). `fathom episodes` asks this binary's one
episode query from a shell that takes no terminal — the same filters, the same order, the same review
states — because ADR-006 §6.7 refuses a fact that only a screen can show, and the acceptance run had to
*look at a monitor* to say "the rail shows 213 episodes". Both readers live in
`crates/fathom-cli/src/episodes.rs`, which is why the strip's count and the payload's are one number with
two printers rather than two numbers that happen to agree today
([the headless verbs](cli.md#the-headless-verbs)).

The shell is *also* told what the index holds — `Options::index`, an `fathom_tui::index::Facts` the
composition root read from the store, because the rail may not (the empty-state defect the MVP
walkthrough filed). It is where the batch's `origin` and the strip's epoch come from, so the rows and
their provenance cannot disagree, and it is what lets an empty rail say
*index epoch 1 ready · 500 commits ·
412 eps* rather than sending the reviewer to an index that already finished (§9). The frame's instant
is passed in the same way (`Options::now`, ADR-002 §4.4): the rail ages every
row against a `Timestamp` it was handed, never against `now`, which is what makes every golden in
`tests/golden/timeline/` stable enough to read.

An `Episode` carries exactly the fields the rail can name: identity and digests, members, created/last,
author kind/name/confidence, summary, paths, file/add/del/binary counts, review state, session and turn,
boundary rule and confidence, an optional risk, and whether the episode is still live. There is no
`structure` field, no `findings`, no `risk_score` — the card cites the fields `e2.11` will publish and
says they are not in this build (§8).

## 2 The row, and the six columns it is made of

```
▸ e8f1a2 0:12 agent  +412 −90 ●●●●●● ? ⚑
```

| Column | Source | What it cannot say |
|---|---|---|
| `e8f1a2` | `Episode::short_id` | — (never dropped) |
| `0:12` | `days::relative(now, created)` | a date at narrow widths — the same fact, coarser |
| `agent` | `AuthorKind` badge: `agent`, `human`, `?` | the `?` **is** the answer: attribution failed, and blank would read as "no author" |
| `+412 −90` | `n_add`/`n_del`, compacted | a rename or a mode change — those are the diff pane's |
| `●●●●●●` | `Scale::spark`, a log scale over the loaded corpus | a size comparison across epochs; the scale is what *this* rail holds |
| `?` / `82` | `Episode::risk`, `Some` from the spine-only ranking (`surfaces`, `size`, `deletions`, `breadth`, `author`) and `None` when the read never reached that episode | a number the reviewer can interrogate with `w`, or `?` — never a `0` standing in for "nobody ranked this" |
| `⚑` | `ReviewState::glyph`: `⚑ ⚐ ✓ ✔ × z ?`, or `gone` | colour: under `NO_COLOR` the glyph is the whole state, which is why it is the last column to go |

The sparkline is a **log** scale against the largest loaded episode, in six cells, clamped to at least
one filled cell: a linear scale puts every ordinary 40-line commit at zero cells beside an agent's
4 000-line turn, and a column of empty cells reads as "nothing happened". A change that is entirely
binary prints `bin` rather than a chart of nothing.

## 3 The column ladder

`Columns::for_width` (ADR-006 §6.3) drops columns in the order the ADR fixes — sparkline, then
relative time to a date, then author, then the risk *number*, then the risk glyph last of all:

| Pane columns | Rung | Row |
|---:|---|---|
| ≥ 40 | all six columns | `▸ e8f1a2 0:12 agent +412 −90 ●●●●●● 82 ⚑` |
| ≥ 34 | no sparkline | `▸ e8f1a2 0:12 agent +412 −90 82 ⚑` |
| ≥ 32 | date instead of age | `▸ e8f1a2 14 Oct agent +412 −90 82 ⚑` |
| ≥ 27 | no author | `▸ e8f1a2 14 Oct +412 −90 82 ⚑` |
| ≥ 22 | no risk number | `▸ e8f1a2 14 Oct +412 −90 ⚑` |
| below | id and state only | `▸ e8f1a2 14 Oct ⚑` |

Then a **fit rule**, because the ladder's thresholds cannot promise arithmetic for every terminal: if a
built row is still wider than its pane, the risk cell goes, then the change size — never the identity
and never the state glyph (`render::episode_line`). The ADR's promise is that the *glyph* is the last
cell standing; a row that kept a risk number and clipped the glyph would be keeping the more decorative
of the two. `tests/golden/timeline/ladder.txt` prints every rung over one corpus, widths 42 down to 18,
so a rung that comes back into existence at a narrower width is visible in a diff.

## 4 Grouping, and what a collapsed group must say

`GroupBy` is `day` (default), `session` or `none`, and it is a view option rather than a filter —
`filter clear` clears the rows it was hiding and does not rearrange the rail the reviewer chose.
Headers count their rows and carry the shape the ADR's mock draws:

```
Tue 14 Oct ── 3 episodes
Thu 18 Sep · session 2 (pi) ── 1 episode
session 4 (pi) ── 6 episodes · 13 Oct
··· 17 more — <Space> opens
```

Days are **UTC days** (`days::day_start`), which is also what `goto.today` jumps to; the card states the
timezone under the state glyph so "the 13th" cannot be argued about. A day whose episodes were all one
session names that session and its author; a group of one prints "1 episode", because a header that
prints "1 episodes" is a UI that has stopped being read.

`<Space>` (`list.expand`) collapses the group under the cursor. Collapsing never removes the *fact* of
the rows: the group leaves behind a counted `··· N more` row that `<Space>` opens again, and on an
episode row the key says what it does instead of doing nothing. This is the one place in the view where
content is deliberately off screen, which is exactly why it is annotated.

## 5 Filters

Seven dimensions, and the rule is the store's rule: **OR inside a dimension, AND across them**, so the
same filter means the same rows in the rail and in an `EpisodeQuery` (`filter::Filter::matches` is
written against the same semantics, and `timeline_behaviour.rs` cross-checks the composition against
the model rather than against the rail's own arithmetic).

| Dimension | Values | Notes |
|---|---|---|
| `author` | names, with counts | multi-valued; `unknown` is a value |
| `kind` | `agent`, `human`, `unknown` | multi-valued |
| `state` | `unread`, `read`, `accepted`, `refused`, `dismissed`, `flagged`, `snoozed` | `read` means *opened* — read or any verdict — which is the store's rule, not a shortcut |
| `area` | a path fragment, facet-suggested | matched case-insensitively against every path, so `tests` finds `tests/test_epoch.rs` |
| `since`, `until` | `2026-10-14`, `today`, `yesterday`, `week`, `7d`, `2w`, `month`, `year`, `any` | typed spans resolve to a **bound**, not a suggestion: `7d` becomes the start of the day seven days back and does not move as the frame redraws |
| `group` | `day`, `session`, `none` | the view option, editable here because the surface that edits filters is where a reviewer expects it |

Facets (`Facets::of`) are computed from the loaded rows in one pass: authors, kinds, path areas up to
three segments, and the states present, each with a count, each capped at `FACET_CAP` values so a
repository with 900 authors does not produce a chooser nobody can scroll. A capped list says it is
capped; typing a value that is not on the list is always allowed.

The `F` surface (`timeline::Editor`) is a **prompt**, not a new mode: `F` opens it and then the prompt's
existing keys do the rest — typing narrows the list the way the palette's does, `<C-n>`/`<C-p>` move,
`<Enter>` chooses, `<Esc>` closes. It costs no new chord, so it adds nothing to the reserved set
(ADR-006 §7 K3/K4) and nothing for `keys.overrides` to get wrong. What is on screen is the filter:
`state · unread ✓` cannot be forgotten about the way a filter tucked into a status line can, and the
panel prints `now: author=pi state=unread` above the choices when one is already set. It is sized to its
rows and drawn **above** the chrome, because the status line is where the index state lives and a filter
chooser that painted over it would hide the facts a reviewer needs most while deciding what to filter.

### Persistence

The filter is a value with a key, so `fathom-cli` can put it in `session_state` and hand it back next
run — which is what `e1.11` (**reco-6hp**) now does; [session state](session-state.md) is that whole
contract: the keys, the damage rules, two TUIs in one repository, `--fresh`. Three keys, namespaced for
`ui_state_all("timeline.")`:

```
timeline.filter     = author=pi&author=stahl;kind=agent;state=unread;area=src/store;since=…;group=session
timeline.selection  = e8f1a2…            (the id, first tier of §6)
timeline.members    = e8f1a2:0,e8f1a2:1  (up to SESSION_MAX, the tier-3 ride)
```

`Filter::decode` ignores keys it does not know and defaults what is missing, so a filter written by a
newer fathom degrades to the parts this one understands instead of refusing to start. Restoring applies
the filter first (it decides which rows exist), then the cursor, then reports what actually came back —
"session restored — filter state=unread, cursor on `40c9b3`", or "`e8f1a2` is not in this index — it
moved or left, and the cursor is at the top instead". A resume that guessed silently is worse than one
that admits it found nothing.

## 6 Selection follows the episode

The rail's cursor is a slot index, but the thing the reviewer is *holding* is an identity. `Anchor`
(`timeline::Anchor`) carries everything ADR-003 §6.2 says to carry, and `load` re-attaches it through
the store's own tiers — same tiers, same order, never revised by a weaker one after it matched:

| Tier | Key | Real case it catches | What the toast says |
|---:|---|---|---|
| 0 | `id` | a re-index: rows re-read, reordered, new ones added | nothing (the cursor did not move) |
| 1 | `fingerprint` — author + subject | member SHAs all changed by a rebase of the same content | "re-attached, the same episode by author and subject" |
| 2 | `shape` — author + paths | a **reword**: the id and subject moved, the paths did not | "re-attached, the same episode by author and paths" |
| 3 | any surviving member SHA | a squash or fixup that kept a member commit | "re-attached on a member commit" — a *different* episode, and the toast says so |
| — | nothing reached across | the episode left the index (I2) | "`77aa0c` left the index…" and the cursor goes to the top |

Two corollaries are worth stating because they are the ones a refactor breaks. A **filter** that hides
the selected episode moves the cursor to the nearest visible row but keeps the anchor, so taking the
filter off puts the cursor back; and a **rebase** that changes nothing about identity must not move the
cursor at all, which is what `a_plain_rebase_moves_every_member_sha_and_the_cursor_does_not_move` is for.

An episode whose members are gone is `live = false` and its marker becomes `gone`: a sign-off on history
that is no longer in the repository is the least silence-worthy fact on the screen (ADR-003 §6.3).

## 7 Motion

All of it is keys, and all of it goes through the registry (`tests/timeline_behaviour.rs` drives real
chords through `App::feed_key`, not the methods):

| Key | Action | Key | Action |
|---|---|---|---|
| `j`/`k`, `↑`/`↓` | one row | `gg`/`G` | first, last |
| `H`/`M`/`L` | top, middle, bottom of the window | `<C-d>`/`<C-u>`, `<C-f>`/`<C-b>`, `<PageDown>`/`<PageUp>` | half and full page |
| `<Enter>` | push the card (drill), then the diff from the card | `<Tab>`/`<S-Tab>` | walk the focus ring `rail` → `card` |
| `<Space>` | collapse/expand the group under the cursor | `F` | `filter.edit` |
| `,` `filter clear` | drop every row filter | `,` `goto first unread` | the first unread row **in the filtered order** |
| `,` `goto today` | today's first row, or the nearest before it | — | — |

The four `e1.6` actions are the ones the registry already published (`filter.edit` bound to `F`;
`filter.clear`, `goto.first_unread`, `goto.today` palette-listed), so the view implements the keymap
rather than extending it, and `ADR-006` §7 K1 — bound or palette-listed, never neither — holds by test.
A jump that cannot jump explains itself and names the key that can: `3 unread in what is loaded, none of
them in this filter — F edits it, filter clear is in the palette`. There is no mouse support to
contradict any of this, and no key in the reserved set is touched.

## 8 The card, and the fields it will not invent

The card cites the delta fields the registry names (`delta.structure.*`, `delta.dependencies.*`,
`findings[].{detector_id,severity}`, `risk.*`, `blast_radius.*`) and prints, in place of each one it
cannot fill:

```
delta    no document was handed to this card — `fathom analyze <episode>` is the one source
risk     spine-1 82 = surfaces 50 + author 8 + size 24 · rank 1 of 21
```

That is the whole design constraint in two lines: a pane whose data has not been handed to it says which
verb produces it, rather than rendering a confident `0` or an empty box. The `risk` row
is the one that changed: it is a spine ranking rather than `e4.4`'s findings, so it prints its model
name, its vector and its own ceiling, and `w` opens all of it
([risk-ranking.md](risk-ranking.md)). What it *can*
say it says precisely: commit count, author kind with its confidence, age, review state, the timezone
the day bounds use, where the change landed (top path prefixes with file counts), the boundary rule and
its confidence labelled "a rule, not a fact" (ADR-003 §4.7), the session and turn, and the member SHAs
the review is keyed to — truncated with `…` and `+N more`, never mid-token.

## 9 Empty, filtered and partial states

Every one of these is a sentence in the rail body, in the strip, or both, because "…the pane is empty"
is not an answer (ADR-001 §7.2, ADR-006 §6):

| Situation | What the rail prints |
|---|---|
| there is no index | `no index here yet — run \`fathom index\` here, then reopen fathom` |
| the index is finished and this pane is the queue | `index epoch 1 ready · 500 commits · 412 eps — \`t\` opens the timeline, which lists them as the index knows them; \`fathom show <episode>\` prints one in full` |
| the index is finished and holds no episodes | `index epoch 1 ready · 0 commits · 0 eps — the index has nothing to list; \`fathom doctor\` says whether that is the repository or the run` |
| the epoch is still `building` | `index epoch 2 building · 500 commits · 0 eps — the epoch is still being written — wait for it; if a rebuild died halfway, \`fathom index --rebuild\` starts a clean one` |
| the store would not open | `the index would not open — \`fathom doctor\` says what is wrong` |
| the caller said nothing (a test frame, an embedding shell) | `nothing loaded yet — the shell was handed no index state` |
| the index answered nothing | whatever `Batch::why_empty` said, verbatim |
| the filter selects nothing | `the filter selects nothing — \`F\` edits it, \`filter clear\` is in the palette` |
| a page of a bigger index | strip: `500 of 3 412 · index holds 3 412 (index epoch 41)` |
| the selection left the index | `…left the index`, cursor at the top |
| the card has nothing selected | `no episode selected`, then the two keys that would |

The first six rows are one decision, not six: they are the `Read` variants of
`fathom_tui::index::Facts`, and each row's sentence is the join of its `summary()` (what the index
holds) and its `remedy()` (what to type, or who owns what is missing). A narrow pane prints the two
halves as two rows; the rail body and the strip print the whole join, so the pane and the strip cannot
disagree about a fact the caller already answered. The MVP walkthrough's empty-state defect is the
reason the split exists: with one
sentence for both states, a reviewer with 500 commits indexed was told to run `fathom index`, did, and
was told it again. The walkthrough's first P1 rewrote the second row for the same reason it was filed:
a remedy that is not true is worse than none, and *nothing in this build can list them* stopped being
true the day the composition root started listing them.

## 10 The performance shape

The acceptance criterion is a 10 000-episode rail scrolling at p99 under 16 ms with constant memory,
and the shape that buys it is small:

* the rail holds `Vec<Episode>` (linear in the input, `stats().retained_bytes` measures it) and a slot
  vector of episodes plus headers;
* a **draw builds only the window**: `Rail::lines(width, visible)` produces exactly `visible` lines from
  the computed offset, and `RailStats::rows_built` is the pane's height, never the index's length;
* a **scroll rebuilds nothing**: scrolling moves an offset. `stats().rebuilds` rises only when the filter
  changes or rows are loaded — which is the assertion, not a comment about it;
* the window's start is *computed* from the cursor and the stored offset rather than stored, because the
  pane may have been resized by the layout ladder since anything mutable last ran.

Reproduce the numbers with `cargo test -p fathom-tui --test timeline_bench`: 240 scroll frames drawn
individually at 10 000 episodes (p99 and worst), plus a counting global allocator that measures bytes
allocated per frame and compares 10 000 episodes against 500 — so a rail that quietly started building
10 000 row strings per frame fails by megabytes rather than by a laptop getting warm. The same frame the
terminal gets, at any size, is `cargo run -p fathom-tui --example preview -- timeline 80 24`.

## 11 Tests, and which criterion each one pays for

`crates/fathom-tui/tests/timeline_corpus/mod.rs` is the one fixture source: ADR-006 §5.2's own rows
transcribed, a deterministic 10 000-episode corpus, and the four rewrites (rebase, reword, squash,
deletion). It is shared so a fixture cannot be tuned to make one file pass.

* `timeline_behaviour.rs` — 37 tests through `App::feed_key` and the real renderer: composition of
  author AND area AND unread, the store's OR-inside/AND-across rule, date bounds, session round-trip,
  every relink tier, the column ladder, grouping and collapse, every motion key, the `F` surface, and the
  four registry entries this view owns.
* `timeline_golden.rs` — pinned frames and panes: the ADR's own 80×24 picture, the view's documented
  minimum 61×16, each degenerate state, and `ladder.txt`. Re-bake with
  `FATHOM_GOLDEN=write cargo test -p fathom-tui --test timeline_golden`, then read the diff.
* `timeline_bench.rs` — §10's two measurements, serialised so the allocator's counters mean something.
* `crates/fathom-cli/tests/tui_data.rs` — the read, from outside the process: it builds a 500-commit
  repository with `git fast-import`, indexes it, and asks the shipped binary what its rail draws, so
  that "a fixture handed to the view" is never mistaken for "the binary can fill a rail" — the check
  the walkthrough's first P1 asked for.

## 12 Deliberately not here yet

`delta.*` and `findings[]` rows in the card (`e2.11`, `e4.3`); the *finding-shaped* risk number the
queue now orders by — what it orders by is the spine-only ranking and its stated ceiling
([risk-ranking.md](risk-ranking.md)), which `e4.4` replaces the inputs of, while `e4.5` replaces the
ordering function itself with risk × staleness × blast radius (`e1.8`'s worklist); the queue's own
selection history and per-view scroll (one cursor and one filter are what the session carries,
which is the honest amount); the delta model's own read —
`insight::episodes` is still `e2.11`'s, and the rail's rows come from the store columns Epic 1 owns
instead (§1), one page of them, which is why the strip says `index holds N` on a repository
bigger than a page; saved, named filters and
filter history (`e1.11`'s session state stores one filter, which is the honest amount); `gone` derived
from the store's dead-episode markers rather than handed in per row (`e1.3`'s ledger edge); colour-blind
verification of the glyph set (`e5.6`).
