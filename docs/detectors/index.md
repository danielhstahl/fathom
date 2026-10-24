# Detector catalogue

- **Status**: every built-in detector, with a worked example, generated from the fixtures that prove it
  (`e7.5`, **reco-60m**).
- **Code**: [`crates/fathom-analyze/src/detectors/`](../../crates/fathom-analyze/src/detectors) — the
  `CATALOGUE` array is the registry and
  [`tests/fixtures/detectors/`](../../crates/fathom-analyze/tests/fixtures/detectors) is the proof.
- **The contract behind the catalogue**: [pathology detectors](../concepts/pathology-detectors.md)
  (what a detector is, what a finding must carry, why the content state is an input);
  [false-positive calibration](../concepts/false-positive-calibration.md) (what it costs to be wrong).
- **Regenerate**: `python3 tools/gen_docs.py write --page detectors`. The example blocks are the
  fixtures, re-rendered; CI refuses a page whose examples do not match the files that
  `cargo test -p fathom-analyze --test detector_fixtures` scores.

A **detector** is a pure function over one reviewable episode — plus, for three of them, the episodes
before it — that returns **findings**: things a coding agent characteristically did that a human should
look at. It is not a linter. A linter asks "is this legal?"; a detector asks "is this the shape of
someone working without a map?", which is a question about a trajectory rather than about a line.

Twelve detectors ship. Eight are on by default; four ship switched off because their false-positive rate
has not been measured on real repositories yet, and each one says so in its own words further down. That
asymmetry is deliberate and it is the product: a detector that cries wolf costs the reviewer's attention,
which is the only resource this tool has.

**Four things every finding carries**, enforced in
[`finding.rs`](../../crates/fathom-analyze/src/detectors/finding.rs) rather than by convention: a
one-sentence **reason** with a number in it, a **jump target** (view plus address), a **confidence** that
is never `1.0` for an inferred claim, and a **suppression key** — `detector:subject` — which is the whole
of what you type to make it stop. Examples of all four are in every worked example below.

**A detector that could not look says it could not look.** Whether the hunks were actually read is an
input (`content` is `full`, `partial` or `absent`), and a scan over an `absent` episode reports
`not examined` per detector with a line naming what would fill it — the failure ADR-001 §7.7 refuses is
an unknown printed as a zero.

---

## The catalogue at a glance

*Needs* is what the read has to carry: `rows` is the path rows and their taxonomy, `content` is the hunk
lines, `history` is the episodes before this one. A `rows`-only detector still answers when the hunks were
never read, which is why four of the eight defaults are cheap enough to run on a cold index.

