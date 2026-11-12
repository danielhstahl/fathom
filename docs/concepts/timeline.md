# The timeline

Status: built by `reco-bjo` (stage `e1.6`), and home since `e10.1` (`reco-ydek.1`) — the view a start
lands on, and the host of the four review acts and their audit trail since `e10.2`
(`reco-ydek.2`, §12 and §13; [ADR-006 §5.1.2](../adr/0006-tui.md)).
Code: `crates/fathom-tui/src/timeline/`.
Surface: view `Timeline` (`t`) in `crates/fathom-tui/src/views.rs`, panes `rail` → `card`; the
state lives in `crates/fathom-tui/src/timeline/mod.rs` (`Timeline`) and is held by `App` next to the
diff viewer's own state, with the acts' rollup the rail's second subject (§13). Bindings:
`src/recommender/tui/keymap.py` (the source of truth, generated
into `crates/fathom-tui/src/keymap.rs` and `docs/adr/0006-tui.md` §7).

The timeline is the screen a reviewer arrives at first and returns to whenever they lose the thread:
*what happened, in what order, and which of it have I not read?* It is a list of episodes — one row per
episode — with grouping, composable filters, quick jumps, and a details pane about the row under the
cursor. Everything below follows from the same rule the diff viewer plays by: **the rail never lies and
never goes quiet.** A risk nothing could rank prints `?`, and a risk the spine did rank prints its
number with `w` beside it ([risk-ranking.md](risk-ranking.md)); a group that is collapsed says how many
rows are behind it; a rail with nothing loaded names the command that fills it. Attribution is not one of
the things the rail went quiet *about*: since `e10.4` it prints no `human`/`agent` word at all, in any
spelling, in any column — the verdict is a datum for `fathom analyze --json` and not a fact to hand a
reviewer ([ADR-006 §5.1.2 §6](../adr/0006-tui.md)).

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

An interactive frame does not freeze at that one read. Held there, a rail ages badly in silence: the
newest row keeps the age it had when the frame opened, and says so for as long as the reviewer leaves
it up. So the loop carries the instant forward on the monotonic clock it already holds for the toast
and the chord timeout (`App::age_clock`, **reco-5oe**) — a monotonic clock is the right instrument for
an age, because NTP cannot step it backwards and make a row younger, and a laptop that slept cannot
make an episode younger than it was when the lid closed. A one-frame render — `--plain`, `--json` — is
exactly that one instant and nothing else, which is what keeps a piped frame reproducible.

An `Episode` carries exactly the fields the rail can name: identity and digests, members, created/last,
author kind/name/confidence, summary, paths, file/add/del/binary counts, review state, session and turn,
boundary rule and confidence, an optional risk, and whether the episode is still live. There is no
`structure` field, no `findings`, no `risk_score` — the card cites the delta fields `e2.11` publishes, and
where the run handed it no document it prints the verb that would fill the row (§8).

## 2 The row, and the six columns it is made of

```
▸ e8f1a2 0:12  +412 −90 ●●●●●● +7 82 ⚑
```

| Column | Source | What it cannot say |
|---|---|---|
| `e8f1a2` | `Episode::short_id` | — (never dropped) |
| `0:12` | `days::relative(now, created)` | a date at narrow widths — the same fact, coarser; `<1m` below a minute, `ahead` for a row the frame has not reached |
| `+412 −90` | `n_add`/`n_del`, compacted | a rename or a mode change — those are the diff pane's |
| `●●●●●●` | `Scale::spark`, a log scale over the loaded corpus | a size comparison across epochs; the scale is what *this* rail holds |
| `+7` / `~+7 (4/5)` / `?` | `delta.complexity` — `EpisodeDelta::complexity`, the roll-up `e8.5` put in the document and `fathom-store` computed at the path grain | a line count: it is a **delta**, signed for exactly that reason, and the sparkline cell between it and `−90` is what keeps ADR-007 §10.1's warning from being true of this row. A floor prints its tilde and its denominator; nothing counted prints `?`, never a `0` |
| `?` / `82` | `Episode::risk`, `Some` from the spine-only ranking (`surfaces`, `size`, `deletions`, `breadth`, `author`) and `None` when the read never reached that episode | a number the reviewer can interrogate with `w`, or `?` — never a `0` standing in for "nobody ranked this" |
| `⚑` | `ReviewState::glyph`: `⚑ ⚐ ✓ ✔ × z ?`, or `gone` | colour: under `NO_COLOR` the glyph is the whole state, which is why it is the last column to go |

