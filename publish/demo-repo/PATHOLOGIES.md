# The five seeded pathologies — the answer key

This repository is a fixture: every commit exists to put a row somewhere in [Fathom](https://github.com/danielstahl/fathom). [`DEMO.md`](DEMO.md) is the tour and deliberately *does not* name the answers; this page does, so the tour can be given to a newcomer cold and still be marked afterwards.

It is generated — `python3 tools/demo_exhibits.py write` in the Fathom repository writes this file, and `tools/demo_acceptance.py check` runs every row against a real binary and fails if what the row promises is not what the product says. Three of the five are honest gaps rather than findings, and each one names the ticket that would close it.

| # | Pathology | Seeded by | Named today | Where to look |
|---|---|---|---|---|
| 1 | `deleted test` | `tests: fold the empty-store case into the append test` | **yes** | the rail's `tests` column, then `fathom why` on the episode and the exported record's *Findings with evidence* |
| 2 | `unplanned dependency` | `add toml to the dependency surface` · `bump serde to 1.0.190 for the flatten fix` | **yes** | `fathom check`'s `dependencies` row, then the exported record's *Dependency delta* |
| 3 | `new cycle` | `summary: fold a window into one figure` | **not yet** | read the two files (`src/core/mod.rs`, `src/api.rs`) — and then read the sentence in the exported record that says the ranking cannot see an import cycle |
| 4 | `churn loop` | `retry: bound the flush retry at three attempts` · `retry: let the flush run to a fourth attempt` · `retry: step the attempt counter twice per turn` · `retry: put the retry bound back` | **not yet** | `fathom episodes --plain` lists four `retry:` episodes in a row; the fourth's diff adds back the line the second deleted (`git show` each of the four, in order) |
| 5 | `reformat-only commit` | `style: reindent the queue module to the house width` | **not yet** | `fathom why` on the commit: the size axis says the churn is reviewable rather than discounted, and the noise axis says it found nothing generated, locked, vendored or binary |

## Each one, in its own words

### 1. deleted test

`tests: fold the empty-store case into the append test` removes a `#[test]` and its assertion, and the episode's own test totals go to −1 case, −1 assertion line.

- **Seeded in:** the miniature (ships in the binary)
- **Where to look:** the rail's `tests` column, then `fathom why` on the episode and the exported record's *Findings with evidence*
- **Status:** a finding today
- **Why it is named:** `test_weakening` is a default-on detector, so the finding arrives with a reason, a jump target, a confidence below 1.0 and the suppression key `test_weakening:<path>`.

### 2. unplanned dependency

`add toml to the dependency surface` declares a dependency no lockfile resolves, on top of two ecosystems' worth of new direct dependencies earlier in the history.

- **Seeded in:** the miniature (ships in the binary)
- **Where to look:** `fathom check`'s `dependencies` row, then the exported record's *Dependency delta*
- **Status:** a finding today
- **Why it is named:** the `dependencies` check raises `dependencies/new-direct` per dependency and `fathom check --fail-on warning` exits **7** over this range: the gate blocks, which is the exhibit, not a defect in it.

### 3. new cycle

`summary: fold a window into one figure` gives `Store::summary` a call to `crate::api::summary` while `api` already imports `core`: a two-module cycle, put there on purpose and said so in the commit body.

- **Seeded in:** the miniature (ships in the binary)
- **Where to look:** read the two files (`src/core/mod.rs`, `src/api.rs`) — and then read the sentence in the exported record that says the ranking cannot see an import cycle
- **Status:** an honest gap
- **Who owns the gap:** `e3.5` (`cycles`) and `e3.11` (the graph row)
- **What would close it:** the index writes the anchor revision's edges. Today a full snapshot carries nodes and no edges (`graph_snapshot.edge_count` is 0 for the first revision of every repository), so every revision above the anchor inherits a graph with fewer edges than the tree has, and `fathom check --only cycles` prints `0 cycle(s) in the revision` for a repository with a cycle in it. Until that is fixed no history — this one or yours — can make the cycle check fire, and a demo that printed a cycle finding would be printing a lie.
- **What this repository proves anyway:** the seed is real, checked against the history rather than asserted here — so when the detector lands, the finding appears with nothing else to change.

### 4. churn loop

four episodes on `src/retry.rs`, all inside `flush_with_retry`: the bound is there, removed, moved, then put back with the comment that said why the first time.

- **Seeded in:** the demo repository's own commits
- **Where to look:** `fathom episodes --plain` lists four `retry:` episodes in a row; the fourth's diff adds back the line the second deleted (`git show` each of the four, in order)
- **Status:** an honest gap
- **Who owns the gap:** `e4.3a` (the disposition set that prices `churn_loop`)
- **What would close it:** `churn_loop` ships switched off — its own page says four detectors are off because their false-positive rate has not been measured on real repositories. This repository turns it on in `fathom.toml` anyway, so the day the calibration lands the finding appears with no further change here. It does not fire today even switched on, which is the second half of why this row is a gap and not a `named` row.
- **What this repository proves anyway:** the seed is real, checked against the history rather than asserted here — so when the detector lands, the finding appears with nothing else to change.

### 5. reformat-only commit

`style: reindent the queue module to the house width`: eight lines changed, every one of them whitespace. `git diff -w` against its parent is empty — that is the definition, and the acceptance run proves it instead of claiming it.

- **Seeded in:** the demo repository's own commits
- **Where to look:** `fathom why` on the commit: the size axis says the churn is reviewable rather than discounted, and the noise axis says it found nothing generated, locked, vendored or binary
- **Status:** an honest gap
- **Who owns the gap:** `e2.2` (separating a reformat from a rewrite)
- **What would close it:** `NoiseClass::Reformat` exists in the ingest record and is never assigned: the only classifier that runs today marks every file `Unknown`, so a whitespace-only commit is indistinguishable from a real change in the noise axis. The types are in place; the comparison (a change whose hunks are equal under whitespace folding) is not.
- **What this repository proves anyway:** the seed is real, checked against the history rather than asserted here — so when the detector lands, the finding appears with nothing else to change.

## Why three of the five are gaps

Because the alternative is a demo that prints things the product does not print. `reco-3qr`'s acceptance criterion is that a clone plus one command reproduces *every documented finding*, and the only version of that sentence that can be true is one where the document is generated from what the binary actually says and re-checked on every commit. Where Fathom cannot yet see a pathology, this page says so, names the owner and names what would change the answer — and the checker fails the day the product gets better, because then the page is wrong in the other direction.
