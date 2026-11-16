# The review ledger and the review memory: sign-offs and notes that outlive the history they were written about

- **Status**: shipped — two pages' worth of it, and in one page. The ledger shipped in `e4.6` (**reco-rlf**)
  and the memory in `e6.5` (**reco-552**), and each of the two pages re-introduced the same apparatus to
  itself: class **M** because committed, keyed to episode identity, a digest on every block, mirrored into
  the store, auditable with no index. `reco-lpuj` merged them under
  [the documentation merge map](../reference/doc-map.md)'s M7 — a row that already said which sections went
  where, and did not re-derive them. The retired path is a signpost now, and nothing was dropped: every
  claim, table row, refusal, measured number and rendered sample from both pages is here, each in exactly
  one place.
- **Merged from**: `review-ledger.md` (`reco-rlf`, `e4.6` — the sign-offs) and `review-memory.md`
  (`reco-552`, `e6.5` — the decisions, questions, follow-ups and annotations). One directory is the reason
  they are one page: `.fathom/reviews/`, and one question — *what did a human say about this change, and
  where does it go when the pull request closes*.
- **In code**: `crates/fathom-analyze/src/ledger/` (`entry`, `format`, `relink`, `collect`) and
  `crates/fathom-cli/src/review.rs` for the sign-offs; `crates/fathom-analyze/src/notes/` (`entry`,
  `format`, `collect`) and `crates/fathom-cli/src/notes.rs` for the notes. The store mirrors are
  `crates/fathom-store/src/ledger.rs` and `crates/fathom-store/src/notes.rs`; the agent door is
  `review_memory` / `add_review_note` in `crates/fathom-mcp/`.
