# Ratchet: baselining what the codebase already owed

- **Status**: **partly shipped, and that is the honest word for it.** The two config keys and the storage
  contract exist today; the ratchet's write path is `e3.10` (**reco-5ht**) and is not built.
- **Decided by**: [the thesis](thesis.md) (the glossary entry "Ratchet / baseline", and *cycle debt*),
  [ADR-004](../adr/0004-storage.md) §5.1 (a declared analyzer input that resolves to `absent` is a trap),
  §10 (durability classes) and §12 (the committed paths).
- **In code today**: `ratchet.mode` and `ratchet.baseline` in
  [the key spec](config.md); `ratchet_entry` in ADR-004's DDL; the class-M prune guard in
  `src/recommender/store/`; `.fathom/ratchet.toml` in the committed-path list.
- **Read this if**: you want to know why a tool that finds architecture problems is quiet about most of
  them, and what "only tightens" is supposed to mean.

A **ratchet** is a committed file recording the violations that already existed, each with an owner and an
expiry, so that only *new* ones alarm. The noun in Fathom is `.fathom/ratchet.toml`, and the verb the design
promises is `fathom ratchet` (`e3.10`).

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
# .fathom/ratchet.toml — the shape e3.10 writes. Illustrative: nothing writes this file yet.
[[allowance]]
kind = "cycle"
key = "billing → invoicing → billing"
owner = "team-payments"
expires = "2026-03-31"
reason = "split lands with the ledger rewrite; see the plan item"
```

Four properties, each of which is a refusal of something easier:

| property | what it refuses |
|---|---|
| **committed** (class **M**) | "the baseline is a local cache". A baseline only a machine has can be deleted by a `git clean`, and the violations come back as if they were new. Class M is must-survive *because it is committed*, and the retention prune refuses to drop a class-M mirror whose committed file has gone missing (ADR-004 §9.6). |
| **owner on every row** | "somebody will notice". An allowance nobody owns is an allowance nobody removes. |
| **expiry on every row** | permanent debt. An expiring allowance turns into a finding again on a date that was chosen when it was cheap to choose one. |
| **it only tightens automatically** | drift. Narrowing or deleting an allowance is automatic; widening or adding one is a deliberate edit with a written reason, and the diff is the review — the file is markdown/TOML in a pull request, so loosening the ratchet cannot happen silently. |

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

And it is why the honest status line at the top of this page says *partly shipped*. The keys and the durability
contract are here; the writer, the CLI and the delta are `e3.10`'s, and a page that implied otherwise would be
the same kind of lie as a baseline that quietly widened itself.

---

**Next**: [dependency risk](dependency-risk.md) for the same "baseline, then alarm only on new" idea applied to
licences · [config](config.md) for how a key like `ratchet.mode` is layered and traced ·
[risk score](risk-score.md) for how a new violation becomes a number in a queue.
