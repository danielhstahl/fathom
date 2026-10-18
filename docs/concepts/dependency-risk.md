# Dependency risk: who asked for this, and should I be worried?

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-yxi` (`e2.7`)
- **Implements**: the plan's `e2.7` acceptance criteria — provenance for a new dependency, optional
  OSV advisory lookup behind explicit network consent, and licence-policy checks configured from
  `fathom.toml`; ADR-002 §4.5 (the analysis half reads no filesystem, clock, environment or socket);
  ADR-001 §7.5 (consent a cloned repository can grant is not consent); ADR-004 §5.5 (the answer is
  cached behind a digest over every key it read); ADR-005 §2.2 (hooks v1 stores `prompt_sha256` and
  `prompt_chars`, not the prompt); [dependency-delta.md](dependency-delta.md), which owns the shape
  of a dependency change, and [summary-card.md](summary-card.md), which owns where this card is
  printed.
- **Executes as**: `src/recommender/deprisk/` (`model.py`, `policy.py`, `osv.py`, `licence.py`,
  `provenance.py`, `delta.py`, `render.py`, `explain.py`, `corpus.py`, `eval.py`, `cli.py`),
  `tests/test_deprisk_contract.py`, and `python3 -m recommender.deprisk …`. The Rust home is
  `fathom-analyze::deps` (team 5), with the card in `fathom-cli::verbs::deps`.

---

## 1. What this decides

Two questions per episode, and the second one is worthless without the first:

> **Who asked for this dependency — and should I be worried?**

`e2.6` answers what moved in a dependency set: added, removed, upgraded, source switched, licence
changed, with an edge path on every row. That is the *arithmetic*. It is not the review question.
The review question is asked by a person looking at `+ fastparse 2.2.0` in an episode titled "speed
up the parser", and it has two halves with completely different failure modes:

* **provenance** fails by being blank. A reviewer who gets no answer reads that as "nobody
  asked", which is a worse outcome than a tool that said "not recorded, and here is what would
  fill it". Silence is not neutral here; it is a false accusation by omission.
* **risk** fails by being quiet. "No advisories found" about a cache nobody refreshed in seven weeks
  is the most dangerous sentence this tool could ever print, and it is a sentence that *sounds*
  like good news.

So the module's rule for both halves is the same one the siblings use: **an unknown is an answer
with a remedy attached, never an empty field.**

| Question | Answer | Where |
| --- | --- | --- |
| Who asked for this new direct dependency? | the episode under review, else a recorded introduction, else `not recorded` plus the two things that would fill it | `provenance.py` |
| What was the intent behind it? | quoted text with its source and confidence, or which half is missing (`not captured`, `digest only`, `not used`, `guess`) | `provenance.py` |
| Does an advisory apply to this package at this version? | from the local OSV cache, always with its age and its coverage | `osv.py` |
| Does this repository's licence policy allow this licence *here*? | a matrix: copyleft class × (library layer / everywhere else), with the edge path that asked for it | `licence.py`, `policy.py` |
| Should I be worried? | a finding table with severity, anchor, evidence and edge path, plus one deterministic sentence | `delta.py`, `render.py` |

## 2. Who asked for this

The provenance answer is a **ladder**, and it stops at the first rung that has an opinion:

1. **this episode** — the package is new in this episode's delta, so this episode introduced it.
   This is the common case, needs no index, and is the rung most questions are answered on.
2. **a recorded introduction** — an earlier episode's record says it added this package
   (`deps.provenance.search_previous_episodes`, on by default, because the question is usually asked
   weeks after the fact). A record for the same name at a *different* version still answers and
   says so in its evidence, because "who asked for `jwt`" is a question about the name.
3. **not recorded** — printed as the answer, with the remedy: `review the episode that added it, or
   index from farther back`. Never a blank line.

The intent half is a separate verdict, because knowing the episode is not knowing the reason:

| Intent evidence | Stated as |
| --- | --- |
| `Agent-Intent:` trailer | the answer, quoted, with source and confidence — the path that works today |
| harness turn prompt text | the answer, only if intent capture (`e4.2`) stored the text |
| `prompt_sha256` + `prompt_chars` | **`not captured — prompt digest only`**. A digest proves a request happened; it does not say what it was, and hooks v1 stores no text (ADR-005 §2.2) |
| plan item / review note / manual note | the answer *if* the source is listed in `deps.provenance.intent_sources`, and printed as `not used — … is not in deps.provenance.intent_sources` if it is not |
| anything below `deps.provenance.min_confidence` | printed as a labelled `guess`, never silently dropped |
| attribution (`agent codex`) | not an answer by default (`deps.provenance.agent_counts_as_intent = false`). It says who *executed* the change. Turning it on makes `intent: not recorded` impossible to ever see |

## 3. Should I be worried — advisories, with their age attached

Advisory lookup is an **enrichment, never a precondition**. The card renders with no cache, a stale
cache, a corrupt cache, or no network, and in each case says which happened.

**Three gates must agree before a byte is sent.** All of these, and nothing less:

1. `deps.advisories.enabled` is true;
2. `deps.advisories.mode` is `refresh` (the default is `cache`: read the cache, never fetch);
3. `privacy.advisories` is true — **user-scoped**, so a cloned `fathom.toml`, a `--set`, or anything
   a repository controls cannot grant it (ADR-001 §7.5).

`fathom deprisk consent` prints the three gates and the verdict:

```
$ python3 -m recommender.deprisk consent
network consent for advisory lookups
  `deps.advisories.enabled` = true
  `deps.advisories.mode` = cache
  `privacy.advisories` = false (built-in default)
  endpoint: https://api.osv.dev (fixed in code: a config key here would let a clone redirect the fetch)
  verdict: cache only: `deps.advisories.mode = "cache"`, so nothing is fetched. Ask for it with `advisories --refresh`
  default config makes 0 network call(s)
