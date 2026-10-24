# Fathom reviewed by Fathom — the first pass of the self-dogfood loop (`e7.7`, **reco-rny**)

- **What this is**: the record of running the shipped binary against the repository that builds it, and of
  treating what it says as ordinary beta intake — same register, same seven dispositions, same cadence page as
  an external reporter's mail. Written by hand from a real pass, because the point of the ticket is that the
  pass *happens*; [`.github/workflows/self-review.yml`](../../../.github/workflows/self-review.yml) is the same
  act automated and pinned to a release.
- **Machine-readable version**: [`fathom-on-itself.json`](fathom-on-itself.json) — the same shape
  `self-review.yml` writes to `self-review.json` on a release, produced here by the same commands.
- **The register it wrote**: [`.plan/intake/2026-W43.md`](../../intake/2026-W43.md) — 12 of its 26 rows are the
  findings here plus the stale-red-list finding at the end of §3.
- **Why the loop is shaped like this**: [`.plan/beta-loop-reco-rny.md`](../../beta-loop-reco-rny.md) — the
  decisions, and the mechanisms that were refused.
- **Headline**: **the tool reports two `error` findings and one blocked check on its own repository, and neither
  error is true.** That is the most useful thing this document could have turned up, and both are filed.

---

## 1 The environment, as the tool described it

```console
$ cargo build -p fathom-cli --all-features && ./target/debug/fathom --version
fathom 0.1.0-alpha.1 features: tui mcp watch sqlite-bundled langs-all deps-all minimal
                                     net-advisories net-llm net-http-transport schema-gen
$ ./target/debug/fathom doctor --json | python3 -c "…findings, exit…"
doctor: OK, 0 finding(s)          # build, sqlite (bundled), index integrity, config layers: all clean
```

`doctor` is the one verb that had nothing to complain about, which is worth stating first: everything below is
not the tool failing to run, it is the tool running and disagreeing with the repository.

| Fact | Value |
|---|---|
| commits / episodes indexed | 128 / 109 (index epoch 1, integrity `ok`, `0 commits owed an analysis`) |
| authorship as recorded | `human` 102, `agent` 7 — in a repository whose commits are written by a coding loop |
| review coverage before this pass | 1 fresh sign-off in the ledger; `108 of 109 live episodes have no fresh sign-off` |
| `fathom check 90c2cf4..defc5f6` | outcome **fail**, 16 findings — 14 `warning`, 2 `error`; **exit 0**, because `mode = warn` |
| checks that ran | cycles (37 examined), api (225), dependencies (166), tests (5), sign-off (5) |
| check that did not | **layering — blocked**: *"`fathom.toml` declares no `[layering]` rules, so nothing is enforced"* |
| `fathom architecture` | **92/100**, 4/5 dimensions measured; coupling 2.5981 edges/module over 943 modules; **13 cycles, 37 nodes inside one, 0 baselined as debt**; boundaries: *"not a clean bill of health, an unwritten one"* |
| `fathom ratchet` | **13 uncovered violations, 0 allowances**, `baseline .fathom/ratchet.toml (none yet)` |
| cache | overall hit rate 10.4 %; `ingest.normalize` 72.1 %, `structure.delta` 33.7 %, **`diff.per_file` 0.1 %** (3339 lookups, 5 hits, 3334 puts); 0 rows ever evicted |
| path taxonomy | `noise unknown 3378` — every file row unclassified, as documented since `e2.2` |

The gate's own summary line, verbatim, because it is the sentence a release reviewer would read:

> `fathom check failed: 16 finding(s) at or above 'warning' in 90c2cf42cd20..defc5f6e54de — worst is error
> [cycles/crates/fathom-core/src/events.rs|crates/fathom-core/src/events/schema.rs]: … no allowance in the
> baseline for this cycle, introduced by this episode — 'check.mode = warn', so this is reported and not enforced`

## 2 What it said, what it was worth, and where it went

Each row is the finding, the judgement, and the bead. Every one of these is in
[`.plan/intake/2026-W43.md`](../../intake/2026-W43.md) with a disposition; the beads were created during this
pass, none existed before it.

### 2.1 A cycle that is a module and its own directory (`reco-wk4`) — the headline

The single `error` on `HEAD`:

```text
crates/fathom-core/src/events.rs → crates/fathom-core/src/events/schema.rs → crates/fathom-core/src/events.rs
— no allowance in the baseline for this cycle, introduced by this episode
```

