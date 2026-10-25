# The review memory: annotations, decisions and questions, committed next to the ledger

- **Status**: shipped (`e6.5`, **reco-552**).
- **In code**: `crates/fathom-analyze/src/notes/` (`entry`, `format`, `collect`),
  `crates/fathom-cli/src/notes.rs`, the store mirror `crates/fathom-store/src/notes.rs`, and the agent door
  `review_memory` / `add_review_note` in `crates/fathom-mcp/`.
- **Decided by**: [ADR-004 §10](../adr/0004-storage.md) (durability classes — class **M** is must-survive
  *because it is committed*), [ADR-003](../adr/0003-episodes.md) (episode identity, which is what a note is
  keyed to), and ADR-004 Appendix A (the `decision_note` table this mirrors).
- **Read this if**: you want to know why a change is the way it is, or you have a question a review left
  open and want it to stop being open.

A review produces more than a verdict. It produces *"we deliberately allow this cycle because the port is
the boundary"*, *"does anything still call the old entry point?"*, *"follow up on this once the migration
lands"*. That knowledge is the part that normally dies in a pull-request thread. The **review memory** is
where it goes instead: markdown under `.fathom/reviews/notes.md`, committed, reviewable in the same pull
request as the change it describes, and mirrored into the index so a surface can query it without parsing.

It is a different file from the [ledger](review-ledger.md) on purpose. The ledger is a gate: four
dispositions, one per reviewer per change, and the north-star metric counts it. The memory is a record:
plentiful, free-text, opened by a question and closed by an answer. A note never counts as a review, and a
sign-off never carries a paragraph of reasoning — each says what it says.

| | the ledger | the review memory |
|---|---|---|
| file | `.fathom/reviews/ledger.md` | `.fathom/reviews/notes.md` |
| unit | a disposition by a named human | a decision, question, follow-up or annotation |
| answered by | a later disposition | a later note with `about:` naming it |
| counts as a review | **yes** (`accepted` only) | no |
| volume | one block per (episode, reviewer) | as many as the review produced |

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
[sign-off](review-ledger.md)) or to a **path**. A path-anchored note is a real anchor, not a failure: the
ladder reports it as `anchored` and `fathom context <path>` finds it.

## Appending, never editing

The file is append-only, and every block ends with a `digest:` line hashing its own lines. So:

* **answering** a question appends a new note with `about: nt-…`. The question's block is not touched, and
  its status becomes `answered` because a later block names it.
* **withdrawing** a note appends a new one with `withdraws: nt-…`. Nothing is deleted; a retracted
  decision is still a fact about the review.
* **editing** a block that has been committed is detected: `fathom note verify` re-hashes every block and
  names the ones whose text no longer matches. It needs no index and no store, so a fresh clone can audit
  the memory before it builds anything.

Unknown fields are *reported*, never dropped: a note written by a newer build and read by an older one
tells the older one what it could not understand, in the same way the ledger does.

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
refusals that leave the file byte-identical and the search corpus are actually checked.

## Surviving a rebase

A note keyed to a commit id is worth nothing after a rebase, and a note that silently disappears is worse
than one that never existed: the reader cannot tell a deleted decision from a rewritten history. So a note
carries the same four digests a sign-off does and goes through the [same re-link ladder](review-ledger.md),
rung by rung: exact id → fingerprint → shape → member-set. What the ladder could not place is reported as
`ORPHAN` with the reason, and stays in the file and in every report.

That is the acceptance criterion, measured in `crates/fathom-analyze/tests/notes.rs`: notes written against
a twelve-turn branch, the tail rebased under new subjects and new commit ids, and every note still attached
— the ones whose episode identity is gone re-linked on a weaker rung, with `relinked_from` and the rung's
name printed so a reader can see how sure it is.

## The store is a mirror

`decision_note` (migration 10) holds the file's content plus the verdicts: the link's method and score, the
state, the derived status, and the tamper flag. It is class **R** by copy — delete it and `fathom note list`
rebuilds it from the markdown, which is why the write path is file-first: append, fsync, then mirror. A crash
between the two loses a row, never a note.

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

## Related

* [The review ledger](review-ledger.md) — the sign-offs this sits beside, and the re-link ladder it shares
* [The export artefact](export-artefact.md) — where the memory is printed for a pull request
* [Paths and files](paths.md) — what is committed, what is disposable
* [Storage](../adr/0004-storage.md) — the classes, and Appendix A's `decision_note`
