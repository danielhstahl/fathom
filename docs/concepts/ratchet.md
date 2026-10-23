# Ratchet: baselining what the codebase already owed

- **Status**: **shipped.** The two config keys, the storage contract and the reader shipped first; the write
  path, the verb and the surfaces are **reco-5ht** (`e3.10`) and are here now. `fathom ratchet` reads the
  ledger, tightens it, and widens it only when told to with a reason.
- **Decided by**: [the thesis](thesis.md) (the glossary entry "Ratchet / baseline", and *cycle debt*),
  [ADR-004](../adr/0004-storage.md) §5.1 (a declared analyzer input that resolves to `absent` is a trap),
  §10 (durability classes) and §12 (the committed paths).
- **In code today**: `ratchet.mode` and `ratchet.baseline` in
  [the key spec](config.md); `ratchet_entry` in ADR-004's DDL; the class-M prune guard in
  `src/recommender/store/`; `.fathom/ratchet.toml` in the committed-path list; `fathom_analyze::ratchet`
  (the plan and the ledger read), `cycles::debt` and `layering::debt` (the two matching grammars), and the
  `fathom ratchet` verb.
- **Read this if**: you want to know why a tool that finds architecture problems is quiet about most of
  them, and what "only tightens" is supposed to mean.

A **ratchet** is a committed file recording the violations that already existed, each with an owner and an
expiry, so that only *new* ones alarm. The noun in Fathom is `.fathom/ratchet.toml`, and the verb is
`fathom ratchet`.

This page is about a decision that is easy to state and easy to lose: **a tool that reports every pre-existing
problem in a repository will be turned off by the second week.** The architecture questions an agent-written
codebase raises — cycles, layering violations, hotspots — are almost never new. The question a reviewer can
act on is not "is this repository messy" but "did *this turn* make it messier", and answering that requires a
baseline that is itself under review.

## The shape of the thing

```toml
# fathom.toml (today, and only these two keys)
[ratchet]
mode = "warn"                                  # what a new violation does. Ships as `warn`.
baseline = ".fathom/ratchet.toml"              # where the allowance ledger lives — committed, class M
```

```toml
# .fathom/ratchet.toml — written by `fathom ratchet --update-baseline --reason "..."`
[[allowance]]
kind = "cycle"
key = "billing → invoicing → billing"
owner = "team-payments"
expires = "2026-03-31"
reason = "split lands with the ledger rewrite; see the plan item"
granted = "2025-12-31"                     # written automatically, to the day
granted_by = "Ada Lovelace <ada@example.invalid>"   # ditto, from the git identity
```

A `layer` row spells its key the way the layering engine spells one — `rule::src->dst`, or
`rule::a|b|c` when one rule was crossed by a set of files — and a `cycle` row spells its key either as an
arrow path or as a member set. Both are normalised on the way in and on the way out, by the engine that owns
the grammar (`cycles::debt`, `layering::debt`), so a pasted key and a typed one match the same violation.

Four properties, each of which is a refusal of something easier:

| property | what it refuses |
|---|---|
| **committed** (class **M**) | "the baseline is a local cache". A baseline only a machine has can be deleted by a `git clean`, and the violations come back as if they were new. Class M is must-survive *because it is committed*, and the retention prune refuses to drop a class-M mirror whose committed file has gone missing (ADR-004 §9.6). |
| **owner on every row** | "somebody will notice". An allowance nobody owns is an allowance nobody removes. |
| **expiry on every row** | permanent debt. An expiring allowance turns into a finding again on a date that was chosen when it was cheap to choose one. |
| **it only tightens automatically** | drift. Narrowing or deleting an allowance is automatic; widening or adding one is a deliberate edit with a written reason, and the diff is the review — the file is markdown/TOML in a pull request, so loosening the ratchet cannot happen silently. |

## Running it

```bash
fathom ratchet                       # read-only: what is debt, what is news, what is about to lapse
fathom ratchet --json                # the same, as `fathom-ratchet/1`
fathom ratchet --update-baseline \
  --reason "the split lands with the ledger rewrite"   # the deliberate act: grant what is uncovered
fathom ratchet --update-baseline --reason "..." --key "cycle:a|b" --owner team-payments --expires 2026-09-30
```

The read is the interesting half, and it is a pure function of four inputs: the file, the violations at one
graph revision, whether each kind was judged at all, and the clock. From those, the plan (`Plan::build`) says
what the file should become:

* **A row whose violation has gone away is deleted**, next bare run, no flag. Tightening is automatic — that
  is the ratchet. The removal is printed with the reason the allowance was granted, and the row's own reason
  is not deleted with it: "who granted this and why" is exactly the history a reader wants when an exception
  vanishes.
* **Nothing is added without a deliberate act.** A bare run leaves uncovered violations *uncovered*; it does
  not write them into the file. `--update-baseline` is the act, and it requires a non-empty `--reason` and an
  owner — the constructor for a grant refuses an empty one, so the refusal is in the type before it is in the
  flag parser. `--reason` without `--update-baseline` is a usage error in both directions, so neither flag can
  be typed as a slip of the other.
* **A grant carries an expiry**, defaulting to 90 days (`DEFAULT_GRACE_DAYS`), and the date and grantee are
  filled in from the clock and the git identity so a written row is always complete. A row written to expire
  *never* is allowed, and prints as `no expiry` rather than being hidden.
* **Expiry is day-grain**, matching `cycles::debt`: "expired on the 31st" must not depend on the hour the
  index ran.
