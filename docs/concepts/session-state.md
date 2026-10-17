# Session state — what a repository remembers about the last reviewer

Status: built by `e1.11` (**reco-6hp**). Code: `crates/fathom-tui/src/session.rs` (what a session *is*:
the keys, the encoding, the restore), `crates/fathom-cli/src/session.rs` (the writer: load, quarantine,
conditional save), `crates/fathom-store/src/episode.rs::ui_state*` (the rows). Loaded and saved by the
`tui` verb in `crates/fathom-cli/src/tui.rs`.

The premise of the whole view is ADR-001 §7.7: *a reviewer is reading, not browsing*, which makes an
afternoon of review a **session** rather than a command. A session has a place the reviewer stopped at, a
set of things they decided were not the question, and a window they made the right size. Throw all of
that away at quit and the tool is a `git log` with extra steps: every relaunch starts at the top of a list
the reviewer already walked.

So: **quitting the TUI saves it, and reopening restores it.** The state lives in the store, next to the
index that produced it, because "where I got to" is a fact about this repository — a different repository
opens with a different filter and a different unread set, and neither of them is anybody's global default.

## 1 What counts as session state

Everything the reviewer set that is not a decision about an episode. The line matters: `EpisodeRepo`'s
ledgers (`mark_viewed`, `set_status`, `set_starred`) are **findings** — what fathom concluded, what the
reviewer decided about a change — and they are keyed to episode identity, written when the action happens.
Session state is the reviewer's posture, has no evidentiary value, and is worth losing.

| Key | Holds | Restored as |
|---|---|---|
| `tui.state_version` | the schema number of the writer (`1`) | never applied; it is read to decide what to do with the rest |
| `tui.view` | `timeline` / `brief` / `graph` / `impact` / `risk` / `review` / `status` | the focused pane |
| `tui.pane` | `main` / `list` / `detail` / `aux` | which pane a bare keystroke acts on |
| `tui.zoom` | `off` / a pane name | `zoom.pane` |
| `tui.size` | `colsxrows` — the last frame's size | the viewport, and the width a `fathom --plain` frame is folded to |
| `tui.pane_sizes` | `pane:cols:rows`, comma-joined | every named pane's own size |
| `tui.tabs` | `view:title`, comma-joined in left-to-right order | the tab strip |
| `tui.mark` | `panel:x:y` | the jump mark (`)`) |
| `tui.search` | a pattern | the search field's default (`/`) |
| `timeline.filter` | `timeline::session::encode`, six segments | the rail's filters — [timeline §5](timeline.md#5-filtering) |
| `timeline.selection` | one episode id | the rail's cursor, by identity through the relink tiers |
| `timeline.members` | `author:` / `kind:` / `area:` / `repo:` / `remote:` / `sha:` prefixes | each filter's multi-member set |
| `unread:<episode-id>` | `opened` / `read`, written when the card materialised | the read set, and therefore where `goto.first_unread` lands |