The complexity column is `e8.6` (**reco-l9bz.6**), and it is the sixth in the sense that ADR-006 §5.2 counts
them, not in the sense that it is the newest thing on the row: the `?` in the risk column and the `?` in
the complexity column are different unknowns about different things, which is why neither one is allowed
to be the other. A rail whose episodes carry no complexity block prints no cell at all — "this read never
looked" is not the same claim as "this file could not be counted", and the second one is what `?` means
([ADR-007 §7.4](../adr/0007-complexity.md)). When the column is not drawn because the pane is narrow, the
rail's strip says so out loud rather than leaving the gap to be read as a `?`.

There was a sixth column here, `agent`/`human`/`?` after the age, and `e10.4` (**reco-ydek.4**) deleted it
rather than narrowed it ([ADR-006 §5.1.2 §6](../adr/0006-tui.md)). The reason was the rung, not the
reader: an episode with no session arc, no claim trailer and a person-shaped git author string fell
through to `human` at `0.50`, so the cell was confidently wrong in exactly the cases a reviewer cared
about — and the `?` that was meant to be the honest bucket was reserved for a *missing* identity, a
different failure. The datum is untouched: `author_kind`, `author_confidence` and the evidence list are
still written, still correctable with `fathom attribution correct`, and still shipped by
`fathom analyze --json` as an `AttributionRef` carrying all three, so a consumer can weigh the claim
instead of inheriting it. What changed is that no surface prints the verdict where a human reads it.

The age column has four shapes and none of them is a zero (**reco-5oe**): `<1m` below a minute, `h:mm`
below a day, `2d4h`, then `3mo`. `0:00` was what the `h:mm` arithmetic said at forty seconds, and it
read as *this hour* about a row that was seconds old — which is the claim the ticket was filed on.
A row dated ahead of the frame gets `ahead`, and the card's sentence gets *dated ahead of this frame*,
because an age cannot be negative and the frame must not pretend otherwise; that is clock skew or a
fixture dated forward, and saying so is the answer, exactly as
[the glance pane](ambient-mode.md) says `unknown` for a queue it never read.

The column turns on a minute and on nothing finer, and a rail whose rung has already traded the age for
a date turns on nothing until the day does (ADR-006 §0.1: a draw is for a change somebody can see).

The sparkline is a **log** scale against the largest loaded episode, in six cells, clamped to at least
one filled cell: a linear scale puts every ordinary 40-line commit at zero cells beside an agent's
4 000-line turn, and a column of empty cells reads as "nothing happened". A change that is entirely
binary prints `bin` rather than a chart of nothing.

## 3 The column ladder

`Columns::for_width` (ADR-006 §6.3) drops columns in the order the ADR fixes — complexity Δ, then the
sparkline, then relative time to a date, then the risk *number*, then the risk glyph last of all. There is
no author rung any more: `e10.4` deleted the cell, and the six columns it cost were spent on the
thresholds rather than on new content. Every rung above the floor moved down by what the cell cost —
40→34 for the full row, 34→28, 32→26 — and the floor itself did not move, because ADR-006 §3 says where a
row stops being a row and one ticket's tidiness is not a reason to move a line another ticket is standing
on.

`e8.6` then added a rung rather than widening one. A rail between 34 and 37 columns reads `no complexity Δ`
and draws exactly what it drew before, which is the honest shape of the trade: the new column pays for its
own three cells instead of taxing every row that has no figure to draw. And it is the only honest shape for
a second reason — below the sparkline rung there is no cell between `−90` and `+7`, and a bare `+7` after
`+412 −90` reads as a third line count, which is the misreading ADR-007 §10.1 names this column as only
being allowed to exist when it is *apart* from the size columns. So the rail sells the new column before it
sells the separator that makes the new column legible.

| Pane columns | Rung | Row |
|---:|---|---|
| ≥ 37 | all six columns | `▸ e8f1a2 0:12 +412 −90 ●●●●●● +7 82 ⚑` |
| ≥ 34 | no complexity Δ | `▸ e8f1a2 0:12 +412 −90 ●●●●●● 82 ⚑` |
| ≥ 28 | no sparkline | `▸ e8f1a2 0:12 +412 −90 82 ⚑` |
| ≥ 26 | date instead of age | `▸ e8f1a2 14 Oct +412 −90 82 ⚑` |
| ≥ 22 | no risk number | `▸ e8f1a2 14 Oct +412 −90 ⚑` |
| below | id and state only | `▸ e8f1a2 14 Oct ⚑` |