* **An expired allowance is debt back due.** Nothing is deleted when a date passes: the violation it forgave
  is uncovered again and is said with its expiry date, and the row stays in the file until it is paid off or
  deliberately re-granted — a re-grant replaces the lapsed row rather than stacking a second one under it.

Three surfaces have to say it, because a date nobody sees before it passes is worse than no date at all:

| surface | what it says |
|---|---|
| `fathom ratchet` | the summary line, then the expiring rows with their days left, the back-due rows with their dates, the uncovered news, and what the file is about to be. `--dry-run` prints all of it and writes none of it. |
| the architecture map (`g`) | the ledger's dates in the detail pane and at the front of the strip — the map already draws which vertices are debt, so the date each stops being forgiven belongs beside it. `fathom_tui::graph::hand::Debt`, filled by `fathom-cli` from `ratchet::Ledger`; the widget has no ratchet of its own. |
| the review artefact (`fathom export`) | the ledger copied into the record as `debts`, in all three renderings, with the rows whose own expiry had gone by by the range's newest commit raised as checklist items. The record keeps its no-clock rule: "lapsed" is measured against the newest commit it carries, not against the day it was printed, so a blessed artefact does not change its mind at midnight. |

And the two refusals worth knowing: **no index or no graph revision exits 12** — with no graph there is no list
of violations to read the ledger against, and a run that reported "nothing uncovered" would be reporting a fact
it never checked — and **a missing file is not an error**: it is `baselined: false`, every violation is news,
and the run says so. Absence is a state, per ADR-004 §5.1.

## Where it plugs in

`graph.cycles` declares `ratchet.` and `layers.` as inputs (ADR-004 §5.1), which is why the keys exist before
the feature does: a declared analyzer input that resolves to `absent` is the exact failure ADR-004 §5.5 names,
so the subtree had to ship with defensible defaults
(the note on `ratchet.` in [the config reference](config.md) is the record of that reasoning). The defaults
that shipped are the two nobody had to guess at — where the ledger lives, and `warn` rather than `fail`, because
a ratchet that blocks a first-day review teaches nothing about the review.

The related nouns, which are *not* this one:
* **cycle debt** ([`e3.5`, shipped](cycle-detection.md)) — import cycles that exist, baselined with owner
  and expiry, so only new cycles alarm; the count of nodes inside cycles is a trendable health metric. The
  four-way verdict (`debt` / `violations` / `back` / `stale`) and the day-grain expiry rule live in
  [cycles and cycle debt](cycle-detection.md); `cycles::debt` is the code that reads this file's
  `kind = "cycle"` rows.
* **layering rule** (`e3.6`) — the intended architecture in `fathom.toml`: named layers, allowed and forbidden
  edges. Evaluated as a per-episode **delta**, so the reviewer sees what this turn did rather than what the
  codebase already owed. The ratchet is what makes that subtraction possible.
* **`ratchet_entry`** — the store's mirror of the file, and one of the things the retention pruner refuses to
  delete (ADR-004 §8.2): a reviewer's evidence must not be garbage-collected because a background job found it
  old.

## The other ratchet, which ships today

The word is used for one other thing in this repository, and it is the same idea with a different subject: a
**perf budget** that only tightens.

```bash
make bench          # measure every budget in benchmarks/budgets.json (~30 s)
make bench-check    # as CI runs it: the budget, and a ±15 % regression gate
make bench-bless    # rewrite the baseline from this run — the ratchet, as a deliberate commit
make bench-selftest # prove the gate can go red, with no build at all
```

`tools/perf_gate.py` checks every measurement twice: against its budget, and against the blessed baseline in
[benchmarks/baselines/perf-gate.json](../../benchmarks/baselines/perf-gate.json) — and it fails past 15 % in
**either** direction, so an un-blessed *improvement* is also red. That last part is what distinguishes a
ratchet from a warning: an improvement that nobody acknowledged is a number that will silently drift back up
later, so the act of blessing is forced while the change is in front of somebody. [The budget page](../../benchmarks/BUDGETS.md)
lists all 172 thresholds and what asserts each one.

When you see the sentence "budgets ratchet, they do not drift" in [graph-storage](graph-storage.md) or a
report, this is the mechanism being invoked: a measured number becomes the budget with a stated headroom, the
next improvement tightens it in the same change that earned it, and no number in a document is allowed to be
louder than the number in the gate.

## Why this belongs in a review tool at all

Because the alternative is the one that has already failed. Architecture reports that list everything are read
once, by the person who asked, and the list is stale by the next merge. A ratchet converts an unbounded backlog
into a per-episode delta with an owner and a date on it, which is the only form of technical debt that has ever
been paid down: the small, dated, attributable kind.

And it is why the writer was built last and the reader first. A page that says *shipped* while the file it
describes is written by hand would be the same kind of lie as a baseline that quietly widened itself: the
worth of a ratchet is entirely in whether its contents can be trusted, so every part of this design is built
around making one particular act — loosening the exception — expensive, visible and attributable, and every
other part automatic.

One interlock deserves naming, because it is the reason the ledger is not a standalone feature: granting an
allowance appends a **decision note** to the review memory (`.fathom/reviews/notes.md`, `e6.5`) carrying the
same reason and the rows it wrote, anchored to the ledger file. The file says what is forgiven; the memory says
who said so, when, and against what — and the re-link ladder will still be able to answer that question after
the branch it was written on has been rebased into an unreachable blob.

---

**Next**: [dependency risk](dependency-risk.md) for the same "baseline, then alarm only on new" idea applied to
licences · [config](config.md) for how a key like `ratchet.mode` is layered and traced ·
[risk score](risk-score.md) for how a new violation becomes a number in a queue.
