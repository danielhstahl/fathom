# Plan versus execution: the contradictions between what an agent said and what it did

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-cpr` (`e4.8`)
- **Implements**: the plan's `e4.8` acceptance criteria — the six drift kinds, each with both sides linked;
  findings bounded by a finite look-ahead window and by caps that say they bit; an episode with an
  unparseable or absent scope is reported as *cannot judge* rather than judged; and a contradiction the
  reviewer already dismissed is counted, not re-reported; ADR-001 §7.2 (a refusal is a pointer, and an
  unchecked list is a refusal made honest); ADR-001 §7.10 (one reality: a drift finding enters the delta
  document through `insight::Finding`, the door every other finding already uses); ADR-002 §4.5 (the
  analysis half is a pure function of what it is handed — only `fathom-cli` reads the store).
- **Executes as**: `crates/fathom-analyze/src/drift.rs` (the vocabulary, the report, the bounds),
  `crates/fathom-analyze/src/drift/detect.rs` (the six rules and the two measurements they share) and
  `crates/fathom-cli/src/drift.rs` (the session read: episodes, intent column, ledger); printed by
  `fathom drift`; pinned by `crates/fathom-analyze/tests/drift.rs` and `crates/fathom-cli/tests/cli_drift.rs`.

---

## 1. What this decides, and what it refuses to

One question, per session:

> **It said it would do four things. Which two did it not do, and where can I check that?**

Drift is a **session** question, not an episode question. A plan item deferred to the next turn is not drift;
a dependency added two turns after the plan that named it is not drift; a "refactor the parser" that touched
only the parser's tests is. So this module reads a session — the episodes in the order a harness recorded
them, each with its recorded intent, its touched paths, and any sign-off `e4.6` holds for it — and reports
where the two halves disagree.

Four refusals:

* **No plan inferred from the diff.** An episode with no recorded intent has no plan, and a detector that
  invented one from the subject line would be reporting its own guess as the agent's promise. Those episodes
  go into `report.unchecked` with the reason and the owning ticket.
* **No reading of the world.** `drift::analyse` touches no store, no clock and no config — a test walks this
  module's own source and fails on a `rusqlite`, a `std::fs` or a `chrono`. That is what makes every finding
  in the report a fixture.
* **No certainty.** Every finding's confidence is the intent rung's price scaled by the rule's own base, and
  the finding prints which rung decided (`intent_source`). A plan that arrived as a hook prompt is a
  stronger claim than one that arrived as an `Intent:` trailer, and the report says so instead of letting a
  self-reported trailer read as a harness event.
* **No re-litigating a dismissal.** Every finding carries a suppression key built from the rule and the plan
  text — never from the episode id, which an amend changes — and a dismissed key produces no finding and one
  line in `report.suppressed`.

## 2. The six kinds, and what each one means operationally

The closed vocabulary is `drift::KINDS`; a kind is one function, one threshold and one severity, and the
severities are the delta model's own words (`high`, `notable`, `note`) so no fourth vocabulary appears.

| kind | the question it answers | how it decides | base |
|---|---|---|---|
| `plan-unimplemented` | which open plan items have no corresponding change? | token overlap of the item against the window's corpus (subjects, touched paths, manifests) below `min_overlap` | 0.62 |
| `test-promised-not-written` | which promises about tests were kept off the test surface? | the item names the test vocabulary, is open, and no episode in the window touched a test path | 0.74 |
| `refactor-claimed-not-performed` | which structural claims left the surface as it was? | the item names a structural verb and a target, and no window path contains the target and no path move was recorded | 0.58 |
| `out-of-scope-edit` | which paths are outside the declared scope? | `e4.2`'s scope globs against the episode's paths (`scope::deviation`), unless a plan item names the path | 0.92 |
| `unplanned-dependency` | which manifests arrived unannounced? | a dependency manifest touched with no plan item and no scope glob accounting for it | 0.78 |
| `contradicts-signoff` | which plan items re-open work a human already signed off? | an open item overlapping an `accepted` ledger entry's own note | 0.80 |

"Base" is the multiplier the rule brings before the intent rung is applied (see
[`crates/fathom-analyze/src/drift/detect.rs`](../../crates/fathom-analyze/src/drift/detect.rs)): a rule that
is arithmetic on a recorded fact starts high, a rule that infers *resemblance* starts lower, because
resemblance is the weakest link in the argument. The product is clamped to `0.30..=0.95` — never 1.0, and
never so low that a trailer-only repository stops being reportable.

## 3. The two measurements every rule shares

* **`tokens`** — content words: lowercased, split on punctuation *and* path separators, stop-words and
  two-letter fragments gone. A comparison on raw strings would call "bump the parser" and "parser" unrelated
  because of "the".
* **`overlap`** — `|A∩B| / min(|A|,|B|)`, asymmetric on purpose. A three-word plan item has to be found
  *inside* a change's corpus, not matched against it word for word; a symmetric coefficient lets a long
  subject drown a short item.

`min_overlap` defaults to **0.34**. Below it, the corpus's "not done" items matched their way out of a
finding; above 0.5, the genuinely-done ones stopped being recognised. It is a flag
(`--min-overlap`), a `fathom.toml` key (`[drift] min-overlap`) and a row of the report's own `policy` block,
so a finding can always be traced to the threshold that produced it.

## 4. Worked example

One agent turn that declared `Agent-Scope: src/pool/**` and finished by touching a runbook and a manifest
(the fixture in `crates/fathom-cli/tests/cli_drift.rs`):

```
$ fathom drift
3 contradiction(s) across 1 planned episode(s) in 1 session(s) [fathom-drift/1]
  2 episode(s) in 1 session(s); 1 stated a plan
  policy:
    window = 4
    min_overlap = 0.34
    ...
edited outside the declared scope:
  [notable] docs/runbook.md — touched `docs/runbook.md`, outside every declared scope glob — the work was bounded to `src/pool/**`
      said     “src/pool/**” (fathom show ep-de002fb36a277371e617)
      history  “docs/runbook.md” (fathom show ep-de002fb36a277371e617 # docs/runbook.md)
      confidence 0.83 via agent-intent-trailer; dismiss with `drift:out-of-scope-edit:4051acf422399484`
```

Both sides, on adjacent lines, each with the address a reviewer opens. The confidence line is the honest
part: `0.83 via agent-intent-trailer` says the plan was self-reported, which is why the same finding on a
hook-prompt episode would read 0.92.

The same findings, handed to the delta document with `Report::to_insight()`, arrive as ordinary
`insight::Finding`s — so the episode narrative and the summary card cite them through the door every other
finding already uses, with both sides' addresses inside the message text (the delta `Evidence` slot holds one
pointer, and this finding has two).

## 5. The bounds, and how a report says it hit one

* **`window`** (default 4): how far past a plan item to look for the change that did it. A window of 1 makes
  a deferred item drift; the same item is not drift at window 4 — that is the knob's whole meaning, and the
  report prints the value it used rather than leaving the reader to infer it from luck.
* **`max_per_kind`** (6) and **`max_findings`** (20): when either bites, `report.truncated` is true and the
  verdict ends with `the caps cut some; raise --max or narrow --window`. A clean-looking report that was
  quietly truncated is the report that hid the thing you were looking for.
* **`cross_session`** (off): compare across sessions, i.e. treat the run in front of you as one conversation.
  Off by default because Monday's harness run is a different conversation than today's, which is exactly the
  false positive the ticket's third criterion is about.
* **`off`**: a kind switched off is *said* to be off — its tally carries `enabled: false`, so "nothing found"
  and "nothing looked for" are different sentences in the JSON.

## 6. What cannot be judged, and who owns it

| situation | what the report does |
|---|---|
| no intent, no trailer, no plan item | the episode is counted in `unchecked` — "no plan to compare against" (`e4.2`) |
| intent recorded with **no scope** | `unchecked`: "an edit outside the plan cannot be told from an edit inside it" — a missing glob is not a universal glob |
| scope globs that do not compile | `unchecked`: inside and outside are not distinguishable (`e4.2`) |
| test surface unknown for an episode in the window | `unchecked` (`e2.9`) — and **never** a `test-promised-not-written` finding |
| a structural item that names no surface | `unchecked`: there is nothing to check |
| a rename the index did not record | `moved_paths: None`, read as "cannot rule a move out", never as "no move happened" |

The second row is the one that gets a tool into an argument it cannot win, so it is worth naming plainly:
without a declared scope, "you went outside the plan" has no referent. The honest output is the sentence
above, not a guess.

## 7. Reproducing it

```bash
cargo test -p fathom-analyze --features sqlite-bundled,deps-all --test drift
cargo test -p fathom-cli --test cli_drift
# on a real repository, after `fathom index`:
fathom drift                                  # the last sessions
fathom drift --session s-7f2 --window 2       # one session, tighter look-ahead
fathom drift --json | jq '.findings[].message'
fathom drift --dismiss "$(fathom drift --json | jq -r '.findings[0].suppression_key')"
```

Config, in `fathom.toml` (every key also a flag):

```toml
[drift]
window = 4
min-overlap = 0.34
max-per-kind = 6
max-findings = 20
cross-session = false
dismiss = ["drift:out-of-scope-edit:4051acf422399484"]
kinds-off = []
```
