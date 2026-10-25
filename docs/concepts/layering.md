# Layering: what the declared architecture refuses, and what this turn added

- **Status**: **shipped as an engine, a baseline and a per-episode delta.** The consumer that decides
  anything is `fathom check` (**reco-ngk**, `e6.6`), which is here: it reads this module's per-episode delta,
  applies `[check.severity] layering`, and exits 7 on what this range introduced. The scorecard
  (**reco-al1**) trends the same violations. Nothing here writes `.fathom/ratchet.toml`; adding a
  row is `fathom ratchet --update-baseline --reason "…"`, same rule as [the ratchet](ratchet.md) follows.
- **Decided by**: [the architecture graph](architecture-graph.md) (the rows this judges and the lane they
  ride), [cycles](cycle-detection.md) (the cycle engine this declares against — this module does not
  detect cycles, it declares them refused), [the ratchet](ratchet.md) (`kind = "layer"` rows),
  [structure extraction](structure-extraction.md) finding 4 (a Go dependency fact is package-to-package),
  [config](config.md) (the two tables, the layers they arrive through, and the warning channel),
  [ADR-002](../adr/0002-workspace.md) §6.1 (a `--no-default-features` build still answers the question).
- **In code today**: `crates/fathom-analyze/src/layering/` — `rules.rs` (the `[layering]` table, and every
  refusal raised while reading it), `engine.rs` (one revision's assessment), `locate.rs` (which line to
  cite), `finding.rs` (one violation: rule, edge, why, fix), `debt.rs` (the baseline verdict),
  `report.rs` (the episode delta and the exported numbers), `episode.rs` (the lane-backed path).
  `tests/layers.rs` (41 tests) is the acceptance fixture; `tests/layers_episode.rs` (6) is the store
  side, and it is where `verify` lives — the graph built by applying the stored delta rows must produce
  the violations a fresh materialisation of the same revision produces.
- **Read this if**: you want to know why a layering violation is printed as one file, one line and one
  rule name, what makes it *new* rather than merely present, and which rules a `fathom.toml` cannot get
  away with.

`[layers]` says where things live. This says which edges between them are a violation, and — the part that
matters to a reviewer — which of those violations this episode is responsible for.

```toml
[layers]                       # where things live
l0_vocab = ["crates/vocab"]
l1_store = ["crates/store"]
l2_core  = ["crates/core"]
l3_api   = ["crates/api"]

[layering]                     # which edges between them are refused
mode = "warn"
order = ["l0_vocab", "l1_store", "l2_core", "l3_api"]
no_upward = true
no_cycle_within_layer = true
forbid = ["door-through-the-store: l3_api -> l1_store"]
allow  = ["the-read-path: l3_api -> crates/store/src/read.rs"]
ignore = ["crates/**/tests/**"]
```

## 1. Endpoints, and the three shapes a rule takes

An endpoint is a **layer name** (`l1_store`), a **path pattern** (`crates/store`, or
`crates/**/generated/**` — the dialect [the graph docs](architecture-graph.md) publish for `[layers]`:
`*` inside a segment, `**` across segments), or `*`. A bare directory names that directory *and*
everything under it, which is what makes a layer name and a path prefix the same kind of thing.

Three shapes, one arrow each:

| Written | Means | Fires on |
| --- | --- | --- |
| `forbid = ["src -> dst"]` | no module matching `src` may depend on one matching `dst` | the edge |
| `allow = ["src -> dst"]` | an exception to a refusal, and it must be **narrower** than the refusal | — (it subtracts) |
| `only = ["dst <- src"]` | `dst` may be reached *only* from `src` | the edge that reaches `dst` from elsewhere |

`only` points the other way on purpose: the sentence is about the thing being reached. Several `only`
entries naming the same target are **one allow-list** — `db <- api` plus `db <- db` says the api layer and
the layer itself may reach it, and nothing else may. Two entries that contradict each other are worse than
no entry, so the union is the reading.

