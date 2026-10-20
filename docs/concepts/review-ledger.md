# The review ledger: sign-offs that outlive the history they were written about

- **Status**: shipped (`e4.6`, **reco-rlf**).
- **In code**: `crates/fathom-analyze/src/ledger/` (`entry`, `format`, `relink`, `collect`) and
  `crates/fathom-cli/src/review.rs`; the store mirror is `crates/fathom-store/src/ledger.rs`.
- **Decided by**: ADR-004 §10 (durability classes — class **M** is must-survive *because it is committed*)
  and [ADR-003](../adr/0003-episodes.md) (identity, which is what a sign-off is keyed to).
- **Read this if**: you have signed something off and want to know what that actually did, or you want a CI
  gate that means "a human looked".

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

## What a sign-off says

Four decisions, and they are the whole vocabulary — the same four dispositions the queue offers
([glossary](thesis.md)):

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
fathom review sync              # render the acts the queue wrote as ledger entries
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

(The long digests are elided in the second block so the lines fit; the real output prints them in full.)

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
  back to the queue rather than being quietly treated as reviewed.
* **`unknown`** — the content digest was never computed, so staleness is undecidable. Reported as stale with
  the reason "content was never measured", because a digest that cannot be computed is not a pass.

Two denominator rules are worth knowing because they are the difference between a gate that closes and one
that whines forever: an episode whose paths are all under `.fathom/` is **excluded** (the commit that records
the sign-offs is itself an episode, which would want a sign-off, forever), and an episode with no paths at all
**stays in** — "there was nothing to look at" is a claim worth a second look.

## The dismiss vocabulary is closed

Dismissing requires a reason from a fixed set, not free text. That is not pedantry: a closed set is what makes
systematic dismissing a number on a screen instead of a feeling, and `queue.retrospect` (`r` in the queue) is
the rollup that reads it. The reasons are also what a *new* reviewer inherits — "why did we stop caring about
this" is a question whose answer has to be in the record, and free text five months later is a shrug in
prose.

## Tampering, and why `verify` needs no index

Every entry hashes itself, so editing an entry's text after the fact is detectable
(`fathom review verify`, exit `7` on anything wrong). `verify` reads only the ledger files — no episodes, no
store — which is the case the check exists for: a fresh clone that has never been indexed can still learn that
a sign-off was edited.

What an entry actually is, from the miniature after one sign-off (the whole file, minus the header comment):

```text
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

Four digests, one member line, one path line per touched path, and the entry's own hash last. Editing the
note by hand is caught, and `fathom review status` treats the whole sign-off as unverified when it is:

```text
$ sed -i 's/read the diff/read the whole diff/' .fathom/reviews/ledger.md
$ fathom review verify; echo "exit=$?"
checked   1 ledger entry
  · .fathom/reviews/ledger.md:32: entry `sg-a11885f50b78cc97`: its `digest:` is sha256:b5d1… and its lines
    hash to sha256:da51… — the text changed after it was written
verdict   the ledger does not match itself — treat these entries as unverified
exit=7
```

(The digests are truncated here so the line breaks stay readable; the real line prints them in full.)

## How it is stored, and why markdown

```text
.fathom/reviews/
  ledger.md            # appended, fsynced before the mirror transaction commits
  <topic>.md           # more files are allowed; every file under here is the truth
```

Markdown, appended rather than rewritten, with one entry per episode and the digests on their own lines: it
diffs in a pull request, it survives every rewrites of the history it describes, it can be reviewed by a
person who has never installed Fathom, and `git log -p` is its audit trail. The store's `ledger_entry` mirror
is derived — dropping every row and re-inserting is a legal recovery action, losing the file is not. That is
the rule ADR-004 §9.6 states for every class-M file, and the shipped binary shows its end of it: `fathom review
status` objects in as many words when the ledger is git-ignored (the miniature's own output is quoted above),
because a must-survive file no clone can fetch is not must-survive.

---

**Next**: [episode](episode.md) for what a sign-off is keyed to ·
[plan drift](plan-drift.md) for the case where signed-off work gets re-opened ·
[the pipeline recipe](../recipes/gate-a-pipeline.md) for the `7`.
