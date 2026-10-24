# signal — the repository Fathom demos itself on

This is a real git repository with 27 commits, two human authors and one agent, and every one of them
exists to put a row in a different part of [Fathom](https://github.com/danielstahl/fathom). Twenty-one
of them are the miniature Fathom unpacks when you run `fathom --demo` — the bytes are literally the same,
they come out of the pack `crates/fathom-cli/assets/fathom-demo.pack`. Five more were added for this
tour: four episodes that argue with one function and then undo themselves, and one commit that is
provably whitespace. Together the twenty-six seed five pathologies; the twenty-seventh commit is this
page, the answer key and the CI job.
**This page is the tour, not the answer key.** It tells you where to look and what to type; it does not
tell you which commit planted what. When you have worked through it — or when you want the markings —
[`PATHOLOGIES.md`](PATHOLOGIES.md) is the answer key, and every claim in both files is generated from the
same catalogue that a machine runs against a real binary in
[Fathom's CI](https://github.com/danielstahl/fathom/blob/main/docs/demo/five-minute.md).

```bash
cargo install fathom-cli        # or: cargo build -p fathom-cli, or the installer in the README
fathom index                    # the one prerequisite: every verb reads the index, nothing builds it for you
```

## Five minutes, five acts, one command

Act 0 is the index. Every act after it is one command each, and Act 3's is the one that answers:

```bash
fathom index --no-progress --json          # 27 commits in milliseconds; the JSON splits it by phase
```

### Act 1 — orient: what is this repository

```bash
fathom episodes --plain                    # the review rail: every episode, its churn, its review state
fathom why 4513a9880166 --plain            # why this episode is in front of a human, as an addition
```

Twenty-seven episodes, every one of them `unread`, ranked. `fathom why` is the one that makes the ranking
checkable: every axis, its weight, what it measured, and — in plain words — what it could not see. Read
the *cannot see* list before you trust a low score; it is the same list this demo is honest about further
down.

**You have oriented when** you can name three episodes you would read first and say why. Nobody has
reviewed this history, which is the state a demo should start in.

### Act 2 — the gate, going red

```bash
fathom check --base 4513a9880166 --head HEAD --fail-on warning
echo $?                                    # 7
```

`fathom check` **fails** on this history, and that is the exhibit. Four new direct dependencies arrive
between the first commit and `HEAD` — `serde`, `toml`, `express`, `zod` — and `toml` is declared in
`Cargo.toml` with no lockfile entry resolving it. A gate that approved all four would be decoration. Exit
**7** is the code that means *a human has work to do*; the CI job in `.github/workflows/fathom.yml` runs
the same six checks on every pull request here, in `warn` mode, because on this repository the findings
are the point rather than a defect.

### Act 3 — the one command

```bash
fathom export --episodes 4513a9880166..HEAD --format markdown --out review.md
```

One command, one artefact: `review.md`. Scope and verdict, every episode with its risk score, the
dependency delta, the architecture delta, the findings with their evidence and suppression keys, the
operator-action checklist, and the sign-off state. This is the artefact you attach to a pull request or
hand to an auditor, and on this repository it is also the answer sheet: **every finding this page
documents appears in `review.md`.** (`fathom index` is the line before it because nothing in Fathom
indexes on its own behalf except `fathom --demo`; the flag that would fold them into one is
`--reindex`, and it is not built.)

### Act 4 — say a human read it

```bash
fathom review sign ep-4a42de295c13b5229318 --note "read on the demo; the store is fine"
fathom review status                       # still 7: one sign-off out of twenty-seven is not a review
```

The sign-off is markdown committed under `.fathom/reviews/` — which is tracked here on purpose, and the
rest of `.fathom/` is not. The store is disposable, the ledger is not. `fathom review status` stays red
afterwards, and that is the correct answer: one episode signed is not twenty-seven reviewed.

### Act 5 — the five pathologies, in the order they were planted

Five things were buried in this history on purpose. Each line says where to look, not what is there:

1. **A deleted test.** `fathom episodes --plain` and the `tests` column; then `fathom why` on the episode
   whose subject says the case was folded into another test. Look for the finding whose id ends in
   `_weakening`, and for the evidence line that quotes the deleted `#[test]`.
2. **An unplanned dependency.** Act 2 already found it: the `dependencies` row, and `review.md`'s
   *Dependency delta*. One of those four dependencies is declared and never resolved.
3. **A new cycle.** Two modules that import each other, added in one episode with a commit body that
   admits it. `fathom check --only cycles --base 4513a9880166 --head HEAD` will not name it — read
   `src/core/mod.rs` and `src/api.rs`, then find the sentence in `review.md` that says the ranking cannot
   see an import cycle. That sentence is the finding, today.
4. **A churn loop.** Four episodes on one function; the last one puts back what the second one deleted.
   `fathom episodes --plain` shows four episodes whose subjects start the same way; `git show` them in
   order. The detector that reads this shape ships switched off, and this repository's `fathom.toml`
   turns it on anyway — nothing fires yet, which is the honest state of that exhibit.
5. **A reformat-only commit.** One commit, one file, every changed line whitespace. Prove it yourself:
   `git log --oneline --stat` to find it, then `git diff -w <it>^ <it>` — an empty output is the whole
   definition. Fathom's noise axis discounts lockfiles and generated files and does not yet name a
   reformat, so the axis that should discount this one counts it as reviewable.

**You have finished when** all five are found, `review.md` is on disk, and `fathom review status` is still
red. If a step printed nothing where this page promised a finding, that is a bug in Fathom or in this
page — open one against the repository, either is a real defect.

## Why this history looks like this

| Commit shape | What it exercises |
|---|---|
| `add the crate`, `parse:`, `query:` | the plain spine: commits to episodes, churn, the rail |
| `move core.rs into its own module` | a rename — episode identity across a path change |
| `tests: fold the empty-store case…` | the test-surface delta, and `test_weakening` at `high` |
| `bump serde…`, `bump express…`, `add toml…` | dependency deltas in two ecosystems, and the gate's `dependencies` check |
| `rustfmt the parse module` | a small reformat, which is *not* discounted yet — see the gap below |
| `ci: deploy the release build…` | non-code surfaces: a change an operator has to act on |
| `generated with claude: …` | attribution: an agent-authored commit, ranked as one |
| the merge at the tip | episode identity across a history rewrite |
| `retry: …` ×4 | a churn loop: the same region, four episodes, and a put-back |
| `style: reindent the queue module…` | a commit that is whitespace and nothing else |

## What this demo cannot show you

Three of the five seeded pathologies are not named by a finding yet, and this page says so rather than
letting you conclude the tool is broken. Each one has an owner and a remedy in
[`PATHOLOGIES.md`](PATHOLOGIES.md):

* **the cycle** — the index writes an anchor revision with nodes and no edges, so `fathom check` reports
  `0 cycle(s) in the revision` for a repository with a cycle in it. No history can make the cycle check
  fire until that is fixed, and a demo that printed a cycle finding would be printing a lie.
* **the churn loop** — `churn_loop` ships switched off until its false-positive rate is measured on real
  repositories, which is a decision with a ticket, not an oversight. It is enabled here so the finding
  arrives with no further change the day it lands.
* **the reformat** — `NoiseClass::Reformat` exists in the ingest record and is never assigned, so a
  whitespace-only commit is scored like a real change.

The same honesty is why the tour exists at all: `python3 tools/five_minute_demo.py check` in the Fathom
repository runs every command on this page against a built binary and fails when one stops being true,
and `python3 tools/demo_acceptance.py check` runs the clone-plus-one-command half. A page nobody re-runs
is a page that goes quietly wrong.

## The gate that runs here

[`.github/workflows/fathom.yml`](.github/workflows/fathom.yml) calls Fathom's reusable
`fathom-check` workflow on every pull request — the same five lines any repository can copy. It reports
rather than blocks (`mode: warn`), because on this repository the findings are the exhibit.
