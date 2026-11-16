# Complexity at index time: the three ceilings, and what an honest refusal sounds like

- **Status**: Accepted, and implemented
- **Ticket**: `reco-l9bz.4` (index-time budget and degradation contract for reading file contents at scale)
- **Implements**: [ADR-007 §9.7](../adr/0007-complexity.md) (where the measurement runs, and the ceilings it
  runs under), [ADR-007 §7.1](../adr/0007-complexity.md) (the reason-code list this reuses rather than
  extends), [ADR-007 §9.6](../adr/0007-complexity.md) (coverage, and never printing a figure for a file that
  was never parsed), [ADR-006 §10.3](../adr/0006-tui.md) (the progress row's contract), and
  [ADR-002 §4.5](../adr/0002-workspace.md) (configuration is read by `fathom-cli` and handed downhill as values)
- **Executes as**: `crates/fathom-analyze/src/fill.rs` (the pass, the cost order, the ledger),
  `crates/fathom-analyze/src/structure/policy.rs` (the three keys and their defaults),
  `crates/fathom-store/src/complexity.rs` (the owed set, the write, the coverage roll-up),
  `crates/fathom-analyze/src/index.rs` (where the pass sits in a run, and the tick's note),
  `crates/fathom-cli/src/{verbs,state,guide,watch}.rs` (resolution, the run line, the progress row, the
  doctor lines), and `crates/fathom-analyze/tests/complexity_fill.rs`

---

## 1. The question, and the answer

ADR-007 §12.11 left one open question attached to a metric that had already been decided: **where does
complexity get measured, when measuring it means reading file contents?** This page is the answer as an
operator sees it; [ADR-007 §9.7](../adr/0007-complexity.md) is the argument.

> The figure is computed during the index run, in the pass that already has the bytes in hand, and stored in
> `git_commit_file`'s complexity columns. It is not a derived row produced later by an analyzer that re-reads
> the contents through the cache.

The reason is that the choice was never "read the contents or don't". The run that writes a file row has
already asked the object database for those bytes to compute churn. The real choice was: count them while
they are in hand, or inflate them a second time from a cache whose ceilings have already decided that large
payloads are not worth keeping.

A deferred reader sits downstream of three budgets that are not its own, and each one turns a *cost* into a
*reason* — which is worse, because a reason is what a reviewer reads:

| Ceiling a deferred reader would inherit | What it would answer, and why that is the wrong sentence |
|---|---|
| `e7.3`'s eviction, which drops `structure.extract` payloads **first** | An inflate, not a lookup: the payload it wanted was the first thing the run threw away |
| `cache.select.structure_extract_max_payload_bytes`, which refuses big structure rows **durably** | `contents_not_kept` for exactly the large files a reviewer is most suspicious of |
| `ingest.size_cap_mib`, which declines to keep a huge blob at all | A second, unrelated reason for the same `?`, from a layer that was never asked about complexity |

And the counter-price is measured, not assumed: ADR-007 §11.2 timed the worst grammar at 24 MB/s, four orders
of magnitude under the blob-copy path. Paying that twice per blob is the whole budget, spent on the pass
rather than on the ingest.

What the index-time choice costs is real and is stated plainly: **the pass runs over every commit, including
the ones nobody will ever review.** That is what the three ceilings below are for. They are not a safety
margin added after the fact; they are the reason the decision is affordable, and so they are documented in the
same vocabulary as the [memory ceilings](memory.md) that made the same trade for resident memory.

## 2. The three ceilings

All three are `fathom.toml` keys, read by `fathom-cli` — repository file first, then `FATHOM_*` over it, in
the order every other key is layered ([config and its five layers](config.md)) — and handed downhill as
values, because a crate below the CLI may not open the config file.

| Key | Default | What it bounds | Decided | When it bites |
|---|---|---|---|---|
| `extract.complexity.max_file_bytes` | 2 000 000 | one blob, in bytes, decompressed | from the object **header**, before anything is inflated | the row prints `?` with `too_large` |
| `extract.complexity.max_run_bytes` | 128 000 000 | decompressed content this pass may read in one `fathom index` | before each read, never after | the remaining rows print `?` with `scan_budget` |
| `extract.complexity.max_run_ms` | 60 000 | this pass's own clock, summed over the commits it filled | before each read | the remaining rows print `?` with `scan_budget` |

Three details that are decisions rather than incidental behaviour:

* **The per-file ceiling is this pass's own key, and it is deliberately not `extract.max_file_bytes`.** That
  key belongs to `e3.1`'s structure extractor, and the two would want different numbers for the same file
  forever — an `imports`-passing scan can stop early on a huge file, while a complexity figure has to read the
  whole thing to count its branches. The *code* is shared (`too_large`), because the reason a file was
  declined is the same reason in both cases.
* **A per-run budget is checked before the read.** A budget discovered by overspending bounds nothing, so the
  byte ceiling declines the *next* blob rather than inflating one byte past the line. This is also why `0`
  works as "decline the whole pass" instead of being a degenerate value: `0 >= 0` refuses the first row.
* **`0` does not mean "no cap" here.** It means the opposite — a run that reads nothing. The keys have no
  "uncapped" spelling, because an uncapped contents pass is the thing this ticket was written to prevent; the
  shipped defaults are the no-cap-shaped numbers (4 GiB, two minutes).

The per-file ceiling is chosen so that nobody notices: 2 MB is above the 99.9th percentile of hand-written
source in every language fathom reads. The two per-run ceilings are chosen for the case that *does* notice,
and the arithmetic is in §7 — they are ceilings for the vendored-JavaScript, generated-protobuf,
ten-thousand-commit case, and they are set so that the case they catch is the case that would otherwise cost
more than the entire rest of the index.

One detail that is a decision rather than an accident: **the time ceiling is this pass's own accumulated
clock**, summed over the commits it filled, not a deadline read from the start of the analyses phase. A
deadline would charge the complexity budget for the structural pass and the dependency pass, and then the
number in the report would not be the number that was bounded.

## 3. What a refusal costs, which is the order the pass is written in

On a monorepo most file rows are declined, and the pass must not pay for a refusal it could have seen for
nothing. So `Fill::side` asks its questions in cost order and stops at the first yes:

| Order | Question | Cost of asking | Reason |
|---|---|---|---|
| 1 | Did this path not exist on this side? (`add`'s base, `delete`'s head) | nothing — a stored column | `absent_revision`, a **measured zero** |
| 2 | Did the spine measure the file binary? | nothing — a stored column | `binary` |
| 3 | Does the extension name none of the six grammars? | a path string | `language_unknown` |
| 4 | Does the store already hold this side's figure? | one index probe | the stored figure |
| 5 | Is a per-run ceiling already spent? | nothing | `scan_budget` |
| 6 | Has this run already counted this very blob for this path? | a window lookup, no read | the figure it counted |
| 7 | Is the blob over the per-file cap, missing, or not a blob? | an object header | `too_large`, `contents_not_kept` |
| 8 | — parse it, count the branches | the read, and the parse | a figure, or `parse_failed` |

Rows 1–3 are why a real repository's first index is cheap: a monorepo's file rows are mostly not source. Row
4 is the store's memo, and the reason a warm run reads only what is new. Row 6 is the same argument one scale
down, inside a single run — the next section is about it. Row 7 is why a 400 MB vendored file costs this pass
a header read instead of a 400 MB inflate.

Row 4 has one rule attached to it, and it is the rule that makes ADR-007 §9.2's telescoping identity
(`Σ before − Σ after = V(G₀) − V(Gₙ)`) a property of the code rather than a reconciliation step: **a base-side
figure is taken from the first parent's stored head figure whenever that parent touched this path.** A rename
is excluded — the parent's figure for a renamed path lives under the *old* path hash, which the head row does
not carry, so borrowing there would put one path's complexity on another path's row. A rename measures its
base side from the pre-rename blob instead, which is the same number arrived at honestly.

### The run's own memo (`reco-xsog`)

Row 4 has a hole a cold index falls straight into: on a first index there is nothing in the store to borrow,
and the parent almost never touched this path — on the 500-commit perf fixture it had something to lend for
24 sides out of 630. But a figure is a property of **these bytes under this path**, not of the commit that
quoted them, and a history is mostly old content versions coming back. So the run keeps the last few figures
it counted per path, keyed by the blob each was counted from, and a side whose blob equals one of those takes
the figure instead of a second inflate and a second parse.

The analyses loop walks the owed commits in ancestry order — parent before child, which is what
`structure.delta` needs for the same reason — so the pairing is the ordinary one: a row's base side is the
content version an earlier row for that path counted as its head. Measured on the perf fixture, **313 of the
699 sides the pass could have counted were ones it had already counted (45 %)**, and the pass went from
619 ms to 372 ms of a 1 490 ms cold index — which is the whole reason `cold.index.ms.small` is back inside
1 500 ms (ADR-004 §11.10).

Three properties keep it a memo rather than a guess:

* **The blob id has to equal the one the figure was counted from.** Not the path, not the file name, not the
  commit: a rename, a revert and a merge that landed two versions of one path all fall back to a read, which
  is everything this pass used to do. A miss costs a read and nothing else.
* **Only a figure counted from content is remembered.** A `scan_budget` refusal is a fact about the run and
  not about the bytes, and remembering it would let a spent ceiling leak into a row the next run should
  have filled.
* **The window is per path and four wide.** Two slots cover a straight line — the version a row's base side
  needs is the one the previous row for that path counted — and the spare two are for a merge that lands two
  versions between the rows that reference them. Past the window the pass reads again. A bound, not a cache
  that grows with the history: this is the pass whose cost a monorepo already complains about.

## 4. Why a ceiling is not a hole in the index

Two rules, both of them load-bearing:

* **A refusal by a ceiling does not close the row.** `scan_budget` and `too_large` stay in the owed set that
  decides what a run may skip, so raising `extract.complexity.max_run_bytes` and re-indexing fills what this
  run declined — the series of budgeted runs converges instead of calcifying. Every other reason
  (`language_unknown`, `binary`, `parse_failed`, `not_utf8`, `grammar_not_compiled`) is a property of the
  bytes, closes the row, and is not asked again: re-reading answers the same way, and a warm run that
  re-refused them would be a run that never finishes converging.
* **A refusal is never a figure.** `?` with a reason is a different stored row from `0` with a reason, and
  only the second one feeds a sum. `fathom complexity` renders `?` rather than rounding a refusal into a
  number that reads as "simple" — ADR-007 §7.3 refuses a metric that can be lowered by deleting a file, and a
  `0` written where the pass declined is that refusal arriving from the other side.

## 5. What you are told, in the order you are told it

The honest-progress contract, from the first tick to the doctor:

| Where | What it says | Example |
|---|---|---|
| the progress row (`fathom index`, TTY) | the phase's counts, plus a note naming the spent ceiling — kept on the row for the rest of the phase, so the tick that reaches 100 % still carries it | `analysing 341 / 341 commits 100 % · complexity byte budget spent` |
| the run report (`fathom index`) | what was read, what was refused, under which basis, and which key would read more | `complexity pass: 1 204 file row(s), 318 MiB read, 9 214 ms; stopped by extract.complexity.max_run_bytes — the rest print `?` with `scan_budget` and stay owed` |
| `fathom index --json` | the run's own ledger under `complexity_fill`: rows, measured, refusals by code, bytes read, the pass's ms, base sides reused, the ceiling that stopped it, and the three ceilings in force | `"complexity_fill": {"rows": 1663, "measured": 412, …, "ceilings": {"max_run_ms": 60000, …}}` |
| `fathom doctor` / `fathom index --stats` | the coverage line §9.6 asked for, with **measured** and **carries a figure** kept apart, and the pass line (`index.complexity_pass` in `--json`) | `complexity 7 of 7 file row(s) measured (100 %), figure on 1; basis mccabe-1; reasons: scan_budget 4, language_unknown 2` |
| `fathom doctor`, as a finding | a store holding `scan_budget` rows, with the key to raise in the remedy | `4 file row(s) print `?` with `scan_budget` because a per-run ceiling was spent: extract.complexity.max_run_ms` |
| `counter` table | the durable half: `complexity.rows_measured`, `…bytes_read`, `…ms`, `…refused.<code>`, `…budget_spent.<key>` | |

Two things that line is careful about, because a coverage line is exactly where a half-filled column lies
quietly:

* **"measured" and "carries a figure" are two numbers.** A run that looked at every row and was refused by a
  budget has filled the column 100 % and printed a figure on a third of it. The line says `figure on 1` when
  the two differ, and says nothing when they do not.
* **The finding only fires on a refusal a ceiling caused.** `binary` and `language_unknown` rows are the index
  being correct, not the index being stuck, and a doctor that complained about them would be trained away in a
  week. It fires on `scan_budget` — the one code whose remedy is a person raising a number.

## 6. The absence matrix: what a reviewer reads, and what would fill it

Every way this pass can come back without a figure, in the order the pass discovers them. ADR-007 §7.1 is the
full vocabulary and the sentence each one prints; this is the short form, with the verb.

| Absence | Reason on the row | What the reviewer reads | What fills it |
|---|---|---|---|
| the path did not exist on that side | `absent_revision` | a **measured `0`**, not a `?` — an add's base and a delete's head are genuinely empty | nothing: it is an answer |
| git says the file is binary | `binary` | *git reports the file as binary* | nothing. `fathom diff` still shows the byte change; a figure would be a fiction |
| the extension names no grammar fathom reads | `language_unknown` | *not one of the grammars fathom reads* | nothing today. This is the majority of a real repository's file rows, and `fathom doctor` counts it so the majority is visible rather than implied |
| a grammar exists but this build was compiled without it | `grammar_not_compiled` | *this build was compiled without the `lang-x` grammar* | a build with `langs-all` (or that one feature), then `fathom index --refresh` |
| the contents were never kept at this revision | `contents_not_kept` | *the file's contents were not kept at this revision* | a fuller clone — unshallow, or re-fetch — then `fathom index --refresh`; `fathom doctor` names the commits the object database is hiding |
| the file is over the per-file ceiling | `too_large` | *larger than the {limit} byte limit* — naming **which** key | raise `extract.complexity.max_file_bytes`, then `fathom index`. The row stayed owed, so this actually fills it |
| a per-run byte or time ceiling ran out | `scan_budget` | *the per-run budget `extract.complexity.max_run_bytes` was already spent; raise it* — and the progress row said so while the run was still going | raise the named key, then `fathom index`. Every declined row is still owed |
| the parse returned more error than code | `parse_failed` | *the parse reported {n} errors and returned no structure* | fixing the file, or a grammar upgrade; the row is a property of the bytes and stays closed |
| the file parsed but a body was not there to count | `body_not_visible`, `node_budget` | a `~`: *{n} of {m} functions have no body in this file* | usually nothing — a trait signature has no branches. `node_budget` is filled by raising `extract.complexity.max_nodes` |

Three of those rows close the file for good (`binary`, `language_unknown`, `parse_failed`): they are properties
of the bytes, and re-running cannot change an answer about bytes. The three above them are budget or retention
answers, and they are the reason the owed set reopens them: **a refusal caused by a number somebody set has
to be re-askable**, or raising the number would be a no-op dressed as a remedy.

## 7. Sizing it, with the numbers that set the defaults

The shape of the bill: the pass costs one inflate and one parse per *source* file row, and nothing else. A
run over a repository whose file rows are mostly vendored or binary pays for the refusals, which are nearly
free (§3), and not for the bytes.

The measurement that set the defaults is `benchmarks/reports/e8-4-index-time-complexity.json`, written by
`tools/complexity_cost.py`, which runs **the same binary twice** — once with `max_run_ms = 0`, so the pass
reads nothing, and once at the shipped defaults — and reports the difference. Defining "before" as the same
build with the pass declining, rather than as yesterday's commit, holds the binary, the schema and the
reason-coding fixed and varies exactly one thing: whether file contents are read. A second report,
`e8-4-index-time-complexity-monorepo.json`, carries the same pair on the monorepo-shaped corpus; reproducing
either is one command (§9).

What it measured, on two corpora, is what changed the defaults. Both are committed, both name their corpus,
and neither re-clones anything — the second is a shallow clone of a generated Go SDK that somebody put in a
directory, because a benchmark that fetches its own corpus has a corpus that changes on its own.

| corpus | cold index, pass declining everything | cold index, as shipped | the pass itself | what it refused |
|---|---|---|---|---|
| this repository — 217 commits, 1 905 tracked files (`e8-4-index-time-complexity.json`) | 112.1 s | 132.2 s (**+18.0 %**), peak RSS **+4.0 MiB** | 18.4 s, 93.9 MiB read, 4 380 rows, 3 088 measured | nothing: the whole repository fits inside both ceilings |
| `aws-sdk-go`, shallow, 5 501 commits, 66 885 file rows (`e8-4-index-time-complexity-monorepo.json`) | 121.2 s | 135.5 s (**+11.8 %**), peak RSS **+1.6 MiB** | 15.3 s, 122.3 MiB read, 66 885 rows, 10 403 measured | 122 661 sides — `language_unknown` 46 785, `scan_budget` 75 847, `too_large` 12, `binary` 17 — every `scan_budget` and `too_large` one of them owed |

Four things worth reading there rather than skimming past.

**The refusals are the run.** On the second corpus 46 785 of 66 885 file rows are refused from the path alone
— generated Go that no grammar claims, declined without a byte inflated — and on the first, 3 427 of 4 380 are.
That is what makes the pass affordable at all: a design that opened every file to find out would have spent its
whole byte budget on files it was never going to count.

**The percentages are a share of something else.** This repository's shipped run spends 108.8 s of its 132.2 s
on the module graph, which the report records in `phases_ms` so a reader can see what the 18.0 % is 18.0 % of.
Both reports record `per_rep_pct` too — 19.7 / 16.2 and 11.3 / 12.2 here — because a cold index on a shared
machine is a noisy instrument and a headline without its scatter is a number pretending to be more than it is.

**The byte ceiling spent itself, unasked.** On the monorepo the shipped arm reported
`declined: "extract.complexity.max_run_bytes"`: it read 122.3 of its 128.0 MB and stopped, and 75 847 sides
printed `?` with `scan_budget`. Nobody engineered that corpus to trigger it. That is the design working —
an unfinished column whose reader is told which key would have finished it, rather than a column that quietly
wrote zeros for the files it gave up on.

**Peak RSS is nearly flat, and that is the point.** +4.0 MiB and +1.6 MiB, on runs that read 93.9 MiB and
122.3 MiB of file content. The largest blob this pass inflates is 2 MiB, so its own footprint is a couple of
megabytes and what remains is allocator churn — which is why the allowance below is wide, and why the honest
claim is "no memory cost that scales with history" rather than a figure a runner's malloc could refute.

Reading 512 MB on that second corpus — which is what an earlier cut of the defaults allowed — cost the pass
**60.9 s of its own clock**: 8.4 MB/s, arriving exactly where spike `s1` put the worst grammar, and half again
the whole rest of the run. The shipped byte ceiling is a quarter of that, which is why the same corpus now
pays 15.9 s instead of a minute. The time ceiling is 60 s because a machine that parses at a tenth of that
speed would not have been caught by the byte budget in time, and a *wait* is what a reviewer feels.

The declared allowance is in the report's `gate` block — **+40 % of cold index time and +256 MiB of peak RSS**
— and it is relative for the reason every timing gate here is relative: a cold index's absolute cost belongs to
the machine and the repository, and a gate written in seconds goes red on a busy runner instead of on a
regression. `python3 tools/complexity_cost.py --repo REPO --check` re-measures it and also checks the
committed report against the run it is supposed to describe; `--audit` re-derives a report's own arithmetic
without measuring anything, and is what the contract test runs on both reports on every commit.

Rough sizing, and the only numbers worth typing into `fathom.toml`:

* **one commit's first index** is what `max_run_bytes` and `max_run_ms` are for; both default well above what a
  warm run can reach, so the only run they stop is a cold one.
* **a cold index of a large repository** is where you will meet them. If the run line reports a spent budget,
  the row says what it cost and `fathom doctor` says what to raise; if you would rather the run stayed inside
  a CI step's clock, lower `max_run_ms` and accept that the tail of the history prints `?` until a later run
  fills it — that is the degradation this contract exists to make visible rather than incidental.
* **`max_file_bytes`** is the key to lower first on a repository with generated code: a refusal there costs
  one row of `?` on files whose figure would have been meaningless anyway, and it is the one ceiling that is
  decided without touching the bytes.

## 8. What this is deliberately not

* **Not a per-function census at index time.** One sum per path per commit, plus one maximum (§2 of the ADR).
  Per-function figures stay derived rows, keyed by content (ADR-007 §9.4).
* **Not a quality verdict, and not a gate.** No threshold ships with any of this; the ceilings bound cost, not
  goodness (ADR-007 §8.3, §8.5).
* **Not a second parse.** The figure is counted from the same tree the structure extractor already built where
  it can be; where it cannot be — the spine has the bytes, `e3.1` declined the path — the fill parses once, and
  the memo is the column it writes rather than a cache entry that can be evicted out from under the answer.
* **Not a promise that every file gets a figure.** It is a promise that every absence has a reason, that the
  reason names its remedy, and that a run which declined is a run that says so while it runs.

## 9. Reproducing it

```bash
# the ceilings, the reason codes, the owed-set rules and the progress note, against real git fixtures
cargo test -p fathom-analyze --features sqlite-bundled,langs-all --test complexity_fill

# the cost measurement on a real repository, and the gate over it. The tool removes the store and the
# payload cache to make each run cold and swaps `fathom.toml` to set each arm's ceilings, so point it at a
# checkout you own; it names its corpus because a benchmark that silently changed corpus changed its meaning.
python3 tools/complexity_cost.py --repo /path/to/a/real/repository \
    --corpus-note "how this corpus was obtained" \
    --out benchmarks/reports/e8-4-index-time-complexity.json
python3 tools/complexity_cost.py --repo /path/to/a/real/repository \
    --corpus-note "how this corpus was obtained" \
    --report benchmarks/reports/e8-4-index-time-complexity.json --check

# a committed report's own arithmetic, re-derived from its own medians — no measurement, no build
python3 tools/complexity_cost.py --audit --report benchmarks/reports/e8-4-index-time-complexity.json

# the same store, watched by eye on a scratch checkout: a budget small enough to bite, and what the index
# then says. Set it in the environment or a copy of the config, not in a config you mean to keep — the value
# that empties the column is also a value someone else will inherit.
cargo run --quiet --release -p fathom-cli -- index
cargo run --quiet --release -p fathom-cli -- doctor

# the config keys, the schema defaults and the CI wiring
PYTHONPATH=src python3 -m pytest -q tests/test_complexity_fill_contract.py
```