```

Three things were refused on purpose:

* **no `deps.advisories.endpoint` key.** An endpoint read out of a cloned file is an endpoint an
  attacker chose, and the "no network by default" claim would then only have been true of the
  default config. `https://api.osv.dev` is fixed in code, and `_osv_query` refuses anything else.
* **no fetch outside `advisories --refresh`.** No render verb takes the flag. The one call site is
  `cli._osv_query`, imported lazily, and `advisories --plan` prints every request a refresh *would*
  make without making one. Without consent the verb exits **8** (`NETWORK_CONSENT_REQUIRED`) naming
  the gate that refused — at the gate, not at the socket.
* **no unbounded refresh.** `deps.advisories.max_packages` caps the query count per episode and
  `deps.advisories.timeout_ms` bounds each request, because a lookup that can hang is a lookup that
  blocks episode rendering, which the ticket forbids.

**Freshness is a first-class answer.** Every cache read lands in one of five states, and the card
says which:

| State | What the card may claim |
| --- | --- |
| `fresh` | "no advisories found for N package(s) checked" |
| `stale` (past `max_age_hours`) | same claim, plus "past `max_age_hours` — still used, age stated" |
| `expired` (past `stale_limit_hours`) | **the tense changes**: `not checked recently`, never `no advisories found`. A seven-week-old clean result is not a clean result |
| `missing` | "no advisory cache to read at `<path>`", the card renders anyway, and every package is listed as not checked |
| `offline` | "advisories were not consulted (`deps.advisories.mode = "offline"`)"; the cache is not even opened |

A package with no cached entry is **never** counted as clean, and neither is a package whose last
query *failed*: `deps.advisories` stores failed queries as failures, with the message, so the next
render says "the last query failed (timeout after 1500 ms)" instead of implying it came back clean.
Advisories with no upstream severity are printed as `unseverity` rather than as nothing.

The cache is an ordinary file at `deps.advisories.cache` (default `.fathom/advisories.json`, which
`.gitignore` already covers). It is written only by a consented refresh, and
`cache --import-from FILE` fills it offline from recorded OSV responses — the route CI and airgapped
machines use, and the route this document's examples were produced by.

## 4. Should I be worried — the licence matrix

A licence is not one question, it is two: what does it *say*, and what layer is now asking for it.
So the policy has two configurable halves.

The **class tables** are lists of SPDX ids, not baked-in opinions: `deps.license.permissive`,
`deps.license.weak_copyleft`, `deps.license.strong_copyleft`, `deps.license.network_copyleft`, plus
`deps.license.deny` as a veto no matrix cell can out-vote. Expressions are parsed: `AND` is
cumulative so the most restrictive part decides; `OR` is a choice, so the least restrictive option
is taken, named (`taken as MIT`), and the options not taken are printed rather than dropped.

