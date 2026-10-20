# Recipe: export a review for a pull request

- **You get**: the review as a document somebody else can read — markdown in the PR body, HTML attached to an
  audit, JSON for a program — all three rendered from one record, so they cannot disagree.
- **Time**: five minutes.
- **Needs**: an index, and ideally the sign-offs you want the artefact to carry.
- **Plan key**: `e5.7` (**reco-1tn**).

One **record**, composed from the delta documents, the [ledger](../concepts/review-ledger.md) and the detector
report — built with no store, no clock and no config in the way, which is what buys the acceptance criterion:
**the same range gives the same bytes twice**. Re-run an export after the PR merges and the artefact is
byte-identical, so the copy you pasted into the description is provably the copy you reviewed.

## The three renderings

```bash
fathom export --format md   --out docs/review/pr-418      # markdown, for the PR body
fathom export --format html --out /tmp/pr-418-audit       # one self-contained file, for an audit
fathom export --format json --out /tmp/pr-418             # for a program that will do something with it
```

`--out` is a stem unless the name already carries the format's extension. For a paste into a PR description:

```bash
fathom export --format md --stdout --episodes 'origin/main..HEAD'
```

```text
# Fathom review record — `HEAD`

<!-- fathom-review-record/1 · written by fathom export --format md --episodes HEAD · regenerate instead of editing -->

- **Scope** — `HEAD` · resolved `70b94d6` · 1 commit(s) · 1 episode(s)
- **Verdict** — 0 of 1 episode(s) carry an accepted sign-off · 1 undecided · 1 action(s) open
- **Built from** — delta `fathom-delta/1/v1` · ledger `fathom-review/1` · narrative `fathom-narrative/1/v1`
- **Produced by** — `fathom 0.1.0-alpha.1` from `.fathom/index.sqlite3`
```

That header is the whole design in four lines: what was in scope, whether a human said so, which contract
versions composed it, and which build wrote it. The comment line is an instruction, not a comment: **regenerate
instead of editing**. An edited artefact is a claim nobody can re-derive, and the moment one artefact in a
project is hand-edited, every later one is suspect.

## What is in it

Summary card · per-episode [narratives](../concepts/episode-narrative.md) with their citations · dependency and
architecture deltas · [findings](../detectors/index.md) with evidence · the **operator-action checklist** with
the command that closes each item · the **sign-off block** with reviewer, instant, scope, digests and the
reason behind every dismissal · and the gap list, which says what was not measured and who owns it.

Two details that matter to the person on the receiving end:

* **hostile input is escaped.** A commit subject carrying a pipe, a backtick or a `javascript:` link renders
  as text, not as a table cell, a link or a surprise. Harness output is untrusted input, and an artefact that
  a reviewer pastes into a PR body is where that becomes a security question
  ([the export contract](../concepts/export-artefact.md));
* **the HTML runs and fetches nothing.** One self-contained printable file with a CSP that forbids both, because
  "attach the audit" should not mean "load three CDNs into an auditor's browser".

## Sign off first, then export

The artefact reports the sign-offs it finds; it does not create them. A record whose verdict line is
`0 of 12 carry an accepted sign-off` is an honest statement that nobody has reviewed the range, which is a
legitimate thing to say out loud and a poor thing to merge on:

```bash
fathom review status                        # what a reviewer has to see before the artefact means anything
fathom review sync                          # turn the acts the queue recorded into ledger entries
fathom export --format md --stdout --strict > pr-418.md    # exit 7 while a checklist item is open
```

`--strict` is the CI form ([gate a pipeline](gate-a-pipeline.md)): it exits `7` while the checklist still has
an open item, which is a fact about the change — a dropped column, a new environment variable, a CI permission
escalation — rather than an opinion about code quality.

## Reading one you were sent

Everything in the artefact is also answerable from a terminal, and the artefact tells you which contract
version it was written from, so a reader without Fathom gets a document and a reader with it gets a
verification:

```bash
fathom analyze <episode> --json          # the delta the narrative was composed from
fathom why <episode> --json              # the score the ranking used
fathom review list --json                # the sign-offs the sign-off block came from
shasum -a 256 pr-418.html                # byte-stable, so this is a meaningful thing to compare
```

If a figure in the artefact cannot be found in the document it cites, that is a bug and worth reporting — the
self-check (`Record::verify`) is meant to make that impossible before the file is written.

**Next**: [review a session](review-a-session.md) for the sign-offs it reports ·
[the review artefact](../concepts/export-artefact.md) for the contract in full.