Then a **fit rule**, because the ladder's thresholds cannot promise arithmetic for every terminal: if a
built row is still wider than its pane, the cells go in the same priority order the rungs do — the
complexity coverage ` (4/5)`, then the complexity Δ, then the risk cell, then the change size — and never
the identity and never the state glyph (`render::episode_line`). The ADR's promise is that the *glyph* is
the last cell standing; a row that kept a risk number and clipped the glyph would be keeping the more
decorative of the two. And a row that lost the Δ says so in the strip, because a column that vanished
silently is a column a reviewer will read as an unknown. `tests/golden/timeline/ladder.txt` prints every
rung over one corpus, widths 42 down to 18, so a rung that comes back into existence at a narrower width is
visible in a diff.

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

Six dimensions, and the rule is the store's rule: **OR inside a dimension, AND across them**, so the
same filter means the same rows in the rail and in an `EpisodeQuery` (`filter::Filter::matches` is
written against the same semantics, and `timeline_behaviour.rs` cross-checks the composition against
the model rather than against the rail's own arithmetic).

| Dimension | Values | Notes |
|---|---|---|
| `author` | names, with counts | multi-valued; git's **name**, recorded at commit time — not the retired author-*kind* verdict, which is not a filter any more (below) |
| `state` | `unread`, `read`, `accepted`, `refused`, `dismissed`, `flagged`, `snoozed` | `read` means *opened* — read or any verdict — which is the store's rule, not a shortcut |
| `area` | a path fragment, facet-suggested | matched case-insensitively against every path, so `tests` finds `tests/test_epoch.rs` |
| `since`, `until` | `2026-10-14`, `today`, `yesterday`, `week`, `7d`, `2w`, `month`, `year`, `any` | typed spans resolve to a **bound**, not a suggestion: `7d` becomes the start of the day seven days back and does not move as the frame redraws |
| `group` | `day`, `session`, `none` | the view option, editable here because the surface that edits filters is where a reviewer expects it |

There is no `kind=` dimension, and it went in the same commit as the badge for the same reason
([ADR-006 §5.1.2 §6b](../adr/0006-tui.md)): a rail filter cannot carry a confidence, so offering
`agent`/`human`/`unknown` in the `F` editor meant asserting the verdict in the one surface that had
neither confidence nor evidence beside it — and a rail reporting 300 rows under such a filter asserts that
300 *is* the number of human-authored episodes, which the rung that answers `human` because nothing said
otherwise cannot support. The token the epic did not touch survives: `author=` matches a git name, which
is a fact git states. (`fathom episodes --kind` survives too, on the CLI, where the rows ship their
uncertainty beside the claim.)

Facets (`Facets::of`) are computed from the loaded rows in one pass: authors, path areas up to
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
timeline.filter     = author=pi&author=stahl;state=unread;area=src/store;since=…;group=session
timeline.selection  = e8f1a2…            (the id, first tier of §6)
timeline.members    = e8f1a2:0,e8f1a2:1  (up to SESSION_MAX, the tier-3 ride)
```

`Filter::decode` ignores keys it does not know and defaults what is missing, so a filter written by a
newer fathom degrades to the parts this one understands instead of refusing to start. A key this build
*used* to know is a different case, and it does not go silently: a store written when `kind=` was a
dimension still names a constraint the reviewer set, so `filter::retired_keys` turns it into a sentence on
the resume toast — `session restored — filter state=unread, cursor on 40c9b3; stored ``kind`` filter —
the author-kind filter is retired …` — because a rail that quietly stopped honouring a filter is a rail
lying about what it is showing. Restoring applies
the filter first (it decides which rows exist), then the cursor, then reports what actually came back —
"session restored — filter state=unread, cursor on `40c9b3`", or "`e8f1a2` is not in this index — it
moved or left, and the cursor is at the top instead". A resume that guessed silently is worse than one
that admits it found nothing.

### The strip, and the number that leads it

One line under the rail, built by `Rail::strip`, and it leads with the open-work count since `e10.2`
(ADR-006 §5.1.2 §3 part 2) rather than trailing it:

```text
4 unread · 1 snoozed · filter: area=any state=any group=day (F edits) · 10 of 10
```

| Part | Said when | Why it sits where it sits |
|---|---|---|
| `4 unread` | always, over the rows on screen | the number the four acts move (§12). The strip is sold from the right at narrow panes, so a count at the end of the line is a count nobody at the rail's 42 columns ever sees — and "the strip's unread count drops by one" is worth nothing as a sentence about a hidden part |
| `1 snoozed` | only while it is true | a snoozed row is neither unread nor decided; `z` is what puts rows in that state (§12) |
| `filter: …` | always, and `F edits` is inside it | a live filter is a claim about the *set* and the thing most worth undoing — but it is a claim about the same 10 rows the next two parts count |
| `10 of 10` | always | what the filter kept over what the rail holds; beside a longer page this is where `index holds 3 412 (index epoch 41)` joins it (§9) |
| `3 opened, not marked` | only while it is true | rows that read `unread` in the ledger and were opened anyway — `opened` is not `marked` (`e1.11`) |

Both counts are counted when the order is built, never at draw time, and an act re-counts them where it
stands: a strip that costs O(episodes) per frame is what `timeline_bench` exists to catch (§10), and a
strip that said `4 unread` about a row the reviewer had just signed off would be the rail lying about a
decision (§12). The same rule makes an act re-filter where it stands when a filter is in force — a
`state=unread` rail that went on listing a row that no longer matches is the same lie wearing a filter.

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
say it says precisely: commit count, age, review state, the timezone
the day bounds use, where the change landed (top path prefixes with file counts), the boundary rule and
its confidence labelled "a rule, not a fact" (ADR-003 §4.7), the session and turn, and the member SHAs
the review is keyed to — truncated with `…` and `+N more`, never mid-token. The attribution word used to
close that row, as `4 commits · agent 0.94 · 0:12 ago · unread`; `e10.4` struck the word and its
confidence together ([ADR-006 §5.1.2 §6a](../adr/0006-tui.md)). A reader handed a bare conclusion cannot
argue with it, and the row had no room for what would make one arguable — the rung, the evidence, the
reason a label was refused. The card says nothing about attribution now, rather than saying less of it.

## 9 Empty, filtered and partial states

Every one of these is a sentence in the rail body, in the strip, or both, because "…the pane is empty"
is not an answer (ADR-001 §7.2, ADR-006 §6):

| Situation | What the rail prints |
|---|---|
| there is no index | ``no index here yet — run `fathom index` here, then reopen fathom`` |
| the index is finished and this pane is the queue | ``index epoch 1 ready · 500 commits · 412 eps — `t` opens the timeline, which lists them as the index knows them; `fathom show <episode>` prints one in full`` |
| the index is finished and holds no episodes | ``index epoch 1 ready · 0 commits · 0 eps — the index has nothing to list; `fathom doctor` says whether that is the repository or the run`` |
| the epoch is still `building` | ``index epoch 2 building · 500 commits · 0 eps — the epoch is still being written — wait for it; if a rebuild died halfway, `fathom index --rebuild` starts a clean one`` |
| the store would not open | ``the index would not open — `fathom doctor` says what is wrong`` |
| the caller said nothing (a test frame, an embedding shell) | `nothing loaded yet — the shell was handed no index state` |
| the index answered nothing | whatever `Batch::why_empty` said, verbatim |
| the filter selects nothing | ``the filter selects nothing — `F` edits it, `filter clear` is in the palette`` |
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
* `timeline_acts.rs` — the four review acts taken from the rail (`e10.2`, §12 and §13): the ledger write
  that comes first, the refusing ledger that changes nothing, the run with no ledger that says so, the
  cursor that does not move, the counts the acts move, and `r`'s two states. Three of them are
  `queue_triage.rs`'s, moved rather than rewritten green, and the timed 100-episode budget is `e4.5`'s bar
  re-hosted with the filter and motion in it.
* `timeline_golden.rs` — pinned frames and panes: the ADR's own 80×24 picture, the view's documented
  minimum 61×16, each degenerate state, and `ladder.txt`. Re-bake with
  `FATHOM_GOLDEN=write cargo test -p fathom-tui --test timeline_golden`, then read the diff.
* `timeline_bench.rs` — §10's two measurements, serialised so the allocator's counters mean something.
* `crates/fathom-cli/tests/tui_data.rs` — the read, from outside the process: it builds a 500-commit
  repository with `git fast-import`, indexes it, and asks the shipped binary what its rail draws, so
  that "a fixture handed to the view" is never mistaken for "the binary can fill a rail" — the check
  the walkthrough's first P1 asked for.

## 12 The four acts, taken from the rail

`x` accepts, `f` flags, `-` dismisses with a reason from the closed vocabulary, `z` snoozes until a
condition (`e4.5`, `reco-wsj`). All four are `global` bindings and always were, and `App::act_target`
already addressed the rail's cursor row; what `e10.2` (`reco-ydek.2`) moved is the **host of the
feedback** — the queue's worklist used to be the pane that showed an act happening, and the rail is now
(ADR-006 §5.1.2 §2). Three things had to be true of the rail for that to be a move rather than a loss:

* **The row says it, in the same frame.** The state column is the confirmation: `⚑`→`✔` for `x`, `⚐` for
  `f`, `×` for `-`, `z` for `z`. It is the one cell the ladder promises never to drop, which is why a
  one-cell change is proof and not a thin excuse for it. The strip's open-work count moves beside it (§5).
* **The cursor stays put.** A queue's whole content is undispositioned work, so `x` advancing was the queue
  being useful; a chronology's cursor is a *place in time*, and a keystroke that jumped forward would break
  the next keystroke — which is `d` on *that* episode nine times in ten. The reviewer working a run of rows
  uses the filter that already exists (`F` → `state=unread`, and then an accepted row leaves the filter
  because the filter reads the marker) or walks with `j`. `timeline_acts.rs`
  (`an_act_leaves_the_cursor_on_the_row_it_just_changed`) pins all three acts to one row.
* **Nothing is marked until it is written.** `Ledger::record` runs before any shell state changes and
  `State::commit` returns where the act landed — `Landed::Recorded`, `Landed::InMemory`, `Landed::Refused`.
  A refusal changes nothing anywhere: not the row, not the counts, not the cursor, and the toast says the
  ledger's own words with `NOT written` in front of it. `None` (no ledger at all — a `--plain` frame over an
  unindexed directory, a test, an embedder) is the third answer and not a failure: the act is taken, the
  row marks, and both the toast and §13's strip say out loud that nothing was written this run. That
  three-way answer is why the rule could move to the rail whole; a bare "was it written?" flag marked
  nothing on a rail, because the queue had been hiding the difference behind its own `decided` map.

The pickers are the acts' other half and their host moved with them: `-`, `z` and `f`'s note draw their
chooser in the rail pane, about the row the rail's cursor is on, and the picker's own cursor — not the
rail's — names the reason that gets written.

## 13 The rail's other subject: what was skipped, and why

`r` (`review.retrospect`, `global` since `e10.2`; it was `queue.retrospect` in the `queue` mode) puts the
timeline on screen from wherever the reviewer was standing and has the rail list *acts* instead of
episodes: what was dismissed, by whom, under which reason, over what coverage denominator. `r` again puts
the episodes back; `<Esc>` pops exactly one thing (§R2), which is this listing and nothing else.

What it prints is `fathom_analyze::queue::retrospect`'s rollup, unchanged, and the honesty rule came with
it verbatim: an unreadable ledger prints its complaint and the *nothing dismissed* line is withheld,
because silence about the ledger plus a clean sheet is the lie a reviewer would act on. The strip is
replaced wholesale while the rail is on this subject — `ledger: 41 acts · 12 dismissed · 3 reasons ·
Dana Stahl`, or `no ledger — nothing written this run` — because a footer describing the filter would be
a strip about a list the pane is not showing.

This is not the queue back under another name: the queue showed one set of episodes twice under two
orderings, and this is a different subject with different rows, a different count and its own denominator.
The deep form is still the verbs' — `fathom review list --state dismissed`, `fathom review status`, `fathom
review verify` — and the ledger itself did not change at all ([review ledger](review-ledger.md)).

## 14 Deliberately not here yet

`e10.1` retired the queue as the home view; `e10.3` deletes it. The risk number in the rail and in the
card is still the spine-only ranking, and its published ceiling is
[risk-ranking.md](risk-ranking.md) §12 — which since `e10.1` is the record of a **retired** ordering and
no longer describes what the product ranks by. `e10.4` took the `human`/`agent` badge out of this view's
row, out of the card's header and out of the `F` editor, and spent the six freed cells on the ladder's
thresholds instead of on new content ([ADR-006 §5.1.2
§6](../adr/0006-tui.md)) — and `e10.5` replaces what the risk number is computed from, including the
`author` component §8's line still spends points on.

`delta.*` and `findings[]` rows in the card (`e2.11`, `e4.3`); the *finding-shaped* risk number the
queue once ordered by — what it orders by is the spine-only ranking and its stated ceiling
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

`e10.2` left its own list behind, and these are the parts of it that are *decisions* rather than unfinished
work: the queue's pane is still on screen and still owns `s`, `J`/`K`, its run mode and its flip until
`e10.3` deletes it, so until then a disposition reads the same marker in two places; the rail's act listing
is a **read** of the ledger and nothing else — retracting, reclassifying and bulk-applying stay the verbs'
(`fathom review retract`, `fathom review apply --all`, `fathom review verify`), because a rail that could
edit an act would be a second place a decision could change and the ledger is one place on purpose; and the
`-`/`z`/`f` chooser is `e4.5`'s, re-hosted unchanged into the rail pane — it is not `F`'s filter editor and
never was, and the two surfaces did not merge.