`session_state` is the store's own table (`episode.rs`, since `e1.3`) with `key TEXT PRIMARY KEY, value
TEXT, updated_utc INTEGER`. The write path is `Store::ui_state_rows` (sorted by key, so a diff is
comparable) and `Store::set_ui_state_if`, which is the whole concurrency story and is [§3](#3-two-tuis-one-repository).

Nothing else is stored here, and that is a rule rather than an omission: [materialisation](materialization.md)
§7 refuses a persisted decode cache for the same reason — bytes that can change underneath a rendering are
not a rendering, and the review ledger is not a preference.

## 2 One grammar, so a bad value is a small bad value

The ticket's third criterion is that corrupt or mismatched state *degrades*. That is decided by the shape
of the rows, not by a recovery handler:

* **one key per fact**, never one JSON blob. A blob has two states, valid and useless; rows have a state
  per fact, so a torn value costs the tab strip and not the afternoon. `tui.tabs` is the blobbiest row in
  the set and still only loses its own row.
* **a total decoder**. `restore` classifies every row into *applied*, *newer than this build*, or *damaged*,
  and every failure adds a warning and continues. There is no error case, because the only thing in the way
  of a warning about one field is the frame.
* **a grammar with no escapes to get wrong.** `[a-zA-Z0-9_.:@/-]+` is the open set; anything else is
  percent-encoded, `,` and `=` are encoded in values because they delimit lists and pairs, and control
  bytes are encoded in keys, because SQLite will happily store a newline and the key is printed in a
  terminal by `--show-session`.
* **a version that only ever moves forward.** `tui.state_version` is written and never applied; a number
  above this build is reported ("was written by a newer fathom; read as far as this build understands")
  and read as far as it can be, rather than refused. `--fresh` is the escape hatch for that case, and the
  same advice the `timeline.filter` panel already gives when a filter hides everything.

`decode` (in `fathom-cli`) is where the two damage stories split, and the split is what the criterion
actually asks for: rows this build does not *own* (`known()`) are unknown, not corrupt — an unrelated
subsystem's `session_state` rows, or one written by a newer fathom, must never be quarantined out of the
store. Only a key that should decode, and does not, is damage.

## 3 Two TUIs, one repository

Two sessions in the same repository have always been fine on episodes — `index.rs`'s header comment says
the write path is one short transaction and a reader just waits. The naive version of *this* feature is
last-writer-wins over the whole state: the first TUI to quit silently deletes the afternoon of the one that
quit second.

The fix is a compare-and-set, and it is one SQL predicate:

```rust
Store::set_ui_state_if(key, expected, value, now) -> Result<bool>
// UPDATE … WHERE key = ? AND value = ?   ·   DELETE … WHERE key = ? AND value = ?
```

`save` writes the **delta** since the baseline read at startup, and each row's compare value is whatever
this process read at startup — including "nothing", for a row it is creating. So a session commits exactly
the rows it changed and nothing else. Three outcomes per row, each of which gets a sentence:

| Outcome | When | What the reviewer hears |
|---|---|---|
| applied | the row still says what this session read | (nothing — it just works) |
| left alone | another session wrote it in the meantime | `session kept another session's state for <view> · <n> row(s): this session's changes to them were not written` |
| unchanged | the value equals what is already there | (nothing, and no write) |

A lost race is not a rollback and not a merge: the other session's layout stays, this session's other
choices are saved, and the reviewer is told which part of what they did was dropped. `tests/cli.rs`-style
coverage of all three is in `crates/fathom-cli/src/session.rs`'s tests (the CLI's Rust unit tests are the
only place in `fathom-cli` with a store handle).

`--fresh` reads the state and then declines to apply it, which is what lets a fresh run *also* be a session:
its baseline is the real one, so its own unchanged-nothing run still writes nothing back, and the state it
ignored is still there for the next launch.

## 4 Damage: three doors, all of them the same advice

| Door | Detected by | What happens |
|---|---|---|
| the file is not SQLite (a truncated copy, the wrong extension feature, someone else's bytes) | the first SQL statement fails with `SqliteFailure`, and `read_maybe_sqlite` confirms the 16-byte header is not `"SQLite format 3\0"` | the whole store — `.sqlite3` plus its `-wal`/`-shm` siblings — is **moved** to `.fathom/quarantine/` and the run starts clean |
| the store opens but a session row is damaged | `decode`, over keys this build owns | the damaged rows are copied to `.fathom/quarantine/session-state-<seconds>.json` and deleted from the store; the good rows apply |
| a value decodes but is unusable (`zoom=nowhere`, `size=0x0`, `unread=maybe`) | the typed setters (`set_zoom`, `set_size`, `set_read`) reject it | the default stands, plus a warning naming the key |

Moved, not deleted: "start clean" is not licence to destroy, and a reviewer's own experiment with `sqlite3`
should be reversible. The third door stays inside a run — a warning and a default is a state a reviewer can
keep working in, and nothing about it is worth quarantining a file for. The `index` verb's existing advice
(`delete .fathom/index.sqlite3-wal and re-run`) is the same philosophy one layer down.

Every path writes warnings to **stderr** and exits 0. A session that came back wrong is a remark, not a
failure — the exit code is reserved for "fathom did not do the thing".

## 5 `--fresh`

`fathom --fresh tui` (the flag is global, and inert on the read-only verbs, which draw no frame):

* reads the state, applies **none** of it, and says so with the default it fell back to;
* starts at the timeline, first page, no filter, unread set empty, this terminal's size;
* writes back on exit **only if** the run changed something — which a look-and-quit run does not, so a
  `--fresh` probe leaves the stored session exactly as it found it.

The "changed something" question is answered by `session::delta`, the same diff the loop's own baseline is
taken from, so the promise is not a special case: looked at and left alone is not a change.

## 6 The four criteria, and what proves each one

| Criterion | Test |
|---|---|
| reopening restores selection, filters and layout exactly | `a_reopened_session_comes_back_exactly`, `the_round_trip_is_stable_so_a_resume_never_drifts`, `the_frame_comes_back_at_the_size_it_was_left_at` (`crates/fathom-tui/tests/session.rs`); `a_session_state_row_survives_a_reopen` (store) |
| two TUIs in the same repository do not clobber each other's state | `two_sessions_write_without_clobbering_each_other`, plus the store's own `a_session_state_write_lands_only_on_the_value_the_writer_read` (`crates/fathom-store/tests/episodes.rs`) |
| a corrupt state file is quarantined, and the app starts clean with a warning | `a_store_that_is_not_a_store_is_moved_aside_and_the_session_starts_clean`, `a_damaged_row_is_quarantined_and_the_good_rows_survive`, `a_session_refuses_to_clobber_a_store_it_never_opened`; in the shell: `printf 'not a database' > .fathom/index.sqlite3 && fathom tui --plain` |
| `fathom --fresh` starts from defaults and writes nothing it did not change | `a_run_that_drew_frames_and_took_no_keystroke_asks_for_nothing`, `a_run_that_changed_nothing_leaves_the_state_alone`, `nothing_changed_writes_nothing_at_all`, `a_run_with_no_rows_at_all_is_a_first_session_not_a_damaged_one` |

Plus, because the seams are where the bugs live: `every_key_the_shell_writes_is_one_the_writer_owns`
(fails the moment `fathom-tui` writes a key `fathom-cli` would not know what with), `an_episode_opened_with
_enter_is_in_the_read_set` and `a_read_set_from_an_earlier_session_moves_the_first_unread_jump` (the read
set is real state, not decoration: `e1.6`'s `goto.first_unread` consumes it), and
`a_state_from_a_newer_fathom_is_read_not_refused`.

## 7 What is deliberately not here

**No global or cross-repository session.** `tui.*` keys are per-repository on purpose. A reviewer's
*preferences* — theme, noise policy, page size — are `fathom.toml`'s (`e0.8`), which layers and validates;
this table is only for what is true of one repository's review.

**No per-reviewer identity.** `reviewer.id` decides which review is *mine* in the ledger; it does not fork
the session table. Two humans sharing a clone share a session, and the compare-and-set is what makes that
survivable rather than lucky.

**Nothing is written on every keystroke.** One read at start, one write at exit. A store that is closed
while the terminal is drawing is a store that cannot be blamed for a torn frame.

**No store handle in `fathom-tui`.** The shell returns `Vec<(String, String)>` and accepts the same; the
CLI owns every SQL statement (ADR-002 §3, lint `W1`). That is why the key list is asserted in both
directions in `crates/fathom-tui/tests/session.rs` — the owning set is a fact two crates hold separately.

**No migration machinery.** `STATE_VERSION` plus "read as far as you can, say what you could not" is the
whole plan at two versions. If a version ever needs translating, `tui.state_version` is the marker and
`tui.<name>` is the only namespace it governs.

## 8 Where to look

| Question | Place |
|---|---|
| What the state is, and how a bad row degrades | `crates/fathom-tui/src/session.rs` |
| Reading it, quarantining damage, conditional write-back | `crates/fathom-cli/src/session.rs` |
| Where a run loads and saves it, and what `--fresh` changes | `run()` in `crates/fathom-cli/src/tui.rs` |
| The rail's own half of the keys | `timeline::session` in `crates/fathom-tui/src/timeline/session.rs` |
| The read set, and what consults it | `Rail::seen`, `Read::Opened` in `crates/fathom-tui/src/timeline/rail.rs` |
| The compare-and-set | `set_ui_state_if` in `crates/fathom-store/src/episode.rs` |
| What state does to a drawn frame | `compose` in `crates/fathom-tui/src/frame.rs` |
| `fathom tui --show-session` | `show()` in `crates/fathom-cli/src/session.rs` |