It is a false positive, and reading the ratchet explains why it is not a one-off: of its 13 uncovered
violations, five are this exact shape — `drift.rs ↔ drift/detect.rs`, `narrative.rs ↔ narrative/compose.rs`,
`scorecard/mod.rs ↔ scorecard/report.rs`, `scorecard/dimension.rs ↔ scorecard/policy.rs` — which is Rust's
ordinary parent-module/child-module idiom, and four more are the spike corpora the cycle detector's own accuracy
numbers are measured on (`spike/s1/corpus/java/…/Circle.java ↔ Shape.java`, `corpus/python/models.py ↔
service.py`, `corpus/typescript/legacy.ts ↔ shapes.ts`), which are **deliberately** cyclic. So the tool's
headline verdict on its own source tree — 13 architecture violations, 13 modules in undocumented cycles — is
a number about its own granularity and fixture handling, not about this repository's architecture. Filed with
both candidate fixes written down, and with the note that whatever survives the decision needs real
`fathom ratchet` rows with owners and expiries, because a ledger of allowances nobody signed is the same lie as
a gate in warn mode.

### 2.2 A test function reported as a breaking API removal (`reco-u1i`)

The second `error`:

```text
breaking test_the_gate_can_go_red_twelve_ways tests/test_release_contract.py:78 — public item removed —
    fn test_the_gate_can_go_red_twelve_ways was last seen at tests/test_release_contract.py:78
    (class: breaking, access: Symbol)
```

A pytest function is not a public surface: nothing outside its file calls it, and renaming one is the ordinary
business of test maintenance (it changed in `reco-klr`'s "make master green" commit, which is the commit the
range covers). The check is reading `tests/**` as an exported package. The cost of not deciding this is that
every consumer who gates on `api` starts suppressing `api`, which is how a good check dies.

### 2.3 The artefact refused to certify itself (`reco-c2i`)

```console
$ ./target/debug/fathom export --episodes HEAD~5..HEAD --format markdown --out self-review.md
… self-check: UNVERIFIED — 4 problem(s), listed in the artefact
the artefact's self-check failed: checklist item finding:generated_edit:tools/gen_docs.py appears twice |
  checklist item finding:untested_new_logic:crates/fathom-cli/src/verbs.rs appears twice | …
```

The range is five episodes, and the same detector fires on the same file in more than one of them, so the
checklist id `finding:<id>:<path>` collides and the record says it cannot vouch for itself. This is the
document that travels to a pull request, and `fathom export --episodes HEAD` (one episode) is clean — the
defect is specifically the multi-episode case that a release-range review is exactly about. The export still
produced a 205 KB artefact with the verdict printed inside it, which is the right failure style; the job is to
make the ids unique per episode.

### 2.4 A blocked check where the policy actually lives elsewhere (`reco-dg7`)

`layering` is the check this repository most wants to be true, and it reports *blocked*: no `[layering]` rules
are declared, so nothing is enforced and the architecture scorecard prints its boundaries dimension as
unwritten. The real policy exists — ADR-002 §4's crate boundary rules, enforced by
`python3 tools/check_workspace.py lint` on every commit — but the tool cannot see a Python checker. Declaring
`[layers]`/`[layering]` to match the ADR is a short edit; the ticket is that nobody has decided to.

### 2.5 The repository has never chosen its gate mode (`reco-azy`)

`policy.notices` says it plainly: *"`fathom.toml` declares no `[check]`, so the gate runs on its built-in
defaults: mode `warn`, fail at `warning`…"* — so a run that reports 16 findings and two errors exits **0**.
Correct behaviour, and an unanswered question: this project's own CI has never blocked on its own gate. That is
a decision to write down, and W43-08 has it.

### 2.6 Two numbers that are the tool being confidently wrong about itself (`reco-3sa`, `reco-0er`)

* `diff.per_file`: 3339 lookups, 5 hits, 3334 puts, **0.1 %** — the durable cache is filled by one run and
  almost never read by the next, and the eviction counters say the budget has never bitten, so it is a key or a
  reuse story. `ingest.normalize` at 72.1 % proves the machinery works when the key matches.
* Attribution: 102 of 109 episodes credited to a `human` at confidence 0.50, in a repository where a coding
  agent wrote them and the trailer vocabulary of ADR-005 is not stamped by anything here. Consequence:
  `authorship` contributes `0.00` on the risk vector, and will keep doing so in every repository whose harness
  does not co-operate, while looking like a measured zero.

### 2.7 Three stale sentences, and one unusable identifier (`reco-j1o`, `reco-gna`, `reco-dvu`, `reco-pfb`)

* `fathom why`'s "what `features-1` cannot see" still says `blast` is absent *because* "the module graph is
  `e3.1`'s and the graph delta is `e3.2`'s" — both closed, and `fathom architecture` measured the graph on this
  very index minutes later. The remedy text is remembered rather than derived, which is the argument
  `tools/check_docs_commands.py` already makes about documented commands.