Then four switches: `no_upward` (against `order`, bottom-first), `no_cycles`, `no_cycle_within_layer`, and
`ignore` for regions nobody is judging. The two cycle rules do not implement cycle-finding; they take
[e3.5's](cycle-detection.md) strongly-connected components and the same shortest witness, and add the
judgement. One cycle engine, two declarations against it.

A rule may be named (`my-name: l3_api -> l1_store`) or left unnamed, in which case it is named by its own
body (`forbid:l3_api->l1_store`). Never by its position: a baseline that pinned rule number three would
move the moment someone inserted a rule above it.

## 2. A rule that cannot match anything is refused, not obeyed

The fourth acceptance criterion is the one that decides whether the other three can be trusted. A config
that silently enforces nothing is worse than no config, because the reader believes it. So
`Rules::from_repo_config_text` returns `(rules, notices)`, refuses the rule **and drops it from the set in
force**, and says which line and why:

| Refused | Why the message says |
| --- | --- |
| endpoint names no declared layer | the nearest layer name, or "no `[layers]` table is declared, so nothing is a layer name here" |
| `no_upward` with an empty `order` | "an upward reference is only upward relative to a declared order" — and the rule is **off**, not silently passing |
| `order` names a layer `[layers]` does not declare | which name, and the layers that are declared |
| `only` written with `->` | the boundary spelling, because obeying `a -> b` in that list would enforce the opposite of what was written |
| a rule with no arrow at all | both spellings, since the arrow is the clause that says which rule it is |
| the same rule twice | the second one is dropped: two rules with one name would move a baseline that pins it |
| `allow` that no `forbid` is at least as broad as | "names an exception to nothing", with the pair to write instead |
| `mode`, `max_violations`, `ignore` of the wrong shape | the bad value, verbatim, and what was kept instead |

Three states are printed and never confused: **no `[layering]` table** (nothing is enforced, and the report
says so rather than printing a clean bill), **`mode = "off"`** (nothing was examined, deliberately), and
**rules in force that found nothing** (which is the answer, and the only one of the three that is).

A rule can also be in force over an edge kind that does not exist, which is none of the three and worse than
the first: the numbers look like the third one. A `[layers]` table whose entries are crate directories, with
rules written between them, asks the module graph about *crate* edges, and in a Cargo workspace the
extractor writes none — every `use fathom_store::…` leaves the corpus and is stored as nothing (item 4 of
[the graph page](architecture-graph.md)). The assessment then reports every row as judged and clear,
which is true of the rows and says nothing about the edges the rules were written for. So a rule list has to
be checked against what the corpus can show it, and where it cannot, the honest line is `[check.severity]
layering = "off"` in the repository's own `fathom.toml` beside a sentence about where the rule really lives.
This repository does exactly that: its crate boundaries are ADR-002 §5, enforced by
`python3 tools/check_workspace.py lint` (`E01`–`E05`, `M02`, `M03`) against the Cargo manifests, and
`tests/test_workspace_contract.py` fails if the `[layers]` table and the ADR's crate list drift or if the
gate stops saying out loud that the layering check is off here.

A layer declared but left out of `order` is a caveat, not a refusal: its edges are counted `unranked` and
not judged for upwardness, because guessing a rank from the order the table was written in would be
inventing an architecture.

## 3. What one violation says

```
layering.forbid `door-through-the-store` (layering.forbid, line 9)
  crates/api/src/router.rs:9 → crates/store/src/sink.rs
  why: the door may reach the store only through the module the boundary names
  fix: route it through the read path, or write the exception you mean: allow = ["crates/api/src/router.rs -> crates/store/src/sink.rs"]
```

Rule name, where the rule was declared, the offending edge with `file:line`, why, and a fix that is
concrete enough to paste. The line comes from the same structural records the graph was built from
(`e3.1`'s import statements), read through [the materialisation lane](graph-storage.md) — so the citation
is the line the *statement* is on, not a line guessed from the file's shape. Where two statements name the
same target, the named one is cited and the report says so; where the substrate recorded no span, the
violation says `only the file can be cited` rather than inventing a line. A violation about a *cycle* cites
the edge that closes the loop and carries the member set and the shortest witness beside it, because the
pair that closed it is rarely the fact about it.

The JSON export keeps the contract as field names: `rule`, `rule_kind`, `claim`, `edge`, `src`, `dst`,
`line`, `why`, `fix`, `key`, plus `unknown_contents` when the far end was a wildcard leg the extractor could
not resolve.

## 4. Judged at the granularity the extractor guarantees

`e0.12`'s checkpoint on this ticket: a rule that fires on a file-level rendering of a package-level fact is
firing on an artefact of the scanner. So a Go edge — `granularity: package`, a `package` claim — is judged
as **package to package**, and the violation names the two packages, not one file in each, and says it
names packages. A layer tag is read off the package's own files; where `[layers]` globs cut a package in
half, the package has no single layer and its edges are counted **indeterminate** and not judged. Two
answers to "which layer is this package" is a question about the config, and the report says which files
disagreed.

The same care applies to a wildcard leg: `use store::*` supports a claim about `store`, so it can produce
a violation, but the row cites a named statement when one exists and marks the finding when it does not.

## 5. The delta, and the line between news and debt

Two revisions in [the lane](graph-storage.md) produce two assessments, and `Delta::of(before, after)`
names what moved: `introduced`, `resolved`, `persisting`. That is the reviewer's number. The repository's
number is the full assessment, and both are printed, because "2 new, 41 total" is the sentence that lets
someone decide.

Then the baseline, in the same `.fathom/ratchet.toml` the cycles lane uses, as rows with `kind = "layer"`
and a key of the form `rule::src->dst` (a cycle rule's key is `rule::member|set`, matching the way
[the cycle ledger](cycle-detection.md) matches member sets). Every current violation lands in exactly one
of four boxes:

- **violations** — no allowance. These are the news, `introduced: true` when this episode caused them.
- **debt** — a live allowance: owner, reason, days left. Never a violation, at any mode.
- **back** — the allowance lapsed and the violation is still there. Due, with the day it lapsed.
- **stale** — the allowance outlived the violation. Offered as a tightening; never applied here.

A key written with a unicode arrow, extra spaces, or the cycle members in a different rotation matches the
generated key, because a baseline whose matching depends on how the arrow was typed is a baseline people
work around. A key with no `::` matches nothing and says so: a bare pair would not name which rule it
forgives.

The exported object carries `violations`, `new_violations`, `baselined_violations`,
`resolved_violations` and `edges_checked`, and each figure is rederivable from the lists in the same object
— which is what `tests/layers.rs` asserts, since a number that cannot be recomputed is a number nobody can
debug. The delta also says `episode: false` when there was no earlier revision: a cold index has no
introduced and no resolved because nobody asked the question, and two zeros must not be printed as "nothing
changed".

## 6. What is not here

No CLI verb, no scorecard, no writer for `ratchet.toml`, no policy about *which* architecture is right.
The scorecard (**reco-al1**) and the CI gate (**reco-ngk**) are the consumers; `fathom config explain
layering.mode` works today because the keys are declared like every other key.