The **matrix** then says what a class may do, and it is the whole point of doing this in config:

| class | in the library layer | elsewhere |
| --- | --- | --- |
| strong copyleft (GPL…) | `deny` | `warn` |
| weak copyleft (LGPL, MPL…) | `warn` | `allow` |
| network copyleft (AGPL, SSPL…) | `deny` | `deny` |
| unknown / undeclared | `warn` (as a stated partial) | `warn` |

`fathom deprisk matrix` prints what is currently in force; `fathom deprisk policy` prints every key
with the layer that won it. `deps.license.library_layers` is matched against `[layers]`, and this
repository dogfoods the key — `fathom.toml` says its library layers are `l0_vocabulary` through
`l3_analysis`, which is why a GPL dependency arriving in `crates/fathom-analyze` is a refusal here
rather than a note in a card nobody reads.

Every licence finding carries the **edge path** that asked for the package, because "who asked" is
the half a reviewer can act on:

```
Licence refused by policy: ring 0.16.8 — GPL-3.0-only · at crates/fathom-analyze/Cargo.toml
  - licence GPL-3.0-only as declared in crates/fathom-analyze/Cargo.toml
  - offending edge path: fathom-analyze → jwt 2.1.0 → ring 0.16.8
```

Four states are distinguished, because "no violation found" means something different in each:
`new` (this episode asked for it), `drift` (the declared licence moved while the version may not
have), `unchanged`, and `removed` (a departing package is never a policy question). A package with
no declared licence is a **stated partial** — `license-unknown` — and never a pass.

## 5. The card

`fathom deprisk demo` renders and audits this exact fixture (`advisory_hot`, a recorded cache):

```
──────── dependency risk · ep-63
  add refresh-token rotation to the auth service
  ep-63 · 3 new direct dependencies (jwt 2.1.0, ring 0.16.8, thiserror 1.0.61); intent
  recorded for all of them; 3 advisories (1 critical, 1 high, 2 with a fix)
  advisory cache 1 h 0 m old (.fathom/advisories.json) · consent: cache only:
  `deps.advisories.mode = "cache"`, so nothing is fetched. Ask for it with `advisories
  --refresh`
  confidence 1.00 · 0 stated partial(s) · 10 finding(s)
who asked for this
  jwt 2.1.0 (rust, direct)
    introduced by: episode ep-63 (commit 7788abc) — added by the episode under review
    intent: "add refresh-token rotation to the auth service"
      source: `Agent-Intent:` trailer (confidence 1.00) · attribution: agent codex
    asked by: fathom-cli → jwt 2.1.0
    declared in: crates/fathom-cli/Cargo.toml
advisories
  cache: .fathom/advisories.json · 1 h 0 m
  !! jwt 2.1.0 · GHSA-7jvh-c3cj-qqh6 critical — jwt: signature bypass when the algorithm
  header is not validated
      fix: 2.1.4 (installed 2.1.0)
  !! ring 0.16.8 · GHSA-h4mh-5fw9-j367 high — ring: buffer overrun in the AES-GCM assembly
  path on 32-bit targets
      fix: 0.16.9 (installed 0.16.8)
   . jwt 2.1.0 · GHSA-4qw4-jh47-jh97 moderate — jwt: denial of service through a deeply
   nested claim set
  no advisories for 1 checked package(s) (cache 1 h 0 m)
  (network: cache only: `deps.advisories.mode = "cache"`, so nothing is fetched.)
licence
    jwt 2.1.0 licence MIT · [permissive · allow] · layer l5_composition (layers) · via
    fathom-cli → jwt 2.1.0
      - rule: permissive outside the library layer, and `deps.license` is `allow`
      - licence MIT as declared in crates/fathom-cli/Cargo.toml
      - resolved from registry:crates.io
findings (10) · critical 1 · high 1 · notable 1 · note 7
  Critical advisory: jwt 2.1.0 · GHSA-7jvh-c3cj-qqh6 · …
```