* Every `fathom why` run ends with `config: risk.weights.new_dependency: new_dependency is not a feature of
  this vector`. This repository's own `fathom.toml:157` carries that key. Honest notice, wrong state: a key
  that configures nothing is a decision in the wrong drawer.
* The `sign-off` finding names `ep-5e23bd8e8`; `fathom why ep-5e23bd8e8` answers `cannot resolve` — the real id
  is `ep-5e23bd8e8abc36d76f25`. A finding whose subject cannot be opened by the verb it tells you to run is a
  dead end at the worst moment.
* `debug_leftover` fired on `tools/demo_exhibits.py`, `tools/epic6_acceptance.py` and four more — files whose
  entire job is printing a report — and the range's artefact listed **71 open actions**. A detector that fires
  on the house style is a detector whose real findings get skimmed.

## 3 The loop acts, not just the reading

The pass is not complete until the product's own verbs were used the way the README tells a reviewer to:

```console
$ ./target/debug/fathom review sign ep-5e23bd8e8abc36d76f25 --decision accepted --note "…features-1 72 is size
  (20.9) plus a 15 % test ratio (18.7) … the msi and the Homebrew formula are built but nothing here can say
  they are right."
signed    ep-5e23bd8e8abc36d76f25  accepted · looprs · 1 commit(s), 85 file(s)
$ ./target/debug/fathom review sign ep-f5c2935a8d202c468a38 --decision accepted --note "…the cycle is a module
  and its own submodule; accepted as documented debt rather than baselined, because the honest expiry is 'when
  the cycle detector learns that a module and its directory are one node'."
$ ./target/debug/fathom review status
ledger    1 file(s) · 3 entries · 3 fresh · 0 stale · 0 orphan · 0 tampered
episodes  109 live · 3 with a fresh sign-off · 106 without
verdict   NOT ALL REVIEWED
```

`review status` also blocks with *"`.fathom/reviews/ledger.md` has changes that are not committed"*, which is
the point of ADR-004 §12 and the reason this pass ends with the ledger committed rather than with a note.
Signing the packaging episode was the useful one: the risk receipt is `size 20.94 + test_ratio 18.68`, and the
note is the argument for why a 15 % test ratio on a change whose logic ships four negative-control self-tests
is a decision and not an oversight.

Two observations recorded and deliberately **not** filed, each because the remedy is already owned elsewhere:

* `fathom episodes --state` still reported `unread 109` immediately after both sign-offs, while
  `fathom review status` reported three fresh sign-offs. The store is a mirror of the ledger and reconciles on
  the next `fathom index` (the re-index this pass started had not finished when the pass ended, so it is
  recorded as *observed, mechanism by design* rather than as a defect). Practical consequence for the loop:
  **`review status` is the sign-off oracle, not `episodes --state`**, and any CI step that gates on review
  coverage must use the former.
* The queue's `spine-1 75` and `why`'s `features-1 72` on the same episode, against a flag τ of 40: already
  measured, argued and left divergent in `.plan/beta-gate-reco-69u.md` §1.4's addendum and
  `queue::order::SEVERITY_HIGH`. W43-21 dispositions it as `triage:by-design` rather than reopening it.

One more, filed, because it is about this loop rather than about a screen: re-running the red list recorded in
`.plan/beta-gate-reco-69u.md` to check it before citing it here found that **the recorded list is stale**. The two
`test_risk_ranking_contract.py` tests it names are green; three other tests in that file are red, all three
failing the same way (the TUI driven over a pty returns an empty queue screen on the 500-commit fixture), and all
three verified red on a clean tree under `git stash -u`. Filed as `reco-2vq`, with the note that a known-red list
nobody re-reads is worse than none, because the next reader will wave the current reds past as the recorded ones.

## 4 Why this is worth repeating, and what would make it a regression test

Three of the eight findings above could only have been found by running the binary over a repository where the
author is also the reporter — the cycle that is a directory, the test reported as an API, the export that could
not certify itself. Two of them are `error` severity on the repository's own newest commit, in a repository
whose own CI has never blocked on the gate, which is the honest answer to "does the gate work?" before the
answer to "does it work for you?".

The loop makes it repeatable in the only way that counts: `self-review.yml` runs it at every release and
attaches the artefact to it, so a release that changes what Fathom says about Fathom has to explain the
difference in its own release notes; the weekly act in
[docs/concepts/feedback.md](../../../docs/concepts/feedback.md) §3–§6 puts the same pass in the triage rota
between the external reports; and [`.plan/intake/`](../../intake/) is the register either way. The next pass
should be boring: fewer uncovered cycles, a `diff.per_file` hit rate worth a sentence, and a review-status line
that says more than three episodes reviewed. If it is not boring, the diff is the news, and it is now a diff
someone can read against this one.