<!-- BEGIN GENERATED: summary -->
| Detector | The question it asks | Needs | Ships | Version |
|---|---|---|---|---|
| [`scope_creep`](#scope_creep--change-outside-the-declared-scope) | did every touched path fall inside the scope somebody declared? | `rows` | **on** | `v1` |
| [`untested_new_logic`](#untested_new_logic--new-logic-with-no-test-surface) | is there new logic here that nothing in this episode runs? | `content` | **on** | `v1` |
| [`test_weakening`](#test_weakening--assertions-or-tests-removed-skipped-or-focused) | did the test surface get smaller, softer, or narrower in this episode? | `content` | **on** | `v1` |
| [`debug_leftover`](#debug_leftover--debug-output-committed) | did something written to look at the program survive into the commit? | `content` | **on** | `v1` |
| [`todo_debt`](#todo_debt--new-todofixme-debt) | how much explicitly unfinished work did this episode add? | `content` | **on** | `v1` |
| [`generated_edit`](#generated_edit--hand-edit-to-a-generated-file) | is a file a program owns being edited by hand? | `content` | **on** | `v1` |
| [`secret_string`](#secret_string--secret-shaped-string) | does a committed string have the shape of a credential? | `content` | **on** | `v1` |
| [`lint_escape`](#lint_escape--disabled-lint-or-type-check) | was a check switched off here, and which one? | `content` | **on** | `v1` |
| [`churn_loop`](#churn_loop--churn-loop-the-same-region-without-converging) | has this region been edited across N episodes and put back again? | `history` | off | `v1` |
| [`rework_loop`](#rework_loop--rework-loop-revert-or-repeated-hunk) | is this episode undoing, or re-landing, something that already happened? | `history` | off | `v1` |
| [`shotgun_edit`](#shotgun_edit--shotgun-edit-across-unrelated-clusters) | does one episode reach into several places that have nothing to do with each other? | `rows` | off | `v1` |
| [`copy_paste_growth`](#copy_paste_growth--copy-paste-growth) | did new text arrive as a copy of other new text? | `content` | off | `v1` |
<!-- END GENERATED: summary -->

## The four that ship off, and what each is waiting for

The `reenable` field is the detector's own gate, stored in the code (`Descriptor::reenable`) and printed
here rather than paraphrased. Four reasons, and none of them is "nobody got round to it": three of the four
wait on the same measurement — the three-repository disposition set behind
[Guardrail 3](../concepts/false-positive-calibration.md) — and `shotgun_edit` waits on something that does
not exist yet, the module graph of `e3.1`/`e3.2`, because today it is a path heuristic with a 0.45
confidence ceiling and it says so in every finding it would produce.

Turn one on deliberately:

```toml
# fathom.toml
[detectors]
enabled = ["churn_loop"]     # adds to the built-in set; it does not replace it
```

`detectors.enabled` **adds**; it does not replace the built-in set, so switching one on never switches another
off. An id that is not in the catalogue is **not refused** — not by the Rust build, not by the reference
implementation — even though `e0.8`'s key spec promises "a warning naming a valid id". The check is the number
`fathom why` prints: a run says `N of 12 detectors answered`, so a typo shows up as a count that did not go up
when you added a detector. Add `fathom why <episode> | grep answered` after editing this key, and delete the
key if a detector seems mute.

<!-- BEGIN GENERATED: disabled -->
| Detector | What it is waiting for, in its own words |
|---|---|
| [`churn_loop`](#churn_loop--churn-loop-the-same-region-without-converging) | e4.3a (reco-mkl.1) must show G3 ≤ 0.30 with miss_rate ≤ 0.20 on the three pinned OSS repositories; the threshold is ADR-001 §5.4's and the harness is not built yet |
| [`rework_loop`](#rework_loop--rework-loop-revert-or-repeated-hunk) | e4.3a (reco-mkl.1): a rebase, a squash-merge and a legitimate re-entry are all indistinguishable from a rework at line level, and only the three-repository disposition set says how often |
| [`shotgun_edit`](#shotgun_edit--shotgun-edit-across-unrelated-clusters) | e4.3a (reco-mkl.1) for the false-positive rate, and `e3.1`/`e3.2`'s module graph to replace directory clusters with real coupling — until then this is a path heuristic with a 0.45 ceiling |
| [`copy_paste_growth`](#copy_paste_growth--copy-paste-growth) | e4.3a (reco-mkl.1): tables, generated tests and parallel implementations of one interface all read as copied; the block-length knob is what the measurement tunes |
<!-- END GENERATED: disabled -->

## Knobs, and how to silence something

One table, from the schema the config layer itself publishes
([`schemas/fathom.schema.json`](../../schemas/fathom.schema.json), generated from
`src/recommender/config/schema.py` and diffed in CI). Every key layers like every other key in Fathom —
built-in, user, repository, environment, command line — and `explain` prints which layer won
([config §8](../concepts/config.md)).

<!-- BEGIN GENERATED: knobs -->
| Key | Default | Type | Read by | What it decides |
|---|---|---|---|---|
| `detectors.churn.min_episodes` | `3` | `integer` | churn_loop | Episodes that must have edited one region before `churn_loop` calls it a loop, and how far back it looks (four times this). Two episodes touching a function is a hard bug; four is a story. Ships disabled until `e4.3a` measures it. `e4.3`. |
| `detectors.copy.min_block_lines` | `12` | `integer` | copy_paste_growth | Consecutive new lines that have to match elsewhere for `copy_paste_growth` to call it a copied block. Ships disabled; this is the knob `e4.3a` tunes first, because a table-driven test and a duplicated idea are the same twelve lines. `e4.3`. |
| `detectors.disable` | `` | `array` | — | The off switch for a detector, as opposed to `detectors.suppress`'s off switch for a finding: an id here does not run, and its row in every report says `disabled` — so the question 'did it look?' still has an answer. Applied after `detectors.enabled`, so it wins a collision, and it is the same grammar as `--disable-detector` / `--detectors -id`. `e6.7`. |
| `detectors.enabled` | `test_deletion, untested_new_logic, debug_leftover` | `array` | every detector | Detectors that run. Unknown detector ids are a warning naming a valid id. |
| `detectors.max_findings` | `12` | `integer` | every detector | Findings kept per detector per episode. What the cap hides is counted in the report, never dropped silently. `e4.3`. |
| `detectors.rework.min_overlap` | `0.6` | `number` | rework_loop | Line-set overlap (Jaccard, both directions) that makes an episode a revert of an earlier one. Ships disabled: a rebase and a rework are the same bytes. `e4.3`. |
| `detectors.shotgun.min_clusters` | `4` | `integer` | shotgun_edit | Directory clusters (the first two path segments of each hand-written file) one episode may touch before `shotgun_edit` calls it spread. Ships disabled, and `e3.1`'s module graph is what will make it a real detector. `e4.3`. |
| `detectors.suppress` | `` | `array` | every detector | The off switch: a detector id, an exact suppression key, or a glob over keys. Keys are `detector:subject`, printed by every finding, and contain no episode id and no timestamp so a rebase cannot invalidate one. `e4.3`. |
| `detectors.todo.min_new` | `1` | `integer` | todo_debt | New `TODO`/`FIXME`/`todo!()` markers before `todo_debt` speaks. `0` is a real threshold — report the episode that removed one — so unlike a count, zero is not refused. `e4.3`. |
| `detectors.untested.min_added_lines` | `20` | `integer` | untested_new_logic | How big a new public surface has to be before `untested_new_logic` says it owes a test. Below it, a three-line accessor demanding a test is noise the reviewer mutes wholesale. `e4.3`. |
<!-- END GENERATED: knobs -->

Where the two sources disagree, this table prints the one that is checked harder. The schema's default
for `detectors.enabled` still names `test_deletion`, an id this build's catalogue does not contain; the
value above comes from
[`catalogue.json`](../../crates/fathom-analyze/tests/fixtures/detectors/catalogue.json), which is
blessed against the Rust `CATALOGUE` by `tests/detector_fixtures.rs`. That is a stale row in `e0.8`'s
key spec, not a hidden ninth detector — and a generated table is only worth having if the page says
where its numbers came from.

Three verbs of silence, cheapest first. Prefer the narrow one: a suppression that outlives the thing it
was written for is a hole with a name on it.

```toml
# 1. One finding. The key is printed with the finding and is stable across a rebase:
#    no episode id, no timestamp, derived from the subject and never from the message.
[detectors]
suppress = ["debug_leftover:src/parse/lexer.rs"]

# 2. One detector, everywhere in this repository.
suppress = ["secret_string"]

# 3. Fewer rows: a detector keeps its judgement but stops printing the tail, and says how much it dropped.
max_findings = 5
```

Keys are `detector:subject`, and `subject` is usually a path — `secret_string:src/config.rs`,
`test_weakening:tests/api.rs`. The reason a key survives a rewrite is ADR-003 §2.7: a review does not move
when the history does, and a key that had to be retyped after every rebase would simply be deleted instead.

The last line of the tuning story is the part people skip. Every detector in the catalogue is scored on a
fixture corpus in CI, and the corpus is small enough to read:

```bash
cargo test -p fathom-analyze --test detector_fixtures     # each fires on its fixture
python3 -m pytest tests/test_pathology_contract.py -q     # the docs, the JSON and the Rust agree
```

## The clean half of the corpus

Every detector must stay silent here, which is the half of the test suite that a detector that fires on
everything fails. A **hard bug written by a human** is in that set on purpose: "obviously bad" is not the
same as "agent-shaped", and a detector that reports a real bug is reporting something a reviewer would
have found anyway.

<!-- BEGIN GENERATED: clean -->
| Clean corpus | Episodes | Findings every detector must not produce |
|---|---|---|
| [`clean/`](../../crates/fathom-analyze/tests/fixtures/detectors/clean) | 3 | 0 |
<!-- END GENERATED: clean -->

---

## The detectors, one by one

Each section is: what it looks at, why it earns a place in a review of agent-written code, the fixture it
is scored on, and how to tune or silence it.

## `scope_creep` — change outside the declared scope

**What it looks at.** The paths this episode touched, against the scope somebody declared — a hook event's
declared scope, an `Agent-Scope:` trailer, or the plan item's own glob list. `rows` only: it never needs
the hunks, so it answers on a cold index.

**Why it earns its place.** With a human, a reviewer infers intent from the author: "Sarah knows this
module, she probably meant to touch it." With an agent there is no author to interrogate, so the declared
scope is the only statement of intent in the repository, and the gap between it and the diff is the
cheapest available signal that an episode is not what its title says. Scope creep is also how a small
contained task quietly becomes the change that touched the migration.

**Tune or silence.** No numeric knob — the scope comes from the record, not from a threshold. Silence one
path (`scope_creep:infra/dns.tf`) or the detector (`scope_creep`). If the noise is systematic, the fix is
usually upstream: the harness is not recording a scope, and `fathom hooks --check` says whether the events
carried one.

<!-- BEGIN GENERATED: example.scope_creep -->
*The input* (`fixtures/scope_creep.json`):

```text
ep-fix-scope-creep · agent author · 1 commit(s) · declared scope: src/api/**
M src/api/orders.rs  +1 −0
  @@ -1,4 +1,5 @@
  + pub fn total(&self) -> Money { self.lines.total() }
A db/migrations/0007_add_charge_column.sql  +2 −0
  @@ -1,4 +1,6 @@
  + ALTER TABLE charges ADD COLUMN refunded boolean;
  + COMMENT ON COLUMN charges.refunded IS 'money';
M src/billing/charge.rs  +1 −0
  @@ -1,4 +1,5 @@
  + pub fn refund(&mut self) { self.amount = Zero; }
```

*What the shipped detector reports* (`fixtures/scope_creep.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `high` | `0.9` | db/migrations/0007_add_charge_column.sql is outside the declared scope (src/api/**), and the file is new in this episode | `diff:ep-fix-scope-creep#db/migrations/0007_add_charge_column.sql` | `scope_creep:db/migrations/0007_add_charge_column.sql` |
| `medium` | `0.9` | src/billing/charge.rs is outside the declared scope (src/api/**), and it was modified here | `diff:ep-fix-scope-creep#src/billing/charge.rs` | `scope_creep:src/billing/charge.rs` |
<!-- END GENERATED: example.scope_creep -->

## `untested_new_logic` — new logic with no test surface

**What it looks at.** New logic lines in this episode, and whether anything in the same episode exercises
them: a test file, a test-named case, a `#[cfg(test)]` module. The floor is
`detectors.untested.min_added_lines` (default 20), because four new lines in a function that had four
lines is not a missing test surface.

**Why it earns its place.** An agent can produce logic faster than it produces tests, and the deficit is
invisible in a per-file diff review: every file looks reasonable, and the suite as a whole has stopped
describing the code. This is the one detector in the catalogue that is about the *shape over time* rather
than the line, and it is the one whose false negatives are the most expensive, which is why it is willing
to make noise at twenty lines.

**Tune or silence.** `detectors.untested.min_added_lines` is the knob; raise it on a repository where small
additive changes are the norm, lower it where a new public function is supposed to arrive with a test. The
suppression key is the file that owes the test.

<!-- BEGIN GENERATED: example.untested_new_logic -->
*The input* (`fixtures/untested_new_logic.json`):

```text
ep-fix-untested · agent author · 1 commit(s) · declared scope: src/**
M src/parser/grammar.rs  +25 −0
  @@ -1,4 +1,29 @@
  + pub fn parse_grammar(input: &str) -> Grammar {
  + let mut items = Vec::new();
  + while let Some(item) = self.next() {
  + items.push(item?);
  + }
  + Grammar { items, source: input.to_owned() }
```

*What the shipped detector reports* (`fixtures/untested_new_logic.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `medium` | `0.7` | src/parser/grammar.rs added 25 new line(s) including 5 public declaration(s) — pub fn parse_grammar(input: &str) -> Grammar {, pub struct Grammar {, pub fn walk(&self, visit: &mut dyn FnMut(&Item)) { (and 2 more) — and the episode touched no test file at all (0 test paths, 0 of them named for `grammar`) | `diff:ep-fix-untested#src/parser/grammar.rs` | `untested_new_logic:src/parser/grammar.rs` |
<!-- END GENERATED: example.untested_new_logic -->

## `test_weakening` — assertions or tests removed, skipped or focused

**What it looks at.** The test surface getting smaller, softer, or narrower inside one episode: a test
deleted, a case skipped or un-skipped, a focus left on, assertions replaced with weaker ones. It argues
about content, not about file names, so a rename that carries its assertions through is not a finding.

**Why it earns its place.** This is the highest-value detector in the catalogue for agent-written code,
because it names the agent's cheapest path to green: change the test. A deleted assertion is two lines in a
400-line diff, it is never the biggest number on the screen, and it is the single change most likely to
turn a passing suite into a decoration. The tool's whole claim is that a human should not have to hunt for
this by eye.

**Tune or silence.** No numeric knob on purpose — "how much weakening is enough" has no honest answer, and a
knob would just move the detector off-by-default. Silence per test file when a suite is being legitimately
retired, and say why in the `fathom review dismiss` reason, because
[the ledger](../concepts/review-ledger.md) keeps the reason.

<!-- BEGIN GENERATED: example.test_weakening -->
*The input* (`fixtures/test_weakening.json`):

```text
ep-fix-test-weakening · agent author · 1 commit(s) · declared scope: src/**, tests/**
M tests/api.rs  +1 −12
  @@ -1,4 +1,-7 @@
  - assert_eq!(order.total(), 0);
  - assert_eq!(order.total(), 1);
  - assert_eq!(order.total(), 2);
  - assert_eq!(order.total(), 3);
  + fn check_all(x: &[Order]) { assert!(true); }
```

*What the shipped detector reports* (`fixtures/test_weakening.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `medium` | `0.7` | tests/api.rs was weakened: 12 assertion line(s) removed and 1 added; the episode's own test totals were 0 cases and -11 assertion lines, so the loss did not move elsewhere in it | `diff:ep-fix-test-weakening#tests/api.rs` | `test_weakening:tests/api.rs` |
<!-- END GENERATED: example.test_weakening -->

## `debug_leftover` — debug output committed

**What it looks at.** Lines written to look at the program that survived into the commit: `dbg!`,
`console.log`, `print(`, `fmt.Println` of a variable, and the rest of the marker list in
[`debug_leftover.rs`](../../crates/fathom-analyze/src/detectors/debug_leftover.rs). The literal it saw is
printed in the finding, which is what makes a false positive arguable rather than mysterious.

**Why it earns its place.** It is the signature of a loop: the agent printed a value to find out what was
happening, the run went the way it wanted, and the print stayed. Individually harmless, collectively it is
how a codebase ends up with forty `println!`s in the hot path and no author to ask. It is also the cheapest
possible reminder that the diff was written by something that was not going to come back and clean up.

**Tune or silence.** The suppression key is the file. A logger call in a logging module reads as a leftover;
suppress that file rather than the detector, and if the pattern is language-specific enough to keep biting,
that is a bug report about the marker list, not a threshold to tune.

<!-- BEGIN GENERATED: example.debug_leftover -->
*The input* (`fixtures/debug_leftover.json`):

```text
ep-fix-debug-leftover · agent author · 1 commit(s) · declared scope: src/**
M src/parse/lexer.rs  +3 −0
  @@ -1,4 +1,7 @@
  + let token = self.next();
  + dbg!(&token);
  + self.position += 1;
```

*What the shipped detector reports* (`fixtures/debug_leftover.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `medium` | `0.8` | src/parse/lexer.rs added 1 debug output line(s) that read as left behind: dbg!(&token); | `diff:ep-fix-debug-leftover#src/parse/lexer.rs` | `debug_leftover:src/parse/lexer.rs` |
<!-- END GENERATED: example.debug_leftover -->

## `todo_debt` — new TODO/FIXME debt

**What it looks at.** New `TODO`/`FIXME`/`XXX` markers this episode added, counted, with the text quoted.
`detectors.todo.min_new` (default 1) is the floor.

**Why it earns its place.** A TODO from a human is a promise the author is still standing next to. A TODO
from an agent is a note left in a room the author will never re-enter, and the episode is usually the last
time anything looks at that region. Counting them per episode is what turns "the code has 200 TODOs" — a
fact nobody acts on — into "this turn added six", which is actionable.

**Tune or silence.** Raise `detectors.todo.min_new` in a repository where a stubbed-out interface is a
legitimate landing. Suppression is per file; note that a TODO with a ticket reference in it is still a
finding, on purpose — the reference is what makes the dismissal cheap to check later.

<!-- BEGIN GENERATED: example.todo_debt -->
*The input* (`fixtures/todo_debt.json`):

```text
ep-fix-todo-debt · agent author · 1 commit(s) · declared scope: src/**
M src/api/retry.rs  +4 −0
  @@ -1,4 +1,8 @@
  + // TODO: back off exponentially, not flat
  + fn retry(&self) -> Result<()> {
  + todo!()
  + }
```

*What the shipped detector reports* (`fixtures/todo_debt.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `low` | `0.85` | src/api/retry.rs added 2 unfinished-work marker(s) (rust): // TODO: back off exponentially, not flat, todo!() | `diff:ep-fix-todo-debt#src/api/retry.rs` | `todo_debt:src/api/retry.rs` |
<!-- END GENERATED: example.todo_debt -->

## `generated_edit` — hand-edit to a generated file

**What it looks at.** A file a program owns being edited by hand: generated markers in the header
(`Code generated by … DO NOT EDIT`), snapshot and lockfile shapes, and the content evidence the taxonomy
already carries. The taxonomy decides what a path *is*; this detector asks whether somebody typed into it
([paths and noise policy](../concepts/paths.md)).

**Why it earns its place.** The edit will be lost — that is the whole finding. Agents are more likely to
make this mistake than people are, because the fastest local fix is the one in front of the failure, and a
generated client is right there in the tree. Telling the reviewer "you just edited a file that the next
generator run overwrites" is worth one minute of their attention every time.

**Tune or silence.** No threshold; silence per file when a vendored file is genuinely hand-maintained in
your tree (and say so in the ledger), or fix the path rules if the file should not have counted as
generated. The rules that decide what counts as generated are keys in `fathom.toml`
([paths and noise policy](../concepts/paths.md)), and `explain` names the rule that won
([config §8](../concepts/config.md)).

<!-- BEGIN GENERATED: example.generated_edit -->
*The input* (`fixtures/generated_edit.json`):

```text
ep-fix-generated-edit · agent author · 1 commit(s) · declared scope: src/**
M src/generated/api_client.rs  +1 −0
  @@ -1,4 +1,5 @@
  + request.headers.insert("x-track", self.track.clone());
```

*What the shipped detector reports* (`fixtures/generated_edit.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `medium` | `0.65` | src/generated/api_client.rs is a generated file (the index classes it generated), and 1 hand-written line(s) were added to it | `diff:ep-fix-generated-edit#src/generated/api_client.rs` | `generated_edit:src/generated/api_client.rs` |
<!-- END GENERATED: example.generated_edit -->

## `secret_string` — secret-shaped string

**What it looks at.** Committed strings that have the shape of a credential: a known prefix, high entropy,
a `secret`/`token`/`key` name beside the value, and the decoys it knows are not secrets (a public key, a
placeholder, a test fixture that says it is a fixture).

**Why it earns its place.** Agents paste a value from the environment into a file to make a test pass, and
then the test passes forever in a public history. Severity is `high` and the jump goes straight at the line.
This is the detector whose false positives are cheapest to forgive and whose false negatives are the most
expensive in the whole catalogue, which is the same asymmetry that makes it willing to say "this looks like
a token" about a fixture.

**Tune or silence.** Suppress the `detector:subject` pair for a fixture that is deliberately fake, and keep
the word "fixture" in its name so the finding stops firing on its own description. Never tune this one to
zero across a repository — the cost of one miss is a key rotation.

<!-- BEGIN GENERATED: example.secret_string -->
*The input* (`fixtures/secret_string.json`):

```text
ep-fix-secret-string · agent author · 1 commit(s) · declared scope: src/**
M src/config.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let stripe_key = "sk_live_51H8xQKZbT4n9rYq0Z3mW7c1Ld6Xe2Pf";
```

*What the shipped detector reports* (`fixtures/secret_string.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `high` | `0.85` | src/config.rs adds Stripe live key — masked sk…Pf (40 characters) | `diff:ep-fix-secret-string#src/config.rs` | `secret_string:src/config.rs` |
<!-- END GENERATED: example.secret_string -->

## `lint_escape` — disabled lint or type check

**What it looks at.** A check switched off inside the diff: `#[allow(...)]`, `#[rustfmt::skip]`,
`// nolint`, `// eslint-disable`, `# type: ignore`, `@SuppressWarnings`, and the file-wide forms of each.
It prints *which* check was switched off, which is the difference between a finding and a smell.

**Why it earns its place.** It is `test_weakening`'s sibling: the second-cheapest route to green is to
switch off the thing that was red. An agent will do it because the suppression is syntactically local and
consequences are not. A repository that started with an agent tends to accumulate these in one commit's
radius, and a reviewer who never sees them has no idea the guardrail is now off.

**Tune or silence.** No knob. Suppress per file where a generated or FFI boundary legitimately needs an
`allow` — the same place you would put the comment explaining why — or accept the finding and let
`fathom review sign` carry the note.

<!-- BEGIN GENERATED: example.lint_escape -->
*The input* (`fixtures/lint_escape.json`):

```text
ep-fix-lint-escape · agent author · 1 commit(s) · declared scope: src/**
M src/parse/lexer.rs  +2 −0
  @@ -1,4 +1,6 @@
  + #[allow(clippy::unwrap_used)]
  + fn peek(&self) -> char { self.buf[0].unwrap() }
```

*What the shipped detector reports* (`fixtures/lint_escape.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `high` | `0.9` | src/parse/lexer.rs adds 1 check-suppressing directive(s) — a lint — and the reason is not in the diff: #[allow(clippy::unwrap_used)] | `diff:ep-fix-lint-escape#src/parse/lexer.rs` | `lint_escape:src/parse/lexer.rs` |
<!-- END GENERATED: example.lint_escape -->

## `churn_loop` — churn loop: the same region without converging

**Status**: ships **off**. Its `reenable` gate is Guardrail 3 measured on three real repositories
([the harness](../concepts/false-positive-calibration.md)), which is what tells a "the agent thrashed here"
claim apart from "this is a hard function".

**What it looks at.** A region edited across N episodes and put back again: `detectors.churn.min_episodes`
(default 3) episodes touching one region, and a look-back window four times that wide. This is one of the
three detectors whose `needs` is `history`: it cannot answer from the episode alone, so an `absent` read
leaves it silent with a gap line rather than guessing.

**Why it is worth reading when you turn it on.** Two episodes touching a function is a hard bug; four is a
story about not understanding it. The churn loop is the detector that sees the agent flailing, and it is
also the one most likely to call a genuinely iterative piece of work a failure — which is exactly the
trade-off the corpus measurement is for.

**Tune or silence.** `detectors.churn.min_episodes` is both the threshold and the window, and raising it is
the conservative direction. While it is off, `enabled = ["churn_loop"]` is an explicit experiment: check the
findings against the diff for a week, and if you suppress more than you sign off, switch it back off and
say so on the ticket.

<!-- BEGIN GENERATED: example.churn_loop -->
*The input* (`fixtures/churn_loop.json`):

```text
ep-churn-3 · agent author · 1 commit(s)
M src/parse/item.rs  +1 −1
  @@ -10,3 +10,3 @@ fn parse_item(
  - let price = self.peek();
  + let price = self.consume();
```

The episodes before it (the history this detector argues from):

```text
ep-churn-1 · agent author · 1 commit(s)
ep-churn-2 · agent author · 1 commit(s)
```

*What the shipped detector reports* (`fixtures/churn_loop.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `medium` | `0.5` | src/parse/item.rs · fn parse_item( has been edited by 3 episodes (ep-churn-1, ep-churn-2, ep-churn-3) without converging: ep-churn-3 puts back 100% of the lines ep-churn-1 removed there | `diff:ep-churn-3#src/parse/item.rs` | `churn_loop:src/parse/item.rs` |
| `medium` | `0.75` | src/parse/item.rs puts ep-churn-2 back: it re-adds 100% of what it removed from this path and re-removes 100% of what it added there | `diff:ep-churn-3#src/parse/item.rs` | `rework_loop:src/parse/item.rs` |
<!-- END GENERATED: example.churn_loop -->

## `rework_loop` — rework loop: revert or repeated hunk

**Status**: ships **off**. Its gate is the same disposition set, and the reason is stated: a rebase, a
squash-merge and a legitimate re-entry are all indistinguishable from a rework at line level.

**What it looks at.** Whether this episode undoes, or re-lands, something that already happened — measured
by line overlap (`detectors.rework.min_overlap`, default 0.60) between this episode's hunks and an earlier
episode's.

**Why it is worth reading when you turn it on.** A revert is a fact a reviewer should never have to notice
by eye, and an agent that has lost its own history re-lands the same change twice — usually with a
different shape, so the diff does not look like the thing it replaced. The honest difficulty is that the
same arithmetic reads a rebase as a rework, and the fix for that is not a threshold.

**Tune or silence.** `detectors.rework.min_overlap`, with a higher number being the conservative direction.
Note that [the ratchet](../concepts/ratchet.md) and [the ledger](../concepts/review-ledger.md) already
capture the durable side of this argument — a signed-off piece of work that gets reopened is a `drift`
finding, and `drift` ships.

<!-- BEGIN GENERATED: example.rework_loop -->
*The input* (`fixtures/rework_loop.json`):

```text
ep-rework-2 · agent author · 1 commit(s)
M src/api/validate.rs  +0 −12
  @@ -1,4 +1,-8 @@
  - fn validate(order: &Order) -> Result<()> {
  - if order.total().is_negative() {
  - return Err(Error::Negative);
  - }
```

The episodes before it (the history this detector argues from):

```text
ep-rework-1 · agent author · 1 commit(s)
```

*What the shipped detector reports* (`fixtures/rework_loop.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `medium` | `0.75` | src/api/validate.rs puts ep-rework-1 back: it removes every one of the 10 line(s) ep-rework-1 added to this path (100% of them gone again) | `diff:ep-rework-2#src/api/validate.rs` | `rework_loop:src/api/validate.rs` |
<!-- END GENERATED: example.rework_loop -->

## `shotgun_edit` — shotgun edit across unrelated clusters

**Status**: ships **off**, and it is waiting on two things: the false-positive measurement, and the module
graph of `e3.1`/`e3.2` to replace directory clusters with real coupling. Until then it is a path heuristic
with a 0.45 confidence ceiling, printed as such.

**What it looks at.** One episode reaching into several places that have nothing to do with each other —
counted as directory clusters (`detectors.shotgun.min_clusters`, default 4).

**Why it is worth reading when you turn it on.** The shotgun edit is the shape of "the agent did the whole
codebase while it was in there", and it is the change most likely to pass review because every individual
hunk is obviously fine. It is also a review-cost story: five unrelated areas in one episode means five
different experts' judgement, and one reviewer's fifteen minutes is not that.

**Tune or silence.** `detectors.shotgun.min_clusters`. Treat its findings as a question about episode size
rather than a defect report — the usual remedy is a narrower episode boundary, which
[the segmentation model](../concepts/episode-segmentation.md) is the place to argue about.

<!-- BEGIN GENERATED: example.shotgun_edit -->
*The input* (`fixtures/shotgun_edit.json`):

```text
ep-fix-shotgun · agent author · 1 commit(s)
M src/parser/file1.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned1 = adjust();
M src/parser/file2.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned2 = adjust();
M src/api/file1.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned1 = adjust();
M src/api/file2.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned2 = adjust();
M crates/fathom-store/file1.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned1 = adjust();
M crates/fathom-store/file2.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned2 = adjust();
M tools/migrate/file1.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned1 = adjust();
M tools/migrate/file2.rs  +1 −0
  @@ -1,4 +1,5 @@
  + let tuned2 = adjust();
```

*What the shipped detector reports* (`fixtures/shotgun_edit.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `medium` | `0.45` | one episode reaches 4 separate clusters (crates/fathom-store, src/api, src/parser, tools/migrate), widest tools/migrate (2 file(s)) — a change this spread is either a cross-cutting fix or a hunt-and-patch, and a path list cannot tell them apart | `structure:ep-fix-shotgun` | `shotgun_edit:clusters` |
<!-- END GENERATED: example.shotgun_edit -->

## `copy_paste_growth` — copy-paste growth

**Status**: ships **off**. Its gate is the same corpus, and its own note says why: tables, generated tests
and parallel implementations of one interface all read as copied, and `detectors.copy.min_block_lines` is
the knob that measurement would tune first.

**What it looks at.** New text that arrived as a copy of other new text — consecutive lines
(`detectors.copy.min_block_lines`, default 12) repeated between the new additions.

**Why it is worth reading when you turn it on.** Duplication is how an agent pays for a fix twice: it solves
the second call site by copying the first, and the two then diverge under the next change. The failure mode
is not ugliness, it is a bug fixed in one copy and not the other, which is why the detector's idea of a
finding is "these arrived together and they are the same".

**Tune or silence.** `detectors.copy.min_block_lines`, and the honest range is wide: at 8 a parameterised
test is a finding, at 40 a real duplicated block is not. Raise it first, then look at what it stopped
reporting.

<!-- BEGIN GENERATED: example.copy_paste_growth -->
*The input* (`fixtures/copy_paste_growth.json`):

```text
ep-fix-copy-paste · agent author · 1 commit(s) · declared scope: src/**
M src/billing/ledger.rs  +24 −0
  @@ -1,4 +1,28 @@
  + let charge_0 = ledger.post(amount_0, currency);
  + let charge_1 = ledger.post(amount_1, currency);
  + let charge_2 = ledger.post(amount_2, currency);
  + let charge_3 = ledger.post(amount_3, currency);
  + let charge_4 = ledger.post(amount_4, currency);
  + let charge_5 = ledger.post(amount_5, currency);
```

*What the shipped detector reports* (`fixtures/copy_paste_growth.expected.json`, re-scored in CI):

| Severity | Confidence | Message | Jump | Suppress with |
|---|---|---|---|---|
| `low` | `0.5` | src/billing/ledger.rs: 24 of its 24 new line(s) sit in 1 repeated run of ≥12 consecutive lines (longest 24); the same text appears again in this file | `diff:ep-fix-copy-paste#src/billing/ledger.rs` | `copy_paste_growth:src/billing/ledger.rs` |
<!-- END GENERATED: example.copy_paste_growth -->

---

## Where a finding shows up

The same `Finding` value, four doors: the attention section of
[the episode card](../concepts/summary-card.md); the ranked queue, whose risk model consumes severities
([risk score](../concepts/risk-score.md)); the delta document's `findings` section, evidence pointers and
all ([the delta model](../concepts/delta-model.md)); and the export, where a finding travels with its
suppression key so the person reading a PR can act on it. A finding that arrives in a pane also carries a
jump target, which is the mechanism behind "every figure is a button".

## Adding a detector

Four edits, and the module's own documentation
([`detectors/mod.rs`](../../crates/fathom-analyze/src/detectors/mod.rs)) is the normative list:

1. one pure function `fn detect(&Episode, &Corpus, &Policy) -> Vec<Finding>`, no capabilities;
2. one row in `CATALOGUE` — id, version, title, the question it asks, `needs`, `default_on`, and `reenable`
   if it ships off;
3. one fixture under `tests/fixtures/detectors/fixtures/` that it **must** fire on, blessed by
   `tests/detector_fixtures.rs`;
4. silence on the clean corpus in the same directory.

No registry call at runtime, no `main` to touch, no surface to teach: the delta document, the card's
attention section and the queue pick up a new row because they iterate over the table. This catalogue
page, `catalogue.json` and the Rust `CATALOGUE` are then checked against each other
(`tests/test_pathology_contract.py`), so a new detector without a documentation entry is a red build rather
than a documentation debt.

---

**Next**: [risk score](../concepts/risk-score.md) for how severities become a ranking ·
[false-positive calibration](../concepts/false-positive-calibration.md) for what a detector's cost means ·
[the recipe for silencing a finding](../recipes/silence-a-finding.md).