Six claims are re-read out of the finished lines by `render.audit`, because each is the kind of line
a later refactor drops while every test still passes: the freshness line, every provenance answer
(including a `not recorded` one), every violation's edge path, every finding or its counted
remainder, the stated confidence, and header counts that match `view.counts()`. `card --check` and
`demo` exit non-zero when the audit fails, so the audit runs in CI rather than in a unit test nobody
trusts.

## 6. Verbs

| Verb | Prints |
| --- | --- |
| `fathom deprisk card` | the whole card (`--fixture NAME` \| `--manifest FILE\|-`, `--json`, `--check`) |
| `fathom deprisk summary` | the one sentence |
| `fathom deprisk provenance` | the "who asked for this" rows on their own |
| `fathom deprisk license` | licence rows, drift, and the newly-added licence ids (`--violations`) |
| `fathom deprisk findings` | every finding with evidence, filterable by `--severity` / `--kind` |
| `fathom deprisk advisories` | cache state, freshness, coverage; `--plan` shows a refresh, `--refresh` does one (exit 8 without consent) |
| `fathom deprisk cache` | inspect the cache, or fill it offline with `--import-from` |
| `fathom deprisk consent` | the three network gates and the verdict |
| `fathom deprisk policy` / `matrix` / `rules` / `definition` / `explain` | every key with its winning layer, the matrix, the finding table, the rules as sentences |
| `fathom deprisk score` | the corpus scored against its own bars — the CI gate |
| `fathom deprisk property` | reconciliation on generated episodes |
| `fathom deprisk demo` | this document's card, audited |

## 7. Configuration

Every key is declared in `e0.8`'s schema (`src/recommender/config/schema.py`), appears in
`schemas/fathom.schema.json` and in [config.md](config.md), and is read as a value — no analysis
module reads a file, an environment variable or a clock. `fathom deprisk policy --digest-only` prints
the digest that invalidates a cached card when any of them moves.

The ones worth knowing: `deps.advisories.mode` (`cache` by default), `deps.advisories.max_age_hours`
(24) and `stale_limit_hours` (168, and it must be the larger of the two — the analyzer refuses the
pair otherwise rather than printing a self-contradicting card), `deps.advisories.max_packages` (200),
`deps.advisories.timeout_ms` (1500), `deps.license.deny`, the four class tables,
`deps.license.library_layers`, the six matrix cells, `deps.provenance.intent_sources`,
`deps.provenance.min_confidence`, `deps.provenance.agent_counts_as_intent` (false), and one switch
per finding kind under `deps.findings.*`. Switching a finding off counts it in the card's
*suppressed* line; it does not delete it.

## 8. What this refuses to do

* **No network by default, and no key that changes that.** Verified by tests that replace
  `socket.socket` with a trap and then run every verb.
* **No advisory lookup that can block rendering.** No cache read is on a render path's critical
  path; there is nothing to wait for, and the timeout bounds the one path that could.
* **No invented provenance.** No guessing an introducing episode from dates, authors or commit
  order. Attribution is never intent by default. A prompt digest is never a quote.
* **No "clean" claim the evidence does not license.** Not from an expired cache, not from an
  unchecked package, not from a failed query, not from an undeclared licence.
* **No licence rule that is not config.** The copyleft tables, the matrix, and which of a
  repository's layers are library layers all come from `fathom.toml`; the built-ins are a starting
  point that `fathom deprisk matrix` prints, not a hidden opinion.

## 9. How it is checked

```
python3 -m recommender.deprisk score      # 28 authored cases, 9 gates, all at 1.0
python3 -m recommender.deprisk property   # reconciliation on 200 generated episodes
python3 -m pytest tests/test_deprisk_contract.py
```

The corpus (`src/recommender/deprisk/corpus.py`) is hand-authored, including recorded OSV payloads,
so no test needs a network or a fake server that would not be the real shape. The gates are
provenance accuracy, intent accuracy, licence precision **and** recall (precision is the trap half:
`dual_licence_ok`, `copyleft_in_application`, `dev_only_copyleft` and `removed_dependency` exist to
catch a checker that fires on everything), advisory recall, freshness accuracy, the finding table,
reconciliation and determinism. `tests/test_deprisk_contract.py` maps the four acceptance criteria
one section each, and the analysis half's purity is asserted by AST — the modules that produce the
answer cannot import `socket`, `urllib`, `os`, `pathlib`, `time` or `datetime`.