- **Decided by**: [ADR-004 §10](../adr/0004-storage.md) (durability classes — class **M** is must-survive
  *because it is committed*) and [ADR-003](../adr/0003-episodes.md) (identity, which is what a sign-off and
  a note are each keyed to), and ADR-004 Appendix A (the `decision_note` table the memory mirrors). The
  acts' host and the retrospective's home changed in [ADR-006 §5.1.2](../adr/0006-tui.md) (`e10.1`); **the
  ledger did not change at all**, which is what
  [the last section](#what-a-retired-view-does-not-take-with-it) is on the record for.
- **Read this if**: you have signed something off and want to know what that actually did, you want a CI gate
  that means "a human looked", or you have a question a review left open and want it to stop being open.

The **review ledger** is the durable, human-readable, committable record of what human beings decided:
sign-offs, notes, dismissals with their reasons, and flags. It is markdown under `.fathom/reviews/`, keyed
to episode identity plus a content fingerprint, with a mirrored index in the store so a surface can query it
without parsing markdown.

The sentence that explains why it exists at all is ADR-004 §10's: *nothing inside `.fathom/` is
must-survive by virtue of being there*. A sign-off that lived only in a SQLite file is one careless
`git clean -fdx` away from never having happened — and the metric this whole product is built on ("a human
reviewed it") would keep counting it.

| | `review_state` (a store column) | `triage_event` (the act log) | **the ledger** |
|---|---|---|---|
| survives `rm -rf .fathom/` | no | no | **yes — it is committed** |
| travels in the pull request | no | no | **yes** |
| says what was reviewed *and what it was* | a state | a state | four digests and the member set |
| can be edited silently | in the store, out of sight | same | **no: every entry hashes itself** |

A review produces more than a verdict. It produces *"we deliberately allow this cycle because the port is
the boundary"*, *"does anything still call the old entry point?"*, *"follow up on this once the migration
lands"*. That knowledge is the part that normally dies in a pull-request thread. The **review memory** is
where it goes instead: the same markdown, under `.fathom/reviews/notes.md`, committed and reviewable in the
same pull request as the change it describes, and mirrored into the index for the same reason.

One page now, and the two files stay two nouns on purpose: **the ledger is a gate** — four dispositions, one
per reviewer per change, and the north-star metric counts it — and **the memory is a record**: plentiful,
free-text, opened by a question and closed by an answer. A note never counts as a review, and a sign-off
never carries a paragraph of reasoning — each says what it says.

| | the ledger | the review memory |
|---|---|---|
| file | `.fathom/reviews/ledger.md` | `.fathom/reviews/notes.md` |
| unit | a disposition by a named human | a decision, question, follow-up or annotation |
| answered by | a later disposition | a later note with `about:` naming it |
| counts as a review | **yes** (`accepted` only) | no |
| volume | one block per (episode, reviewer) | as many as the review produced |

## What a sign-off says

Four decisions, and they are the whole vocabulary — the same four dispositions the acts offer, taken from
the timeline's rail as of `e10.1` ([glossary](thesis.md); the acts and their keys):

| decision | means | clocks the north-star metric |
|---|---|---|
| `accepted` | "I understand this and I accept it" | **yes** — this is *the* sign-off |
| `flagged` | "look at this, someone else" — carries who was asked | no |
| `dismissed` | "not a problem, for a listed reason" | no, and the reason is kept |
| `snoozed` | "not now, and here is what I am waiting for" | no |

An entry carries the decision, the reviewer identity, the instant, its scope, **four digests**, and — for a
dismissal — the reason. Four digests rather than one, because each answers exactly one question, and
"was this the same thing?" is not any of them. The one this ticket added is the **content digest**: a digest
over the final content of each path the episode touched, which is what makes staleness decidable at all. It
deliberately ignores commit order, subject and message wording — a rebase reorders paths and rewording a
subject is not a re-review.

```bash
fathom review sign <episode> --note "read the migration; deploy order is in the note"
fathom review status            # is every episode covered by a sign-off that still matches?
fathom review list --state stale
fathom review verify            # tampering check; reads no index, works in a fresh clone
fathom review sync              # render the acts the shell wrote as ledger entries
```

`fathom review status` on a freshly indexed repository with nothing signed off — the miniature from
`fathom --demo`, so these are bytes someone can reproduce:

```text
$ fathom --demo --plain review status; echo "exit=$?"
ledger    0 file(s) · 0 entries · 0 fresh · 0 stale · 0 orphan · 0 tampered
episodes  21 live · 0 with a fresh sign-off · 21 without
verdict   NOT ALL REVIEWED
  blocks: 21 of 21 live episodes have no fresh sign-off
exit=7
```

`7` is `CHECK_VIOLATIONS`, the code a pipeline branches on, and reusing it rather than inventing a code is a
decision: the ledger's use is that a gate can depend on it. The message names every blocking reason, because
an exit code that says "no" without saying why is the reason people delete gates.

After one sign-off in that same miniature — the identity line matters, and so does the second `blocks:` line,
which is the miniature telling you something about itself:

```text
$ fathom review sign ep-06cf9f46eaf35e6b071c --note "read the diff; the retry ceiling is what the pager budget needs"
signed    ep-06cf9f46eaf35e6b071c  accepted · Dana Stahl <dana@example.com> · 1 commit(s), 1 file(s)
written   .fathom/reviews/ledger.md
  · identity from git config
still open  20 unsigned · 0 stale · 0 orphan · 0 tampered

$ fathom review status; echo "exit=$?"
ledger    1 file(s) · 1 entries · 1 fresh · 0 stale · 0 orphan · 0 tampered
episodes  21 live · 1 with a fresh sign-off · 20 without
verdict   NOT ALL REVIEWED
  blocks: 20 of 21 live episodes have no fresh sign-off
  blocks: `.fathom/reviews/ledger.md` is ignored by git: a sign-off git will not add is one clean from never
          having happened — un-ignore the directory (`!.fathom/reviews/`, the line ADR-004 §12's stanza requires)
exit=7
```

That objection is not a bug in the miniature: the bundled repository ignores `.fathom/` wholesale, and a
sign-off nobody can commit is a claim, not a record. Your own repository should carry the un-ignore line ADR-004
§12 specifies — `!.fathom/reviews/` — and `fathom review status` will keep telling you until it does.

## What a note says

```markdown
## nt-725876132d326f08 | decision | 2026-10-23T09:47:14Z | Ana Dev <ana@example.com>

- kind: decision
- episode: ep-f041c51ecf0e8f3ace87
- author: Ana Dev <ana@example.com>
- author-kind: human
- written: 2026-01-04T10:05:00Z
- text: we allow the cycle between m1 and m2 because the port is the boundary
- target: src/m1.rs:1
- fingerprint: f041c51e…      <!-- author+subject: the strong identity -->
- fallback: c99b26f4…         <!-- author+subject, the weaker form -->
- shape: d5371b6e…            <!-- author+paths: what a reword rides on -->
- content: 4d6c788d…          <!-- the content it was written against -->
- members: 289d49b8, 5caf3a4e
- paths: src/m1.rs@fd63fdcbf8f1
- via: fathom note add
- digest: sha256:2be40d4e…
```

Four kinds, and the vocabulary is ADR-004 Appendix A's, not a new one:

| kind | means | open until |
|---|---|---|
| `decision` | why the code is the way it is, written down | answered, if somebody answers it |
| `question` | what a reviewer did not understand, or did not accept | a later note names it in `about:` |
| `followup` | work somebody owes | a later note names it in `about:` |
| `annotation` | a remark attached to a line or a change, with no ask in it | — |

A note is attached to an **episode** (by its identity digests, exactly like a
[sign-off](#what-a-sign-off-says)) or to a **path**. A path-anchored note is a real anchor, not a failure: the
ladder reports it as `anchored` and `fathom context <path>` finds it.

### Appending, never editing

The file is append-only, and every block ends with a `digest:` line hashing its own lines. So:

* **answering** a question appends a new note with `about: nt-…`. The question's block is not touched, and
  its status becomes `answered` because a later block names it.
* **withdrawing** a note appends a new one with `withdraws: nt-…`. Nothing is deleted; a retracted
  decision is still a fact about the review.
* **editing** a committed block is the tampering case, and it is checked the same way the ledger's entries
  are: [Tampering, and why `verify` needs no index](#tampering-and-why-verify-needs-no-index) owns that rule
  for both files, and `fathom note verify` is the memory's copy of it.

Unknown fields are *reported*, never dropped: a note written by a newer build and read by an older one
tells the older one what it could not understand, in the same way the ledger does.

## What the file says

`.fathom/reviews/ledger.md` after that one sign-off, with its header comment trimmed (the header is the
instruction to the next human: do not edit a committed block, append a new one):

```text
# Fathom review ledger
<!-- format: fathom-review/1 -->

## sg-a11885f50b78cc97 | accepted | 2026-10-20T01:58:47Z | Dana Stahl <dana@example.com>

- episode: ep-06cf9f46eaf35e6b071c
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2026-10-20T01:58:47Z
- fingerprint: 06cf9f46eaf35e6b071cf15cf2d8bf6549505f9d7ac6d7ea1a973b258e407483
- fallback: 86a37980e28ff91a54b16da0e887382290116cecd2e36cce07f92740677353a7
- shape: 1e87b4547e438d66febb1eac284d262256aa4a7a9ecc9642d99f5a4455593421
- content: f69ca480f4c1132ba2b9a580b48043723f24aae27a6860df18d5b1a1c2239eab
- members: d367b98d
- paths: Cargo.toml@0c0881be7651
- scale: 1 commit(s), 1 file(s), +1 -0
- note: read the diff; the retry ceiling is what the pager budget needs
- digest: sha256:b5d11e24a16e6d3777f1a625de5e0b1cd2818a83b6debfce88f713f93fc694de
```

Four digests, one `members:` line, one `paths:` entry per touched path with the blob it saw, and the entry's
own hash last. Editing that note by hand after the fact is caught: `fathom review verify` re-hashes every
entry against its `digest:` line and exits `7` naming the line, and `fathom review status` reports the entry
as tampered rather than fresh — so the sign-off stops counting as a review the moment its text changes:

```text
$ sed -i 's/read the diff/read the whole diff/' .fathom/reviews/ledger.md
$ fathom review verify; echo "exit=$?"
checked   1 ledger entry
  · .fathom/reviews/ledger.md:32: entry `sg-a11885f50b78cc97`: its `digest:` is sha256:b5d11e24… and its
    lines hash to sha256:da51d421… — the text changed after it was written
verdict   the ledger does not match itself — treat these entries as unverified
exit=7

$ fathom review status | tail -3
  · .fathom/reviews/ledger.md: fresh (TAMPERED) add toml to the dependency surface
verdict   NOT ALL REVIEWED
  blocks: 1 ledger entry is tampered: its text no longer hashes to its own `digest:` — run
          `fathom review verify` for the line numbers
```

(The long digests are elided in that block so the lines fit; the real output prints them in full.)

The stamp is to the whole second, and that is a rule rather than a rounding: `entry_id` and `digest` are derived
from the text of the file, so a sub-second remainder would make the entry an act read from the act log and a
different entry read from the markdown, and `fathom review sync` — whose whole contract is that it is re-runnable
— would render the same keystroke twice, the second copy landing as a sign-off nobody made and the mirror's
primary key rejecting its own id. So a stamp is cut to the resolution the file can carry on its way in
(`ledger_clock`), which makes the round trip through the markdown an identity.

## Who signed

Resolved in this order, first answer wins: `--identity`, `$FATHOM_REVIEWER`, `[review] identity` in
`fathom.toml`, then git's `user.name` / `user.email`. An answer of `unknown` is no answer: **a sign-off
without an identity is not a sign-off**, so the verb refuses with the four knobs that would fix it rather
than storing a review nobody can attribute. `--agent <name>` records an act as an agent's verdict *on behalf
of* someone, which is a different thing from a human acceptance and is rendered as such.

## The verdict, and the three states that are not "reviewed"

`fathom review status` answers one question: *is every change here accepted by a human, and is what they
accepted still what is here?* The states come from the re-link ladder:

* **`fresh`** — the episode is live and the content digest matches. This is a review.
* **`stale`** — the sign-off re-linked, but the content has since changed. The old judgement is *evidence*,
  not a pass. This is the state that makes an amend after review visible instead of comfortable.
* **`orphan`** — the entry cannot be re-attached to anything live. Fail-visible by design: the work comes
  back as unread on the rail rather than being quietly treated as reviewed.
* **`unknown`** — the content digest was never computed, so staleness is undecidable. Reported as stale with
  the reason "content was never measured", because a digest that cannot be computed is not a pass.

Which question the ladder's first rung asks matters more than it looks. An episode id can survive a rewrite and
still name nothing that exists: the segmentation pass keeps a dead row when it carries a decision (ADR-003
§6.5) so the sign-off reads from either end, and the marker an act leaves in the TUI is exactly such a row.
`exact` therefore means "the id names a **live** episode"; a dead one is not an answer but the absence of one,
and the ladder goes on to the tiers that find the successor a reword moved the change onto (§5.5), keeping the
dead row for the orphan's sentence when nothing answers. Stopping at the id instead orphaned every sign-off the
product itself had recorded, while the store re-attached the same marker one table away
(`a_reword_after_a_queue_act_relinks_the_sign_off_on_the_shape_tier`).

Two denominator rules are worth knowing because they are the difference between a gate that closes and one
that whines forever: an episode whose paths are all under `.fathom/` is **excluded** (the commit that records
the sign-offs is itself an episode, which would want a sign-off, forever), and an episode with no paths at all
**stays in** — "there was nothing to look at" is a claim worth a second look.

## The dismiss vocabulary is closed

Dismissing requires a reason from a fixed set, not free text. That is not pedantry: a closed set is what makes
systematic dismissing a number on a screen instead of a feeling, and `review.retrospect` — `r`, which since
`e10.1` is a state of the timeline's rail listing *acts* rather than the queue's worklist flip (ADR-006
§5.1.2), with `fathom review list --state dismissed` as the deep form — is the rollup that reads it. The reasons are also what a *new* reviewer inherits — "why did we stop caring about
this" is a question whose answer has to be in the record, and free text five months later is a shrug in
prose.

## Tampering, and why `verify` needs no index

Every entry hashes itself, so editing an entry's text after the fact is detectable
(`fathom review verify`, exit `7` on anything wrong). `verify` reads only the ledger files — no episodes, no
store — which is the case the check exists for: a fresh clone that has never been indexed can still learn that
a sign-off was edited. The entry it re-hashes, and the `sed -i` that gets caught, are printed in
[What the file says](#what-the-file-says); what is worth saying twice is only the boundary — the check needs
nothing but the file, so it never waits on an index.

The memory sits under the same rule and has its own verb. `fathom note verify` re-hashes every note block and
names the ones whose text no longer matches, and it needs no index and no store either, so a fresh clone can
audit the memory before it builds anything. It is the check
[*Appending, never editing*](#appending-never-editing) exists to make decidable: the file is append-only, so a
block whose text no longer matches its own `digest:` was rewritten rather than added to.

## Surviving a rebase

A note keyed to a commit id is worth nothing after a rebase, and a note that silently disappears is worse
than one that never existed: the reader cannot tell a deleted decision from a rewritten history. So a note
carries the same four digests a sign-off does and goes through the
[same re-link ladder](#the-verdict-and-the-three-states-that-are-not-reviewed), rung by rung: exact id →
fingerprint → shape → member-set. What the ladder could not place is reported as `ORPHAN` with the reason,
and stays in the file and in every report.

That is the acceptance criterion, measured in `crates/fathom-analyze/tests/notes.rs`: notes written against
a twelve-turn branch, the tail rebased under new subjects and new commit ids, and every note still attached
— the ones whose episode identity is gone re-linked on a weaker rung, with `relinked_from` and the rung's
name printed so a reader can see how sure it is.

## The store is a mirror

`decision_note` (migration 10) holds the file's content plus the verdicts: the link's method and score, the
state, the derived status, and the tamper flag. It is class **R** by copy — delete it and `fathom note list`
rebuilds it from the markdown, which is why the write path is file-first: append, fsync, then mirror. A crash
between the two loses a row, never a note. The ledger's `ledger_entry` mirror is the same arrangement, and
its recovery action is stated in [How it is stored, and why markdown](#how-it-is-stored-and-why-markdown).

## Asking what the repository knows

```console
$ fathom context src/payments/ledger.rs
context   src/payments/ledger.rs · 2 episode(s) touched this path
  episode ep-f041c51ecf0e8f3ace87 · 2 note(s) · feat: payment boundary
decisions
  [decision] 2026-01-04T10:05:00Z · Ana Dev <ana@example.com> · we allow the cycle because the port is the boundary
      · episode ep-f041c51ecf0e8f3ace87 — feat: payment boundary
      · fingerprint f041c51ecf0e
      · members 289d49b8, 5caf3a4e
      · target src/m1.rs:1
      · .fathom/reviews/notes.md#nt-725876132d326f08
      · digest 8fb90a2926fc
memory    1 open question(s) or follow-up(s) of 2 note(s) here
```

One query answers *"why did we do X"*, *"who said so, and when"*, and *"what is still unanswered"* — for a
path (every episode that touched it, newest first) or for an episode id. The same structure backs
`fathom context --json`, the MCP tool `review_memory`, and the [export artefact](export-artefact.md)'s
review-memory block, so the three cannot disagree.

The notes are also in the search corpus, so `/` and `fathom search kind:note` find them by their words:
that is what makes a recorded decision findable by someone who does not know it was recorded.

The library tests (`crates/fathom-analyze/tests/notes.rs`) hold the rules; the verbs are driven as a binary
in `crates/fathom-cli/tests/notes_verb.rs`, which is where the exit codes, the `--json` contract ids, the
refusals that leave the file byte-identical and the search corpus are actually checked. `fathom review`
does the same job for the sign-offs, tested in `crates/fathom-cli/tests/review_verb.rs`.

## Through the agent door

`add_review_note` appends to the same file `fathom note add` appends to, with `via: fathom mcp
add_review_note` and the author recorded as an agent (or as the human named in `reviewer`, which the file
then says). `review_memory` reads it back with the evidence lines attached, and is a **read** tool, so a
read-only session can answer *"why did we do X"* — which is the question that used to have no answer.

| field | means |
|---|---|
| `status` | `open`, `answered`, `withdrawn` |
| `link_state` | `fresh`, `stale`, `orphan`, `anchored` |
| `relinked_from` | the episode it was written against, when a rewrite moved it |
| `evidence` | the citation lines: episode, digests, member commits, target, file anchor, digest |

The write half is gated like every write: `mcp.allow_writes`, an audit line per call, and a body bounded and
redacted on the way in — the file is committed, so a secret pasted by an agent would otherwise ship in a
pull request.

---

## What a retired view does not take with it

`e10.1` (`reco-ydek.1`) retired the attention queue as a surface and `e10.3` (`reco-ydek.3`) deleted the
pane. The ledger is the reason that was a *pane removal* rather than a *feature removal*, and the boundary
was worth writing down before the deletion happened, because every part of it is a place a sign-off could
go invisible:

| what the queue held | where it is after the pane | what proves it survived |
|---|---|---|
| the four acts | bound in `global`, taken from the rail (`e10.2`); write-before-state-change intact | `crates/fathom-tui/tests/timeline_acts.rs` holds the write-first, refusal-keeps-the-row and no-ledger cases, moved whole out of `queue_triage.rs` by `e10.2` rather than rewritten green |
| the act log (`triage_event`) | **unchanged, including its name** — the table is about acts, and the queue was a reader of it | no migration: `SCHEMA_VERSION` does not move, so a store written with the queue opens without it and vice versa |
| the ledger (`.fathom/reviews/`) | untouched; class **M** as ever | `fathom review sync` still renders a queue-era act, `fathom review verify` still passes, and `fathom review status` still calls those episodes `fresh` and counts them in the verdict |
| the stamped risk on an act | history, never recomputed: an act signed under `spine-1` prints `spine-1` after `e10.5` changes the vector | `fixtures/queue-era/`, below: the frozen store's acts come back with the model name that scored them |
| the retrospective | `r`, a state of the rail listing acts (`e10.1`), with `fathom review list --state dismissed` as the deep form | the rollup prints the same dismissals, actors and reasons it printed in the worklist, and still says out loud when nothing was written this session |
| persisted shell state | a stored tab naming the dead view, or a stored author-kind filter, decodes to the home view plus a warning — never to a vanished cursor | the session-state decoder's existing *applied / newer / damaged* rule gains the warning |

The check `e10.3` owed, in one sentence: **open a store written by the build that still had the queue** — a
committed fixture at the last `SCHEMA_VERSION` that build wrote, per
[fixtures](fixtures.md) — and assert every act reads back (disposition, actor, reason, note, snooze
condition, stamped risk and model), `fathom review sync` renders each as a ledger entry, `fathom review
verify` is clean, and `fathom review status` reports every episode those acts signed as `fresh`. A reviewer
who had forty sign-offs must still see forty, in the new binary, having done nothing.

**What `e10.3` shipped, said plainly because an owed check that is quietly unshipped is worse than one
nobody named.** The structural half holds by construction and is asserted: no table was renamed,
`SCHEMA_VERSION` did not move, and `crates/fathom-store/tests/triage_ledger.rs` still writes acts and reads
them back with their disposition, actor, reason, note, snooze condition and stamped risk model, and
`fathom review sync`/`verify`/`status` over a store carrying acts is `crates/fathom-cli/tests/review_verb.rs`
(`fathom review list --state dismissed` is the retrospective's deep form, and `r` reads the same records).
What `e10.3` could not ship was the frozen file itself, and it named it as owed for a reason rather than by
oversight: a store written by this build agrees with this build for the only reason that matters, so it
cannot be the check.

### The frozen store, which is what `reco-o884` is for

The check now has its artefact. `fixtures/queue-era/` holds three committed files, all of them produced by
`81eba3b` — epic 10's decision commit, the last tree that shipped the pane — and nothing in them was written
by a build that has seen the deletion:

| file | what it is |
|---|---|
| `store.sqlite3` | a store that build indexed: forty commits, forty episodes, forty acts (`x` `f` `-` `z`, all eight dismiss codes, every snooze condition including the two that carry a baseline), the `review_state` markers, and the `ledger_entry` mirror its own `sync` wrote |
| `ledger.md` | the class-M ledger those acts rendered, in the reviewer's own repository, committed by her |
| `manifest.json` | provenance — the pinned commit, the version line, each file's SHA-256 — plus the record that *that* build made of what it stored and what its own `review status`, `verify` and `sync` printed |

`tools/freeze_queue_era_store.py` is the generator: it puts a detached worktree of the pinned commit beside
the checkout, builds `--locked` from that commit's own lock, authors a fixed-clock forty-commit repository,
indexes it with the pinned binary, writes the acts through the pinned crate's `Store::record_triage` (the
door the act keys used — not a hand-written `INSERT`), renders the ledger with the pinned `sync`, and copies
the files out. The expectations the tests read are the manifest's, which is what keeps the fixture from
certifying itself: the numbers were recorded by a build that is not the one being tested.

What reads it back:

* `crates/fathom-store/tests/queue_era_store.rs` — the file opens, every act comes back with its disposition,
  actor, reason, note, stamped snooze condition, stamped score and the model name that scored it (`spine-1`,
  a name this build no longer writes), every act left its marker, and `triage_event_count` says forty.
* `crates/fathom-cli/tests/queue_era_review.rs` — `fathom review verify` checks forty entries and calls them
  intact; `fathom review status` prints, byte for byte, the sentence the queue-era build printed —
  `40 entries · 40 fresh`, `40 live · 20 with a fresh sign-off · 20 without`, and the same exit `7`; and
  `fathom review sync`, run over the act log with the ledger deleted, re-renders forty entries whose file is
  **identical to the queue-era ledger, byte for byte**, then reports forty fresh and never `0 with a fresh
  sign-off`.

Two honesty notes, because both are load-bearing. The store is byte-frozen in the sense that matters — these
bytes, from that build, pinned by digest and checked by `tests/test_queue_era_fixture_contract.py` and by
the store's own test — and it is **not** bit-reproducible on a second run, because `fathom index` stamps its
run and its cache rows with a system clock that has no override (`crates/fathom-cli/src/clock.rs`, ADR-002
§4.4). Re-running the freeze tool therefore re-blesses the artefact, which is the point of pinning it: a
fixture nobody can accidentally regenerate is a fixture nobody can accidentally *fix*. And the ledger in the
fixture is committed in a git repository because ADR-004 §10.1 is right — a sign-off git does not hold is
one `git clean -fdx` from never having happened — so the frozen verdict is a reviewer's real verdict and not
a fixture complaining about itself.

## How it is stored, and why markdown

```text
.fathom/reviews/
  ledger.md            # appended, fsynced before the mirror transaction commits
  notes.md             # the review memory: same class, same rules, a different noun
  <topic>.md           # more files are allowed; every file under here is the truth
```

Markdown, appended rather than rewritten, with one entry per episode and the digests on their own lines: it
diffs in a pull request, it survives every rewrites of the history it describes, it can be reviewed by a
person who has never installed Fathom, and `git log -p` is its audit trail. The store's `ledger_entry` mirror
is derived — dropping every row and re-inserting is a legal recovery action, losing the file is not. That is
the rule ADR-004 §9.6 states for every class-M file, and the shipped binary shows its end of it: `fathom review
status` objects in as many words when the ledger is git-ignored (the miniature's own output is quoted above),
because a must-survive file no clone can fetch is not must-survive.

## The ratchet still owes this a write

The ticket pairs the memory with the ratchet: granting an allowance should write *both* the `[[allowance]]`
in `.fathom/ratchet.toml` and a `decision` note saying why, because "we deliberately allow this cycle because
the port is the boundary" is the sentence a TOML row cannot hold and a later reader cannot recompute. The
ratchet (`e3.10`, **reco-5ht**) is built — `fathom ratchet` reads the file, the allowances and the digest —
but nothing writes a `[[allowance]]` from a granted cycle yet, so that paired write does not exist either;
when it is wired, it
belongs to its verb — which owns the owner, the expiry and the digest of the loosening — and it appends
through the same door `fathom note add` uses. The memory will not become the ratchet file in the meantime: an
allowance is a dated permission with a matcher, a note is a dated claim with an author.

---

## Related

* [The export artefact](export-artefact.md) — where the memory is printed for a pull request
* [Paths and files](paths.md) — what is committed, what is disposable
* [Storage](../adr/0004-storage.md) — the classes, and Appendix A's `decision_note`
* [episode](episode.md) — what a sign-off and a note are each keyed to
* [plan drift](plan-drift.md) — the case where signed-off work gets re-opened
* [the pipeline recipe](../recipes/gate-a-pipeline.md) — what the exit `7` is for
