# ADR-007: cyclomatic complexity — what it means per language, and what it is allowed not to know

- **Status**: Accepted
- **Date**: 2026-11-08
- **Ticket / plan key**: `reco-l9bz.1` / `e8.1`
- **Implements**: [ADR-001](0001-product-thesis.md) §7 principle 7 (*confidence is part of the answer:
  `unknown` is a first-class value*), principle 2 (*never hide silently*), principle 9 (*explainable
  before clever*), principle 10 (*one reality*); the *delta* and *episode* nouns of §4.1 and the delta
  model's data contract in §5.5 of the
  [thesis](../concepts/thesis.md#7-principles-the-thesis-commits-us-to)
- **Constrains**: `reco-l9bz.2` (`e8.2`, the per-grammar counting pass and its corpus),
  `reco-l9bz.3` (`e8.3`, the store columns and their invalidation cost),
  `reco-l9bz.4` (`e8.4`, the index-time budget and degradation matrix),
  `reco-l9bz.5` (`e8.5`, `EpisodeDelta` carrying the figure),
  `reco-l9bz.6` (`e8.6`, the rail column, the card row and the diff-tree sort),
  `reco-l9bz.7` (`e8.7`, the risk axis and the G3 re-measure). None of them may define the metric,
  aggregate it, or name it differently: this document is the only place any of those are decided.
- **Evidence**: [what six tree-sitter grammars can actually tell `e3.1`](../concepts/structure-extraction.md)
  (spike `s1`'s verdict table, its recorded blind spots, and the normalisation rules `e3.1` shipped),
  [rename and copy pairing](../concepts/rename-pairing.md) (`e2.4`'s fact/guess tiers, the measured
  0.75 bar, and the rule that a grouping is never a fact),
  [ADR-003](0003-episodes.md) **X3** (an episode's diff is `parent(first_oid) … last_oid` following the
  first parent), [ADR-004](0004-storage.md) §5.2–§5.5 (the cache key and what each field is allowed to
  evict), §5.7 (the version-bump gate), §9.5 (migrations never migrate derived data), and a node-kind
  audit of the six pinned grammars run for this document (§11.1) whose method is in §11.1 and whose
  finding is that the shipped complexity proxy counts fourteen node kinds the pinned grammars never emit.
- **Executed by**: nothing yet — this is the ADR written before its code, the way ADR-002 … ADR-006
  were. The children named above are the execution; §14 lists the command each of them owes this page.

> **The rule this ADR exists to make unavoidable.**
> Complexity is a count of the decision points in a body a parser can read. Where the parser cannot read
> the body, the answer is *unknown* and prints `?` with a reason — never `0`, because `0` is a claim about
> the code and `?` is an honest claim about this build. A figure that is a count of what was read is a
> measurement; a figure that is the absence of a count is a fabrication, and the difference is the whole
> content of §7.

**Contents.** [1 Why now](#1-why-this-document-exists-before-the-extractor-does) ·
[2 Decisions](#2-decision-summary) · [3 The unit](#3-the-unit-what-gets-a-number) ·
[4 Decision points per grammar](#4-decision-points-per-grammar-the-six-shipped-lists) ·
[5 Aggregation](#5-aggregation-per-file-per-episode-and-which-figure-is-the-glance) ·
[6 Added, deleted, renamed](#6-added-deleted-renamed--and-the-pairing-a-rename-needs) ·
[7 Unknown, not zero](#7-unknown-not-zero-the-reason-codes) ·
[8 What is refused](#8-what-is-refused) ·
[9 Consequences for the store](#9-consequences-the-store-ticket-inherits) ·
[10 The one word](#10-the-one-word-every-surface-uses) ·
[11 Measured facts](#11-measured-facts) ·
[12 Alternatives](#12-alternatives-considered) ·
[13 Consequences](#13-consequences) ·
[14 Verification](#14-verification--the-commands-the-children-owe)

---

## 1. Why this document exists before the extractor does

Epic 8 says a reviewer can see how much an episode changed (`+1 204 −904`, already on every rail row)
but not how much harder it made the code. Six tickets are going to be written against that sentence:
one counts decision points in six grammars, one adds columns, one budgets reading file contents at
scale, one puts a figure in `EpisodeDelta`, one draws a column, one teaches the risk ranking to use it.
Every one of them needs the same four answers — what is a decision point, what is a function, which
revision is "before", and what does the tool say when it cannot count — and if they each answer them,
there will be six metrics wearing one word, which is the failure ADR-001 §7.10 was written to prevent
and which this repository has already paid for once (ADR-004 §5.2: nine meanings of "invalidate" and
2 686 rows that had missed as something else).

So this document decides, and the children implement. Three things were *measured* rather than opined
before writing it, and all three are re-runnable (§11):

1. **What the six pinned grammars can see**, taken from spike `s1`'s verdict table and `e3.1`'s shipped
   extractor rather than from a language specification — including the four languages whose
   declarations a program wrote and a parser cannot see.
2. **Which node kinds each grammar actually emits**, audited against the `node-types.json` of the exact
   crates `Cargo.lock` pins, plus real parses of probe files through the shipped grammar bindings.
   This is how §4's tables were written, and it is not a style choice: it found fourteen node kinds in
   the shipped complexity proxy that the pinned grammars never produce (§11.1), which means a list
   copied from a wiki would have been wrong in exactly the direction that prints a confident zero.
3. **What the pairing of a renamed file costs and how sure it is**, taken from `e2.4`'s shipped
   verdict — measured bars, not a similarity number someone likes.

One thing is deliberately *not* here: any claim about what a given complexity number means for code
quality. That is a consumer's decision and §8 refuses to make it.

---

## 2. Decision summary

| # | Decision | Why | § |
|---|---|---|---|
| D1 | **The unit is one named callable the structure extractor already names** — identity `(file, kind, dotted name)` from `e3.1`'s normalisation rules, kinds `fn` and `method` | The store, the graph and the diff tree already key on that identity; a second identity vocabulary is a second answer to "which function?" | §3.1 |
| D2 | **A figure is `V(G) = 1 + (decision points)`** over the unit's own subtree, and `1` is a measured answer, not an empty one | McCabe's number is the count of independent paths; a body with no decision points has one, and printing `?` there would be a lie of the opposite kind | §3.2 |
| D3 | **Nested scopes of the same body count in the enclosing unit**: closures, lambdas, blocks, comprehensions and anonymous functions are not units | They are not named, not keyed, and not diffable on their own; splitting them invents rows a reviewer cannot point at | §3.3 |
| D4 | **A `match`/`case`/`switch` arm is one decision point, whatever its pattern list holds** — three patterns in one arm is one branch, not three | A reviewer counts arms by eye; the by-eye check is the definition, and the disagreement with the strict-CFG reading is recorded rather than hidden | §3.4 |
| D5 | **A short-circuit operator is one decision point** (`&&`, `\|\|`, `??`, Ruby's `and`/`or`, Python's `and`/`or`); an early `return`/`break`/`continue`/`raise`/`throw` is **not** | A short-circuit is a conditional jump a reader traces; an unconditional exit is not a branch, and counting exits double-counts the branch that got there | §4 |
| D6 | **Six per-language decision-point lists, each naming node kinds the pinned grammar emits**, each versioned as one basis token (`mccabe-1`) | "Same metric, six languages" is only true if the six lists are written down and diffable; the token makes a stale figure identifiable instead of silently comparable | §4, §9 |
| D7 | **Per file: the sum and the maximum** of the units' figures. No mean | A sum answers "what does this file ask a reader to hold"; a maximum answers "what is the worst thing in it". A mean answers neither and hides its own denominator | §5.1 |
| D8 | **Per episode: `Δ = figure(head) − figure(base)`**, `base = parent(first_oid) … last_oid` per ADR-003 **X3**, first parent at a merge | It is the only "before" that is the same question the episode's own diff answers; a second base would make the complexity row disagree with the line-count row | §5.2 |
| D9 | **The glance number on a rail row is the delta**, signed; the absolute at head is a second figure on the card and is never alone on a row | An absolute is a property of a revision that many episodes share; a delta is a property of the row it is printed on | §5.3 |
| D10 | **Absence is measured; unreadability is not.** A path that did not exist at the base has a figure of `0` with reason `absent_revision`; a path that existed and could not be read has no figure | Conflating them makes an added file look free and a deleted one look like a gift | §6.1 |
| D11 | **A rename is paired only at `e2.4`'s fact tier**, or by a reviewer's recorded accept. A guess pairs nothing and forces `partial` with reason `rename_unpaired` | A wrong pairing silently turns a rewrite into `Δ 0`, which is the one error this metric cannot survive; `e2.4` already measured which pairings deserve the word "fact" | §6.3 |
| D12 | **Unknown prints `?` with a reason from one closed vocabulary**, extending `e3.1`'s `UnavailableReason` rather than inventing a second one (§7.1), and a figure known to be incomplete prints `~`, not a number | ADR-001 §7.7, and the `x / ~ / ?` glyphs the card already uses. Two vocabularies for "we did not read it" is one too many | §7 |
| D13 | **A scope a program wrote is a named unknown, not a surprise**: `e3.1`'s `synthetic::*` marks (`macro_definition`, `decorated`, `monkeypatch`, `dynamic_method`, `synthesised_accessors`, `build_constraint`, …) force the file to `partial` | Spike `s1` measured exactly four irreducible blind spots across the six grammars; carrying their names costs nothing and prevents a whole class of wrong claim | §7.3 |
| D14 | **Refused: a repository-wide absolute score, a per-author leaderboard, any threshold inside the metric, and any cross-language normalisation** | Those are the four shapes a primitive acquires when nobody says no; each one moves authority from the reviewer to the tool | §8 |
| D15 | **The figure is content-addressed and the basis travels with it**, so no cache-key field needs to move for this epic: `cache::ROW_SCHEMA_VERSION` (key field 9) does **not** move, and zero derived rows are evicted | ADR-004 §5.2 already fixed the field that means "the shape of a derived row"; a new column on the spine is not that | §9 |
| D16 | **The word is `complexity`.** Not `cx`, not `cyclo`, not "cognitive load". Every other quantity that prints that word is renamed by its owner or re-fed by this figure | One word, one meaning, or `fathom why` and the hotspots lane disagree in front of the reviewer | §10 |
| D17 | **Per-function figures are derived rows keyed by content; the spine stores per-file sums only** | A per-revision per-function table is `O(revisions × functions)`; keyed by `(path, blob)` it is `O(distinct content)`, and deduplication is free because the key is the content | §9.4 |

---

## 3. The unit: what gets a number

### 3.1 A named callable, using the identity the graph already uses

`e3.1` shipped a normalisation rule that spike `s1`'s corpus committed to: **a declaration's identity is
`(file, kind, dotted name)`**, members carry their container in their name, and *declarations inside a
function body are not symbols*. That rule is load-bearing here for one reason beyond tidiness: the store
rows, the graph nodes and the diff tree are all keyed by it, so a complexity figure attached to it can be
traced from the rail to the graph to the file without a translation table, and a translation table is a
second place to be wrong.

So: **one figure per unit, where a unit is a symbol the extractor emits with kind `fn` or `method`** —
including methods of a class, trait or impl block, and including a declaration the grammar sees with no
body, which is then an unknown (§7.2) rather than nothing. What is *not* a unit:

| Not a unit | Why | Where its decision points go |
|---|---|---|
| A function nested inside a function body (`fn` in `fn`, Python's `def` in a `def`, a TS `function` in a function) | Not a symbol under `e3.1`'s rule, so not nameable, not keyable, not diffable | Counted in the enclosing unit |
| A closure / lambda / block / anonymous function (`closure_expression`, `lambda`, `arrow_function`, `do_block`, `lambda_expression`, `func_literal`) | Same: no identity, and a move of four lines re-keys it | Counted in the enclosing unit |
| A comprehension, generator, or a Go `range` body | Same | Counted in the enclosing unit (its own `if` filter is a decision point — §4.2) |
| A module, class, struct, enum or interface body | It is a container, not a body of control flow | Its *own* figure is the sum over its units (§5.1), never a `V(G)` of its own |
| A field, record component, associated const | `e3.1`: "fields are not symbols anywhere" | Nowhere |

Two consequences worth stating because they will be argued:

* **A Rust trait method with no default body, a Java `abstract` method or interface method, a TypeScript
  overload signature and a `.d.ts` `declare function` are units with no readable body.** They are
  `unknown` with reason `body_not_visible` (`e3.1` found the same fact in the other direction with
  `ambient_declaration` in a `.d.ts` and called it a declaration, which it is — of a signature).
* **A generated accessor is not a unit at all**, because the file does not declare it: a Java `record`'s
  accessors, a `@dataclass`'s `__init__`, Ruby's `attr_accessor` list and Rust's `macro_rules!` output
  are `s1`'s irreducible blind spots. They are not missing units; they are units the source does not
  contain, and the scope carrying them is marked (§7.3).

### 3.2 The arithmetic

`V(G) = 1 + Σ (decision points inside the unit's subtree, per the language's list in §4)`.

* The base is **1 per unit**, always. A one-line function scores 1. This is a figure, not an unknown:
  a body with no branches has exactly one path and the number says so.
* A figure is a **non-negative integer with a minimum of 1**. `0` is never a unit's figure. Where a file
  sum is `0`, it is `0` because there were no units and the reason says `no_bodies`, or because the path
  did not exist and the reason says `absent_revision` (§6.1).
* **No normalisation, no weighting, no scaling.** The number is a count. A consumer that wants a score
  (risk, hotspots, the ratchet) multiplies it in *its* arithmetic and prints *its* model string; the
  primitive never carries a weight (§8.3).
* **The figure counts decision points, not nesting.** Nesting is a different property, already counted
  by `e3.1`'s proxy and used by the hotspot score. McCabe is depth-blind on purpose; pretending otherwise
  would make two printed numbers mean the same thing when they do not.

### 3.3 Nested scopes belong to the enclosing unit

A closure's `if` is the enclosing method's problem: it is the reviewer's problem at the same moment, and
it is the only reading that survives a refactor that moves the closure four lines. The node kinds
counted *through* rather than *as* units are `closure_expression` (Rust), `lambda`,
`generator_expression` and every comprehension (Python), `arrow_function` and `function_expression`
inside a call (TypeScript), `func_literal` (Go), `block`/`do_block`/`lambda` (Ruby),
`lambda_expression` and `method_reference` (Java). Decision points inside them are counted once, in the
unit that lexically contains them, and no figure is ever counted twice.

For a *method body that is a single expression containing a ternary* (a Rust `fn` ending in a `match`, a
Kotlin-style expression body) nothing changes: the subtree is the subtree.

### 3.4 An arm is one decision point, whatever it matches

The ticket asks the question every implementation has to answer anyway: is `match v { 1 | 2 => a, 3 => b,
_ => c }` 3 branches (one per arm) or 4 (one per pattern alternative, wildcard included)?

**It is three: one decision point per arm.** Reasons, in the order they matter:

1. **A reviewer can check it by eye.** Open the file and count arms. Every alternative reading requires a
   rule about wildcards (`=> a` is a path; is the implicit wildcard in a non-exhaustive Rust `match` a
   path?), about `|` inside nested patterns (`Some(A | B)`), and about Go's `case 1, 2:` versus Ruby's
   `when 1, 2 then` — a rule set a reviewer cannot re-derive from the source in front of them. When a
   printed number and the code disagree, the number stops being evidence (ADR-001 §7.9).
2. **It matches how the grammars model it, once you look** (§11.1): Rust puts `1 | 2` in one
   `or_pattern` inside one `match_arm`; Go puts both values in one `expression_case`; Java's arrow-form
   puts `case 1, 2 ->` in one `switch_rule` with one `switch_label`; Ruby puts both in one `when` with
   two `pattern:` children. Every one of the six grammars says *arm*, and none of them says *alternative*.
3. **The alternative is gameable in the wrong direction.** Splitting `1 | 2 | 3 | 4 => a` into four arms
   to dodge a threshold is a mechanical edit; under the arms rule it is also a mechanical *no-op*, which
   is what you want a metric to be when someone optimises against it.

The honest cost: under the arm reading, `V(G)` **understates path count** for a wide `match`, and it does
so by exactly `(alternatives − arms)`. This is recorded as a known property of basis `mccabe-1`, not a
defect: an understate that is documented and constant is auditable, and an overstate that depends on how
someone spelled a pattern is not. A consumer that needs path count over a pattern-heavy language can
count alternatives in its own arithmetic and must print which it counted.

Related rulings, all of the same shape, all of them by-eye checks:

| Construct | Counts | By-eye sentence |
|---|---|---|
| `if` / `else if` / `elif` chain of 4 tests | 3 (one per test; the terminal `else` is 0) | "count the conditions, not the `else`s" |
| A trailing `else` / `default` arm | 0 for the `else`, 1 if it is written as a `default` arm in a `switch`/`match` | "an arm you can point at is one; an `else` that only says *otherwise* is none" |
| `if x && y \|\| z` | 1 for the `if`, 2 for the operators | "count the conditions and count the short-circuits" |
| `let Some(x) = y else { return None };` (Rust) | 1 | "a `let … else` is an exit on a failed test: one" |
| `if let Some(x) = y {}` (Rust) | 1 for the `if`, not 2 | "`if let` is one test, even though the grammar spells it in two nodes" |
| A guard on an arm (`when x if x > 0`, `case _ if ok ->`) | +1 for the guard | "a guarded arm is two tests" |
| A bare `return`, `break`, `continue`, `raise`, `throw`, `?` | `?` counts 1 (Rust: it is an exit on a failed test); the others 0 | "an exit costs nothing; a test that leads to it costs one" |
| A comprehension's filter (`[x for x in xs if x]`) | +1 for the filter, +1 for the `for` | "a comprehension is a loop with a condition, exactly like the `for` loop it replaces" |

---

## 4. Decision points per grammar: the six shipped lists

Six lists, because six grammars model "a branch" six ways. Every node kind in these tables was checked
twice before it was written here: once against the `node-types.json` shipped by the exact crate version
`Cargo.lock` pins (§11.1 reproduces it), and once against a real parse through the shipped bindings —
`cargo run -p fathom-analyze --features langs-all --example s1_spike -- --sexp <language> <file>`.
That double check is not decoration. It found that fourteen node kinds currently counted by `e3.1`'s
complexity proxy do not exist in the pinned grammars at all (§11.1), so a list copied from a
language-reference page would have carried the same disease: **an entry that names a kind the grammar
never emits counts zero forever, silently, and prints as a confident low number.**

Hence the rule `e8.2` inherits and must gate:

> **C-KIND.** Every node kind named in an implementation's decision-point list must be present in the
> pinned grammar's `node-types.json`, and must be *produced by at least one file* in that language's
> complexity corpus. A kind that satisfies the first half and not the second is a bug in the list or in
> the corpus, and CI says so. This is what makes the §11.1 defect impossible to re-acquire.

Three rules apply to all six lists:

* **An operator is read as an operator.** `&&`, `||`, `??`, `and`, `or` are anonymous node kinds in every
  one of the six grammars; a rule that counts `binary_expression` counts comparisons too and inflates
  every figure by the number of `>`s in the file. The count reads the operator child's kind.
* **A clause that only says *otherwise* is not a decision point.** The `else` of an `if`, the `else` of a
  `try`, a bare `finally`/`ensure`. An entry in a written list of alternatives — a `match` arm, a
  `case`, a `when`, and yes, a written `default` — **is** one decision point, because the reader
  enumerated it.
* **An exit is not a branch.** `return`, `break`, `continue`, `raise`, `throw`, `retry`, `yield` count 0.
  The branch that led to the exit is already counted. The exceptions are the *tested* exits — Rust's `?`,
  TypeScript's `?.` and Ruby's `&.`, each of which is a "did it succeed?" test wearing an exit's clothes —
  and they count 1.

### 4.1 Rust — `tree-sitter-rust` 0.24.2

Units: `function_item`; `function_signature_item` is a unit with no body (reason `body_not_visible`). A
closure is `closure_expression` and is not a unit.

| Decision point | Node kind(s) | + |
|---|---|---|
| `if`, `else if`, `if let`, `if … let …` | `if_expression` | 1 each |
| `else { … }` | `else_clause` | 0 |
| `let … else` (a failed pattern exits) | `let_condition` belonging to a `let_declaration` | 1 |
| `let` inside `if`/`while` | `let_condition` inside `if_expression`/`while_expression` | 0 — the `if`/`while` already counted this test |
| each `match` arm, wildcard included | `match_arm` | 1 each |
| `match` itself, `match_block`, an arm's pattern list, `\|` alternatives, ranges, nested patterns | `match_expression`, `match_block`, `match_pattern`, `or_pattern`, `range_pattern` | 0 |
| a guard on an arm (`n if n > 5`) | the anonymous `if` child of a `match_arm` | 1 |
| loop of each shape | `for_expression`, `while_expression`, `loop_expression` | 1 each |
| short-circuit | `binary_expression` whose operator child is `&&` or `\|\|` | 1 each |
| `expr?` | `try_expression` | 1 |
| `unsafe`, `async`, `.await`, a `return`/`break`/`continue`, `panic!`/`assert!`/`dbg!` macro calls | `unsafe_block`, `await_expression`, `return_expression`, `break_expression`, `continue_expression`, `macro_invocation` | 0 |

Named blind spot carried forward: a `macro_rules!` (or an imported macro) that expands to control flow is
**not in the file**. A file with one in scope is `partial` with reason `macro_definition`; the figure it
does produce is a floor, and the card says *may have more*, never *has* `s1`'s word for it.

### 4.2 Python — `tree-sitter-python` 0.25.0

Units: `function_definition`, **unwrapped from `decorated_definition`** — `s1` measured that a walker
matching on `function_definition` alone loses every `@property`, `@classmethod` and `@dataclass`
method, and `e3.1` fixed it there; the complexity pass reads the same symbol list and inherits the fix.
`lambda` is not a unit.

| Decision point | Node kind(s) | + |
|---|---|---|
| `if`, `elif` (a 4-test chain is 3) | `if_statement`, `elif_clause` | 1 each |
| `else` (of `if`, `for`, `while`, `try`) | `else_clause` | 0 |
| `try` / `finally`, each `except` | `try_statement` 0, `finally_clause` 0, `except_clause` **1 each** | as stated |
| `match` / each `case` | `match_statement` 0, `case_clause` **1 each** | as stated |
| ternary `a if c else b` | `conditional_expression` | 1 |
| `and`, `or` | `boolean_operator` whose operator child is `and` or `or` | 1 each |
| `not`, a comparison, a walrus | `not_operator`, `comparison_operator`, `named_expression` | 0 |
| `for`, `while` | `for_statement`, `while_statement` | 1 each |
| comprehension / generator: its `for` and each filter | `for_in_clause` **1 each**, `if_clause` **1 each** (inside `list_comprehension`, `set_comprehension`, `dictionary_comprehension`, `generator_expression`) | 1 each |
| `raise`, `return`, `break`, `continue`, `yield`, `with`, `assert` | `raise_statement`, `return_statement`, `break_statement`, `continue_statement`, `yield`, `with_statement` | 0 |

Named blind spots carried forward: `decorated` (a decorator that rewrites or adds methods — `@dataclass`
writes `__init__`; `s1` found 2 of 2 missed and called it irreducible), `monkeypatch` (a method assigned
from outside the class body). A scope marked with either is `partial`.

### 4.3 TypeScript — `tree-sitter-typescript` 0.23.2

Units: `function_declaration`, `function_expression`, `method_definition`,
`generator_function_declaration`. Not units: `arrow_function` (counted in the enclosing unit). A
`method_signature`, an overload signature without a body, or a `.d.ts` `declare function` inside
`ambient_declaration` is a unit with no body (reason `body_not_visible`) — `s1`'s corpus deliberately
annotated an ambient declaration as *the* declaration of that symbol, and this ADR keeps that reading.

| Decision point | Node kind(s) | + |
|---|---|---|
| `if`, `else if` | `if_statement` | 1 each |
| `else` | `else_clause` | 0 |
| each `case`, and a written `default` | `switch_case`, `switch_default` | 1 each |
| ternary / optional chaining / nullish coalescing | `ternary_expression` **1**, `optional_chain` **1**, `binary_expression` with operator `??` **1** | 1 each |
| `&&`, `\|\|` | `binary_expression` whose operator child is `&&` or `\|\|` | 1 each |
| `for`, `for…in`, `for…of`, `while`, `do` | `for_statement`, `for_in_statement`, `while_statement`, `do_statement` | 1 each |
| `try`/`finally`, each `catch` | `try_statement` 0, `finally_clause` 0, `catch_clause` **1 each** | as stated |
| `throw`, `return`, `break`, `continue`, `switch` itself | `throw_statement`, `return_statement`, `break_statement`, `continue_statement`, `switch_statement` | 0 |

`s1` recorded no blind spot for TypeScript. A `.d.ts` merge is the one case to watch: an
`ambient_merge` mark means *another file adds to this surface*, which is a completeness caveat on the
file, not on the figure.

### 4.4 Go — `tree-sitter-go` 0.25.0

Units: `function_declaration`, `method_declaration`. Not units: `func_literal`. The `package` clause is
not a declaration (`e3.1b` says so for the graph and it says so here too).

| Decision point | Node kind(s) | + |
|---|---|---|
| `if`, `else if` (incl. `if v, ok := m[k]; ok`) | `if_statement` | 1 each |
| `else` | `else_clause` | 0 |
| each `case` of an expression switch, each `case` of a type switch, each `comm clause` of a `select`, each written `default` | `expression_case`, `type_case`, `communication_case`, `default_case` | 1 each |
| the `switch`, `switch type` and `select` headers themselves | `expression_switch_statement`, `type_switch_statement`, `select_statement` | 0 — the arms are the branches |
| the one loop, in all its forms (`for`, `for cond`, `for range`, `for {}`) | `for_statement` | 1 each (`for_clause`, `range_clause` add 0) |
| `&&`, `\|\|` | `binary_expression` whose operator child is `&&` or `\|\|` | 1 each |
| `defer`, `go`, `return`, `break`, `continue`, `fallthrough`, a labelled break | `defer_statement`, `go_statement`, `return_statement`, `break_statement`, `continue_statement`, `fallthrough_statement` | 0 |

Named blind spot carried forward: **`//go:build` pairs**. `init_linux.go` and `init_other.go` both
declare `metrics.Init` and exactly one is compiled; a parser does not run the build, so `e3.1b` marks the
scope `build_constraint` and this metric inherits the mark: both files are measured, neither may claim to
be the whole story, and a `partial` that names `build_constraint` is the correct output rather than a
failure to notice.

### 4.5 Ruby — `tree-sitter-ruby` 0.23.1

Units: `method`, `singleton_method`, and a `define_method` whose name is a literal — because `e3.1`
already emits that as a symbol and this ADR adds no second list of what a function is (`e3.1b`: a literal
`define_method(:perimeter)` *is* in the file; `define_method(n)` over a collection is not). `block`,
`do_block` and `lambda` are not units.

| Decision point | Node kind(s) | + |
|---|---|---|
| `if`, `elsif`, `unless`, and their statement modifiers | `if`, `elsif`, `unless`, `if_modifier`, `unless_modifier` | 1 each |
| `else` | `else` | 0 |
| `case` / each `when` | `case` 0, `when` **1 each** (a `when 1, 2 then` is one) | as stated |
| `case`/`in` / each `in` clause and its guard | `case_match` 0, `in_clause` **1 each**, `if_guard`/`unless_guard` **1 each** | as stated |
| ternary | `conditional` | 1 |
| `&&`, `\|\|`, `and`, `or`, and the safe-navigation call operator | `binary` whose operator child is `&&`, `\|\|`, `and` or `or`; the anonymous `&.` | 1 each |
| `while`, `until`, `for`, and their modifiers | `while`, `until`, `for`, `while_modifier`, `until_modifier` | 1 each |
| `rescue`, `rescue` modifier; `ensure`/`begin` | `rescue` **1 each**, `rescue_modifier` **1**, `ensure`/`begin` 0 | as stated |
| `return`, `break`, `next`, `retry`, `yield` | `return`, `break`, `next`, `retry`, `yield` | 0 |

Named blind spots, and the distinction `e3.1b` already drew: `s1` called all of these irreducible
(`macro_method` ×7, `struct_new` ×3, `define_method_dynamic` ×2, `class_eval`, `monkeypatch_or_data`,
`alias`), and `e3.1b` shipped marks for the subset that is genuinely not enumerable — `dynamic_method` for
`define_method` over a collection, `monkeypatch` for a `class_eval` on a class the file does not declare —
*because* a literal `define_method(:perimeter)`, an `attr_accessor` and a `Struct.new` turned out to be in
the file after all. This figure uses `e3.1b`'s shipped marks (§7.3), not `s1`'s tags: a literal is a unit
with a figure, a loop over names is a marked scope. Ruby is also the language `s1` measured an order of
magnitude slower than its neighbours, which is why `e3.1` already has a per-run scan budget (`scan_budget`,
named by its config key) and why the complexity pass does not get to add a second parse: §10 of
`structure-extraction.md` priced Ruby at 0.612 MiB/s, and the arithmetic there is the arithmetic `e8.4`
extends rather than re-derives.

### 4.6 Java — `tree-sitter-java` 0.23.5

Units: `method_declaration`, `constructor_declaration`, `compact_constructor_declaration`. An interface
method without a `default` body and an `abstract` method are units with no body
(reason `body_not_visible`). A `record`'s synthesised accessors are **not units** — `e3.1b` records them
as a `record_accessor` note and a `synthesised_accessors` mark on the record's own scope, and this metric
prints the same verdict instead of inventing a figure for a body that is not in the file.
`lambda_expression` and `method_reference` are not units; a lambda's decision points belong to the method
that contains it.

| Decision point | Node kind(s) | + |
|---|---|---|
| `if`, `else if` | `if_statement` | 1 each |
| `else` | `else` / `else_clause` | 0 |
| each classic `case` group, each arrow `case … ->` rule, each written `default` | `switch_block_statement_group`, `switch_rule`, and a `switch_rule` whose `switch_label` is `default` | 1 each |
| `for`, enhanced `for`, `while`, `do` | `for_statement`, `enhanced_for_statement`, `while_statement`, `do_statement` | 1 each |
| ternary | `ternary_expression` | 1 |
| `&&`, `\|\|` | `binary_expression` whose operator child is `&&` or `\|\|` | 1 each |
| `try`/`finally`/try-with-resources; each `catch` | `try_statement` 0, `finally_clause` 0, `try_with_resources_statement` 0, `catch_clause` **1 each** | as stated |
| a `when`/`guard` on a pattern-matching `case` | `guard`, `when` | 1 each |
| `instanceof` used as a condition, a pattern in a `case` label | `instanceof_expression`, `pattern` | 0 — the `if`/`case` that tests it is already counted |
| `throw`, `return`, `break`, `continue`, `yield`, a lambda, a method reference | `throw_statement`, `return_statement`, `break_statement`, `continue_statement`, `yield_statement`, `lambda_expression`, `method_reference` | 0 |

Named blind spot carried forward: `synthesised_accessors` (a `record Circle(String name, double r)`'s
getters — `s1`: 2 of 2 missed, irreducible). Java is also the language where `e3.1b` had to scope a guess
(the same-package scan); nothing about that guess touches this metric, and this ADR does not reopen it.

### 4.7 Worked examples

Two, written out, because "a reviewer can check it by eye" is a claim that has to survive being tried.

**Rust.**

```rust
fn classify(v: i32, flag: bool) -> String {          // base                       +1
    if v > 10 && flag {                              // if_expression,  &&          +2
        "a".to_string()
    } else if v > 5 || flag {                        // if_expression,  ||          +2
        "b".to_string()
    } else {                                         // else                        +0
        match v {                                    // the match itself            +0
            1 | 2 => "one-two",                      // arm (one, not two)          +1
            3 => "three",                            // arm                         +1
            _ => "many",                             // arm, wildcard included      +1
        }
        .to_string()
    }
}
```

`V(G) = 8`. A reader with the source in front of them counts: two `if`s, two short-circuits, three arms,
plus one. If they count something else, the figure is wrong and the corpus file that disagrees names the
rule that broke.

**Python.**

```python
def f(xs, ok):                          # base                                  +1
    if ok and len(xs) > 2:              # if_statement,  and                     +2
        return [x for x in xs if x]     # for_in_clause,  if_clause              +2
    elif ok or xs:                      # elif_clause,  or                       +2
        try:                            # try                                    +0
            return xs[0]
        except IndexError:              # except_clause                          +1
            return None
    return "y" if ok else "n"           # conditional_expression                 +1
```

`V(G) = 9`. The comprehension is the row a reviewer is most likely to miscount, and it is counted the way
the equivalent `for` loop would be — which is the test of whether a rule is principled or just written
down.

---

## 5. Aggregation: per file, per episode, and which figure is the glance

### 5.1 Per file: the sum and the maximum

For one file at one revision:

* **`sum`** = Σ `V(G)` over the units the pass found in the file. This is the figure that answers *how much
  does this file ask a reader to hold*, and it is the one that behaves correctly under a refactor that
  moves a function between two files: the totals move with the code, which is the property a
  per-revision census needs to have.
* **`max`** = the largest single `V(G)` in the file at that revision. This answers *what is the worst
  thing in here*, and it is what the diff tree sorts on next to the delta.

Both are stored. Nothing else is aggregated at file level:

* **No mean.** A mean's denominator is the number of functions, which is itself the thing under review;
  a file that gains a third of its complexity in one new 40-branch function can be *calmer* on a mean
  than it was. It also cannot be recovered from two files' means without their counts, so it composes
  badly — which is how a repository-wide average of means gets published and turns out to be meaningless.
* **`max` does not aggregate across files the way `sum` does.** A file's `max` is the max of its units;
  an episode's `max` is the max over its files' head figures — it is never a sum, and a roll-up that
  added maxima would be a number with no name.
* **A file with no units** (a `mod.rs` of re-exports, a constants module, a `.json`) has `sum = 0` with
  reason `no_bodies`. That zero is a measurement: the pass read the file and found no bodies.

The three states, at every level, are the three the card already uses (`insight::delta`: `x` measured,
`~` partial, `?` unavailable), and they mean the same thing here:

| State | Condition | Printed as |
|---|---|---|
| measured | every unit in scope was counted | the number |
| partial | at least one unit or file in scope was not counted, and at least one was | `~41` with the count: `~41 (3 unmeasured)` |
| unavailable | nothing in scope could be counted | `?` plus the reason sentence |

A partial figure is a **floor**, and says so: the number is the sum of what was read, and the count of
what was not is printed beside it, never folded into it. Adding the un-read files as zero is the specific
act ADR-001 §7.7 exists to forbid, and it is tempting precisely because it makes the column look complete.

### 5.2 Per episode: delta and absolute, against one base

The base is **not a decision this epic gets to make**, because the episode's line counts already made it:
ADR-003 **X3** fixes an episode's diff as `parent(first_oid) … last_oid`, following the first parent at a
merge. So:

* **`figure(base)`** — `sum` per touched path, each parsed at `parent(first_oid)`.
* **`figure(head)`** — `sum` per touched path, each parsed at `last_oid`.
* **`delta`** = Σ over touched paths of `figure(head, path) − figure(base, path)`.
* **`absolute`** = Σ over touched paths of `figure(head, path)`. It is *not* a repository figure and the
  word "absolute" in this document is always qualified by "at head, over the paths this episode touched"
  (§8.1).

Both are arithmetic over stored rows, and the store already contains everything needed
(§9.2): the per-commit row for the **first** commit of the episode carries `complexity_before` at exactly
the base, and the row for the **last** commit carries `complexity_after` at exactly the head. So the
episode figures are a join, and the sum of the per-commit deltas is the same number by telescoping —
which is stated as a *check* the store ticket asserts, not as the definition, because a definition that
depends on contiguity breaks on a gapped episode and nobody wants to discover that in the arithmetic.

A merge episode follows **X3**'s first-parent rule silently and correctly: the "before" is the merge's
first parent, which is the only "before" that answers *what did this merge bring in* — the question a
reviewer of a merge episode is asking. Second parents are not averaged, not unioned, not consulted.

### 5.3 The glance number is the delta

**On a rail row the figure is the signed episode delta.** `+7`, `−3`, `±0`, or `?`. The absolute at head
and the coverage are on the card. Why the delta and not the absolute, in order:

1. **The row is an episode, so the number has to be about the episode.** An absolute-at-head is a
   property of a revision: ten episodes that all touch `engine.rs` carry nearly the same absolute and
   differ from each other only in what they did. A rail of absolutes ranks files, and the rail is sorted
   by time or by attention, so the column would be noise that looks like signal.
2. **The delta is the only candidate the reviewer can act on**, which is the same argument the
   ratchet makes in its own domain ("not *is* this repository messy but *did this turn make it messier*").
   A reviewer can accept a `+7`, can ask an agent to undo a `+7`, and can sign off on it. There is no
   verb for `41`.
3. **An absolute in a per-episode column is the refused number wearing the primitive's clothes.** A
   reviewer who sees absolutes per row will add them up mentally down the rail, and the tool will have
   shipped a repository-wide score by omission (§8.1).
4. **The delta survives being wrong about the base in a way the absolute does not.** If one file of five
   could not be read at the base, the delta is `partial` and the figure still describes the four that
   could; an absolute at head silently includes the unreadable file's neighbours and prints a total whose
   meaning depends on which files happened to parse.

What the card then carries, as figures of one section (`e8.5`'s ticket names the plumbing):
**delta** first, **absolute at head** second, **coverage** third (`4 of 5 files measured`), and the
unknowns with their reason codes. The rail cell is the delta alone plus, when width allows, the coverage
in parentheses. The diff-tree sort is by per-file delta, with `max` as the secondary sort — the tree is
where the number is *proved*, so it shows the same quantity the rail glanced at rather than a second one.

Where the rail cannot fit the column, the overflow ladder decides (ADR-006 §5.2, and `e8.6` picks the
priority). This ADR constrains the choice with two sentences: the figure is allowed to be dropped before
the change-size cell is (the size cell is the one the ledger and the diff summary are stated in), and **a
dropped column is not allowed to look like an unknown figure** — the pane's legend has to say the column
is not drawn at this width, because `?` means "could not read this" and silence means "not drawn here",
and a reviewer who cannot tell those apart has been lied to by the layout rather than by the data
(ADR-001 §7.2).

### 5.4 Coverage is part of the figure, never a footnote

`coverage = measured_paths / touched_paths`, computed at the same time as the delta and carried with it
(both in the stored row and in the `Figure`'s `raw`). Two rules:

* **Coverage is printed with the number it describes**, on the card always and on the rail when it is
  under 100 %: `+7 (4/5)`. A figure whose reliability a reader cannot see is a figure they will
  eventually distrust wholesale, which is worse than the distrust being available at the source.
* **Zero coverage is not a zero delta.** If nothing was measurable, the figure is `?` with reason codes
  named, and the card's gap line names the owner. `±0` means "measured: it made no difference".

---

## 6. Added, deleted, renamed — and the pairing a rename needs

### 6.1 Absence is measured; unreadability is not

The line that decides everything in this section:

| Situation | `figure` | reason code | Prints |
|---|---|---|---|
| path added in the episode — nothing existed at the base | `0` | `absent_revision` | `+9` (the whole figure is new) |
| path deleted in the episode — nothing exists at head | `0` on the head side | `absent_revision` | `−9` (the whole figure leaves) |
| path exists at both sides and was parsed | the real sum | `''` | the number |
| path exists but was never read (binary, oversized, no blob, wrong grammar) | no figure | its §7.2 code | `?` |
| file was parsed but declares no bodies | `0` | `no_bodies` | `±0`, and the reason is available |

The first two rows are why `0` has to be available: a deleted 200-line matcher genuinely made the code
simpler, and refusing to say so would be the mirror-image lie of counting an unreadable file as simple.
The last row is the third meaning of zero, which is why the reason code travels with it: three different
zeros, three different claims, and only one of them is "nothing happened".

Deleted functions inside a surviving file subtract automatically — the file's `sum` at head simply does
not contain them. Nothing in the design needs to know that a function was deleted, which is a feature:
the arithmetic is the same whether the delete was one function or the whole file.

### 6.2 A new function is not a new file, and neither needs a rule

Because the file figure is a sum over units, an added function inside an existing file is already handled
by `figure(head) − figure(base)`. Function-level identity matters for one thing only: the per-function
dump (`e8.2`'s reviewer-readable output), where a function that appears at head and not at base prints as
`added` and one that vanishes prints as `removed`. A *renamed* function in a *surviving* file is
therefore two lines in the dump (one removed, one added) and **zero** in the episode figure if their
figures match, and this ADR deliberately accepts that: pairing at function level would need a similarity
score for bodies, and `s4` measured what body similarity does to a reviewer's belief (§6.4).

### 6.3 A renamed file: pair at the fact tier, and nowhere else

For a file rename the episode's delta is `figure(head, new_path) − figure(base, old_path)`. That needs a
pairing, and the pairing question has been asked, answered and measured once already — by `e2.4`, whose
verdict this ADR reuses rather than re-deriving (the ticket says so explicitly, and it is right to):

* **Fact tier**: the pairing `git diff -M` reports at or above `rename.fact_bar` (default 0.75, measured
  so that nothing untrue is asserted on `s4`'s corpus). **A fact pairs.** A reviewer can re-check it with
  `git log --follow`, which is `e2.4`'s whole test of what earns the word.
* **Reviewer-accepted**: `e2.4`'s accept makes a guess *asserted* and names who did it. **An accepted
  guess pairs**, on the reviewer's authority, and the row keeps saying so — a human decision is a
  legitimate reason to treat two paths as one function, as long as it is attributed and not laundered
  into git's.
* **Guess tier, untouched**: everything else `e2.4` will stand behind — a pairing git did not report, one
  below the bar, **and every grouping: a split, a fold, a copy** (`e2.4`'s gate `X1`: no grouping is ever
  a fact, because git asserts pairs, not shapes). **A guess pairs nothing.** The two paths are each
  measured on their own side, and the episode figure is `partial` with reason `rename_unpaired`, naming
  both paths and the tier.

(The accept/reject surface is `e2.4`'s: decisions live in `.fathom/rename-review.json`, bound to the bytes
they were made about, and the `rename` verb that edits that ledger is `e2.4`'s own unbuilt half — its page
says so. This figure reads the ledger either way: an accepted pairing is a recorded decision in the file,
and whether a human typed it or wrote it is not this metric's business.)

Why the hard line: an unpaired rename is not a small error, it is a *sign-flipping* one. A moved 400-line
file with two new branches reads, if the guess was right and unpaired, as `+2` of `+402` of complexity —
no, worse, as `+402` of which the reviewer cannot tell which half is the move. And if the guess was
wrong, pairing it silently turns an unrelated new file into a `Δ 0`, which is the one thing on this
screen that a reviewer will not check. A `partial` that names the guess is the correct answer to a
question this tool cannot settle alone, and the pairing ledger's accept is the decision that settles it.

The reciprocal refusal, stated because it is the tempting one: **no second similarity score for
complexity pairing.** `s4` ablated path similarity and found that adding it as a weight took wrong
pairings from 1 to 36 at the same bar; `e2.4` ships path similarity as display only, with no weight, for
that reason. The complexity pass reads `e2.4`'s verdict and adds no scoring of its own.

### 6.4 Splits, folds and copies

One file into three, three into one, or a copy that survives: `e2.4` represents all of these as one
grouping with a shape, never as a 1→1 rename, and gate `X1` keeps them out of the fact tier. So under
§6.3 they are never paired by the tool. What happens instead is arithmetic, and it is worth spelling out
so nobody "fixes" it:

* **Split** (`1→3`): unpaired, the base side keeps the source's figure and the head side sums three
  destinations, so the delta is the *net* change plus the doubling of whatever moved. That is a real
  inflation, and it is why a split's episode figure prints `partial` with reason `rename_unpaired` and
  names the grouping's shape (`1->3`) rather than printing a floor.
* **Fold** (`3→1`): the mirror image, and it can print *negative* for a pure reorganisation. Same reason
  code, same `partial`.
* **Copy** (source survives): the source keeps its figure and the copy adds its own, which is honest — a
  copy really does add the complexity of a second copy of the body to the repository. No reason code
  needed; the grouping is printed by the pairing card, and this figure's reason list is not the
  place the copy is explained.

---

## 7. Unknown, not zero: the reason codes

ADR-001 §7.7, in this metric's words: **a file this build could not read has no complexity, not complexity
zero.** The corollary that makes it implementable is that *unknown* is not one thing but three, and each
needs a different printed shape:

| What happened | Prints | Example |
|---|---|---|
| Counted everything in scope | `41` | a parsed Rust file |
| Counted some of it, and know what was missed | `~41 (3 unmeasured)` | one function's body was a macro |
| Counted nothing | `?` + a sentence | a 40 MB generated file |

`?` never stands alone: every unknown carries a reason code, and the code's sentence arrives with it,
because a bare `?` is a shrug and ADR-006 §10.3 refuses shrugs as remedies. The code travels with the
figure all the way into the store column (§9.1) and out again on the card, so the reason a reviewer reads
is the reason the pass recorded, not one a surface improvised.

### 7.1 One vocabulary, extending the shipped one

`e3.1` already ships a closed vocabulary for "this file has no structural model":
`structure::UnavailableReason`, whose variants each carry `as_str()` (the stable string, and therefore the
column value) and `sentence()` (the wording a surface prints, written in the library so the phrase and the
reason arrive together everywhere). **The complexity figure extends that enum; it does not start a second
one.** Two vocabularies for "we did not read it" is one too many, and the first divergence between them is
a card and a graph page disagreeing about the same file.

Seventeen codes in the vocabulary: **fifteen mean *no figure***, and two — `absent_revision`, `no_bodies` —
mean ***a figure of `0` that is a fact***. The ten strings on the last row are not codes: they are `e3.1`'s
scope marks, and §7.3 says what they force. Levels: `unit` = one function, `file` = one path at one
revision, `episode` = a roll-up, `scope` = a container the pass cannot enumerate.

| Code | Level | Produced by | Forces | Sentence a surface prints | Filled by |
|---|---|---|---|---|---|
| `language_unknown` | file | language detection (`e3.1`) | `?` | *complexity unavailable: not one of the grammars fathom reads* | nothing — it is a ceiling |
| `grammar_not_compiled` | file | the pass, naming its ADR-002 §6.5 feature | `?` | *complexity unavailable: this build was compiled without the `lang-x` grammar* | a build with the feature |
| `extractor_not_shipped` | file | the pass, for a seventh language before its extractor | `?` | *complexity unavailable: this language's extractor has not shipped yet* | the ticket that ships it |
| `too_large` | file | `extract.max_file_bytes` exceeded | `?` | *complexity unavailable: the file is larger than the {limit} byte extraction limit* | raise `extract.max_file_bytes`, then `fathom index` |
| `not_utf8` | file | the pass, after reading | `?` | *complexity unavailable: the file is not text* | nothing |
| `binary` | file | the spine: git says binary, contents never read (distinct from `not_utf8` — a different layer knows this one) | `?` | *complexity unavailable: git reports the file as binary* | nothing |
| `contents_not_kept` | file | the spine: no blob kept for that side (`e8.4`'s cap) | `?` on that side | *complexity unavailable: the file's contents were not kept at this revision* | `e8.4`'s retention knob + re-index |
| `parse_failed` | file | parse produced more error than code | `?` | *complexity unavailable: the parse reported {n} errors and returned no structure* | fixing the file |
| `parser` | file | the grammar could not be handed to the parser | `?` | *complexity unavailable: the parser could not be initialised* | a bug report |
| `scan_budget` | file | a per-run cap spent (Ruby's, or `e8.4`'s new ones), naming the key | `?` | *complexity unavailable: the per-run scan budget `{key}` was already spent; raise it or reorder `priority` if this file matters* | the named key, then re-index |
| `body_not_visible` | unit | the pass: a declaration with no body (trait/`abstract`/interface method, overload signature, `.d.ts` declare, `extern`) | that unit uncounted → file `~` | *{n} of {m} functions have no body in this file (declared elsewhere, e.g. a trait signature)* | nothing — the body is elsewhere |
| `node_budget` | unit | the walk hit `extract.complexity.max_nodes` before the body ended | that unit uncounted → file `~` | *{n} of {m} functions were too large to count within `extract.complexity.max_nodes`* | raise the bound, then re-index |
| `column_not_filled` | file | the store: this row predates the column (§9.6) | `?` | *complexity has not been measured for this revision yet — `fathom doctor` reports coverage; `fathom index` fills it* | `fathom index` |
| `rename_unpaired` | file pair | §6.3: a rename below the fact tier | episode `~` | *a rename was guessed, not asserted, so its before and after were not treated as one file (`{old}` → `{new}`, {tier})* | the pairing ledger's accept (`e2.4`) |
| `mixed_basis` | episode | §9.5: figures computed under different basis tokens | episode `~` | *the figures in this episode were counted under different rules ({list}); they are not comparable with each other* | a full re-index |
| `absent_revision` | file | §6.1: the path did not exist on that side | figure `0`, state measured | *(no sentence: it is a zero with a reason, not an unknown)* | — |
| `no_bodies` | file | parsed fine, declares no callable | figure `0`, state measured | *(none; available on `fathom show`)* | — |
| `macro_definition`, `macro_invocation`, `decorated`, `monkeypatch`, `dynamic_method`, `synthesised_accessors`, `build_constraint`, `wildcard_reexport`, `ambient_merge`, `unknown_shape` | scope | the pass, reusing `structure::synthetic::*` | file `~` (§7.3) | *a macro, decorator or the runtime may add or hide branches here — this figure may be a floor* | nothing — it is a property of the language |

The last row is not a new vocabulary either: those ten strings are `e3.1`'s shipped `synthetic::*`
constants, verbatim, and §7.3 is why the complexity figure uses them rather than coining parallel codes.

### 7.2 Discarded truncation flags are a bug, not an optimisation

`util::count_kinds(node, kinds, budget)` returns `(counted, truncated)`, and today the six
`complexity()` functions write `let (branches, _) = …` — the flag exists and is thrown away. A figure that
stopped counting halfway through a body is a floor, and a floor printed as an exact number is the one
silent undercount this ADR will not carry. **`e8.2` inherits: any unknown bound — node budget, byte cap,
per-run time budget — sets the state and the code, and a figure with a spent bound never prints bare.**

### 7.3 A scope a program wrote is a named floor, not a surprise

Spike `s1`'s headline finding was that every miss it recorded was a declaration a *program* wrote, and
that an extractor is allowed to say it cannot enumerate a scope as long as it never says nothing. This
metric inherits that whole stance verbatim, including its strings: a scope carrying a `synthetic::*` mark
means *its declaration list is incomplete by construction*, and the rule that rides on it is
`may_have_shrunk()`'s sibling — a surface may print **may have more**, and may not print a total as
though the file were closed.

The four blind spots the ticket names as "recorded, not surprising" are all in the table above, and each
is carried exactly where `s1` found it:

| Language | Blind spot, as `s1` recorded it | Consequence for this figure |
|---|---|---|
| Rust | `macro_definition` — "macro-generated items are invisible; a symbol absent from the parse is not a symbol absent from the crate" | the file is `~`; its figure is a floor |
| Python | `decorated_class` (a decorator rewrote the class body), `monkeypatch` | the decorated class's methods that the decorator wrote are not missed rows; they were never in the file — the class scope is `~` with `decorated` |
| Ruby | computed method names (`define_method` over a collection — `s1`: 4 of 4 missed), `class_eval` on a class the file does not declare | units that do not exist in the text are not counted and are not counted as missing: scope `~` with `dynamic_method` or `monkeypatch`. A **literal** `define_method(:perimeter)`, an `attr_accessor` and a `Struct.new` are *in* the file — `e3.1b` extracts them, so they are units with figures, not marks |
| Java | `record_accessor` — "accessors the compiler synthesises are not in the tree" | no unit, no figure, and the record's scope carries `synthesised_accessors`; a `~` on the file naming the count |

Two languages, Go and TypeScript, recorded **no** blind spot in `s1`. That is not a licence to print a
confident total for them either — a `.d.ts` merge and a `//go:build` pair are the two ways each can be
incomplete anyway, and both are already in the table.

### 7.4 Four stored representations, and only two of them feed arithmetic

| Stored shape | Meaning | Feeds a sum? | Prints |
|---|---|---|---|
| figure = *n*, reason = `''` | measured | yes | `n` |
| figure = `0`, reason ∈ {`absent_revision`, `no_bodies`} | a zero that is a fact | yes | `0`, with the reason on demand |
| figure = `NULL`, reason = a §7.1 code | not measurable, and it is known which | no | `?`, or counted into `~` |
| figure = `NULL`, reason = `''` | this build never looked (`column_not_filled`) | no | `?` + the coverage line |

Row four is why the reason column defaults to `''` rather than to a code: *"this index has not filled the
column"* and *"this file cannot be read"* are different claims, one of which is fixed by running
`fathom index` and the other of which is not, and a reviewer who is told the second when the first is true
will stop trusting the tool. It is also the row `fathom doctor` exists to count (§9.6).

---

## 8. What is refused

Five refusals, each of them a shape this primitive would take on if nobody said no. They are here because
the epic's children are the ones under pressure to grow them.

### 8.1 No repository-wide absolute score

No verb prints "this repository has complexity 12 480", and no stored figure is a total over all files at
all revisions. Reasons, in order of how much they cost:

* **It is not reviewable.** The whole product is a queue of episodes a human can hold; a scalar over a
  repository is a number no action of a reviewer's can confirm or refute, which makes it a recommendation
  rather than a measurement (ADR-001 §7.9).
* **It is not comparable.** The figure is a count over six different decision-point lists (§4). A total
  across languages is a sum of unlike units — Rust `match` arms plus Ruby `when` clauses plus Java
  `switch_rule`s — and printing it as one number implies a comparability §4 explicitly disclaims (§8.4).
* **It is gameable in exactly the ways a scalar cannot see.** Splitting one 20-branch function into four
  6-branch ones moves a repository total by about `+3` (the extra per-unit bases) while making every
  per-function figure look calm — so any per-function threshold is dodged by an edit that changed nothing
  about the reading load, and the episode that did it prints a small delta. A reviewer watching rows, not
  a total, sees the restructure; a total conceals it in the direction that flatters.
* **The consumer that wants a baseline already has a better-shaped tool.** The ratchet records what the
  repository already owed, with an owner and an expiry, and only alarms on new ones. That is what a
  repository-wide number is actually wanted for, and it is already built, with a review process attached.

The episode absolute at head (§5.2) is not this refusal: it is scoped to the paths one episode touched,
labelled as such, and never summed down a rail.

### 8.2 No per-author leaderboard

No figure is aggregated by author, no surface ranks people by it, and `e8.7`'s risk axis may not weight an
author's history. The thesis' user is *the human accountable for software an agent wrote* — the question
is whether this change is understood, not who is slow. Attribution exists to tell a reviewer whose
judgement to weigh on an attribution claim, and the privacy stance in
[privacy.md](../concepts/privacy.md) is the same one: individual productivity is not a fathom output.
Nothing in §6 or §9 lets a per-author roll-up be reconstructed from stored columns either: the columns are
per `(commit, path)`, and any leaderboard would be a new computation with a new owner and a new argument.

### 8.3 No threshold baked into the metric

No "10 is complex", no "15 fails", no severity, no traffic light, no colour that encodes a judgement, and
no default cap in the counting code that silently changes what is measured. A threshold is a *policy
decision about a consequence*, and consequences belong to owners who can print theirs:

| Consumer | What it may decide | Where it must print it |
|---|---|---|
| risk ranking (`e8.7`) | a cap for the complexity axis, its weight, and the model string that carries them | the printed vector string, exactly as `spine-1 …` and `hotspots-1 …` do today |
| the ratchet (`e3.10`) | a baseline and an expiry per entry | `.fathom/ratchet.toml`, committed and reviewable |
| `fathom check` | a score at or over which an episode wants a sign-off, and `--fail-on` escalation | its config key and its own output line naming the threshold, as it does today |
| hotspots | its axis knee (today `complexity 120`, on the *proxy* — §10.3) | its published knee, which `fathom architecture` prints |
| `fathom doctor` / `fathom stats` | what counts as low coverage worth warning about | the warning line itself, with the number in it |

The reason for the hard refusal is not tidiness. A threshold *inside* the metric becomes the metric's
meaning — every later surface inherits "complex" as though it were measured rather than chosen, and the
one place a threshold can be argued about (the consumer, with its consequence attached) stops being where
the argument happens.

### 8.4 No cross-language comparability, and no normalisation

There is no per-language divisor, no z-score, no "normalised complexity". A Rust `match` arm and a Ruby
`when` are not the same unit of reader-load, and any transform that pretends otherwise hides a modelling
opinion inside a formula. Cross-language comparison is allowed only where it is honest: a per-file figure,
printed with its language, in a list a reviewer sorts themselves; or a per-language distribution a
consumer computes, labels and owns.

### 8.5 No quality verdict, no gate owned by this epic

No surface may print "too complex", "unmaintainable", "needs refactoring", or a score that implies one,
from this figure alone. This is not a soft touch: it is the reason the hotspot lane's rule ("a hotspot is
not a bad module") can be reused here — a structural figure is *evidence about reading load*, and the
judgement belongs to a reviewer with the file open. Correspondingly, **no CI gate in this epic fails on a
complexity number**; `e8.7`'s G3 harness gates on *calibration* (does the axis earn its weight), which is
a claim about the model, not a claim about the code under review. If a later consumer wants a gate, it
buys one the way `fathom check` did: a named threshold, a named owner, and an escalation that says it
escalated.

### 8.6 No figure without a pointer to its evidence

The epic already states this constraint and this ADR binds it to a constructor: every printed complexity
figure is a `Figure` built through `Figure::new`/`Figure::unknown` with an `Evidence` — a JSON pointer
inside the delta document for figures the document can prove, an `external` command for a figure the
reviewer can re-check by typing something, or an `owner` for a standing gap. `EpisodeDelta::verify()`
walking those pointers is the executable half of this refusal, and it already exists.

### 8.7 No per-revision, per-function census

Refused for size, and the substitute is better: per-function figures are keyed by *content* (§9.4), so the
table is bounded by distinct code rather than by `revisions × functions`.

---

## 9. Consequences the store ticket inherits

`e8.3` names its own deliverables; this is the part it must not invent.

### 9.1 Columns, and the constraints that make `?` structural

Six columns on `git_commit_file` — the same table that already carries `n_add`, `n_del`, `is_binary` and
`oversized`, so a file's size facts and its complexity facts sit in one row and are read by one join —
added through the `ADDED_COLUMNS` probe list, which is the pattern ADR-004 §9.5 keeps out of migration
strings precisely because SQLite has no `ADD COLUMN IF NOT EXISTS`:

```
complexity_before        INTEGER,   -- sum of V(G) over the path's functions at the row's base side
complexity_before_reason TEXT NOT NULL DEFAULT '',
complexity_after         INTEGER,   -- … at the head side
complexity_after_reason  TEXT NOT NULL DEFAULT '',
complexity_max           INTEGER,   -- largest single V(G) on the head side
complexity_basis         TEXT NOT NULL DEFAULT '',   -- e.g. 'mccabe-1'
```

Two `CHECK`s, both satisfiable by every row an older fathom wrote (its three figure columns are `NULL` and
its three text columns are `''` — which is exactly what "this build never looked" means, and matches how
`intent_state = ''` is read), and both evaluated on every write:

```sql
CHECK (complexity_after IS NOT NULL OR complexity_after_reason <> '')
CHECK (complexity_before IS NOT NULL OR complexity_before_reason <> '')
```

That is the ticket's "the CHECK constraints are the ones it names": **a side with no figure always names
why**, so `?`-without-a-reason is not representable. There is deliberately **no `CHECK` enumerating the
reason vocabulary** — a closed `IN (…)` on a column whose vocabulary grows with each new refusal would
make adding a code a migration, and a migration is the expensive way to say something the enum already
says. The closed list lives in `structure::UnavailableReason`, and one contract test asserts that enum,
this table and §7.1's agree.

**As shipped (`e8.3`), each constraint carries two clauses this section does not spell out.** Both live in
`crates/fathom-store/src/episode.rs`'s `ADDED_COLUMNS`, and both are this section's own rules rather than
new ones:

* **A row an older fathom wrote must satisfy the constraint.** That is what this section says in words and
  what the SQL above does not do: such a row has `NULL` figures and `''` reasons, which is §7.4's fourth
  row, so the literal constraints would reject the very state this ADR spends a section on. Each constraint
  therefore begins `complexity_basis = ''`: a row with no basis carries no figures to defend, and the basis
  is already the store's marker for "some build that knew about these columns wrote this row".
* **A figure always names its basis** — §9.5's "every figure carries `complexity_basis`", which the escape
  hatch above would otherwise leave a door for: a figure written with `basis = ''` would read as a figure
  that shares a basis with everything, and §9.5 exists precisely to stop that being the default.

The stamping itself is bookkeeping rather than DDL. `SCHEMA_VERSION` moves to 12 and `MIGRATION_12` is an
empty batch — the probe list applies the columns whether or not a store reached 12 the ordinary way — but
`user_version` and `meta.schema_version` move with it (ADR-004 §9.1), so an older fathom refuses this store
instead of reading columns it cannot name. That is the whole cost of the change: a published
forward-compatibility rule rather than an accident of this migration, and §9.3 is the proof that no derived
row pays for it.

So each shipped constraint reads, for each side:

```sql
CHECK (   (complexity_basis = '' OR complexity_after IS NOT NULL OR complexity_after_reason <> '')
       AND (complexity_after IS NULL OR complexity_basis <> ''))
```

Two assertions on it, both executable: `complexity::the_constraints_refuse_a_silent_question_mark_and_an
_anonymous_figure` proves the schema refuses a `?` with no reason *and* a figure with no basis, and proves
the never-looked row is still writable — the last of those is a migration-compatibility test wearing a
short name, because an older store that cannot open is the failure this section's first paragraph is
about.

No `complexity_delta` column: it is `complexity_after − complexity_before` and both are in the row, and a
stored derived value is a value that can disagree with its inputs.

### 9.2 The episode roll-up is a join, not a column

`episode_member ⋈ git_commit_file` already answers "which paths, in which commits, for this episode" —
that is `Store::episode_paths`, one statement over a page of rails rather than one per episode, and the
complexity roll-up is the same statement with two more columns and an ordering:

* `figure(base, path)` = `complexity_before` on the **earliest** row for that path in the episode;
* `figure(head, path)` = `complexity_after` on the **latest** row for that path;
* `delta`, `absolute`, `coverage`, and the reason-code tally, all by aggregation over that join.

The store ticket exposes it as one read beside `n_add`/`n_del` so `e8.5` never re-walks files, and the
telescoping check (Σ per-commit deltas == the base-to-head delta) is asserted on the fixture rather than
assumed. Storing a per-episode roll-up column as well would be a second answer, and ADR-004 §4.7 exists to
keep this repository out of those.

### 9.3 Which cache-key field moves, and what it evicts

ADR-004 §5.2 lists nine key fields; the question `e8.3` must answer in a comment and a test is which of
them this change moves. The answer is:

| Field | Moves? | Evicts |
|---|---|---|
| 3 `analyzer_version` | **yes**, for the analyzer(s) whose code computes the figure | that analyzer's rows only, and — via `dep_digest`'s transitive closure (§5.4) — the analyzers that declare it as a dependency |
| 5 `config_digest` | **yes**, whenever a newly declared key (`extract.complexity.*`, `e8.4`'s caps) changes value | the analyzers that declare those keys — that is what "declared" buys (§5.5) |
| 6 `result_schema_version` | **no** | nothing: `e8.5` must land additive at the unchanged delta-document version, and `tools/check_delta_compat.py check` is the gate that proves it |
| 9 `row_schema_version` (`cache::ROW_SCHEMA_VERSION`) | **no — and this is the decision, not an oversight** | **nothing** |

Field 9 is the one the ticket's own wording worries about ("*schema_version is field 9 of every derived-row
cache key, so decide and test whether this evicts every derived row*"), and the answer is that after
`reco-3sa` it is **not** the store's migration counter any more — it is `cache::ROW_SCHEMA_VERSION`, the
highest migration that creates or alters `cache`, `blob` or `blob_ref`. New columns on `git_commit_file`
are neither of those, and the `ADDED_COLUMNS` probe list does not even bump `user_version`, so:

> **Complexity evicts zero derived rows.** Not "few", not "an acceptable amount": zero.

The consequences that follow, which belong in the same comment as the claim:

* **A store written by an older fathom keeps every cached analysis it has.** The new columns arrive as
  `NULL`, and the figure for existing revisions is absent, not stale — which is what §9.6's lazy fill is
  for and what makes "the migration is free" true rather than aspirational.
* **What *is* cold is exactly the new work**: `structure.complexity`'s own rows (cold on first use, since
  it is a new producer) and whatever declares it as a dependency. `fathom index --stats` prints the
  per-analyzer hit rates that make that visible rather than felt (§5.6).
* **`ROW_SCHEMA_VERSION` may not be bumped to make this ticket easier.** Bumping it costs every derived row
  of every store on the next open, which is the cost ADR-004 §5.2 was rewritten to stop paying. If
  someone needs to force a re-derive of the complexity rows, the lever is `analyzer_version` (field 3),
  and the CI gate of §5.7 is what makes using it cheap.
* **Per-file figures are content-addressed, so a file edited once is re-counted once.** Scope `path` is
  `sha256(path_hash ‖ blob_oid)` (§5.3): two commits with identical bytes share the row, and the
  episode-base lookup finds a cached figure rather than a re-parse. This is also what makes `e8.4`'s
  budget realistically satisfiable — a full re-index re-counts changed content, not every revision.

### 9.4 Per-function figures are derived rows, keyed by content

The spine stores per-file sums only (§9.1); the per-function dump `e8.2` owes a reviewer lives in the
analyzer cache under a `path`-scoped key, so it is stored once per `(path, blob)` rather than once per
commit, and deduplication is a property of the key rather than a job. The row carries, per function:
dotted name, span, figure, the decision points that earned it (kind + start line, so a figure can be
defended line by line), and its own reason code when it has no figure. `fathom show` and the diff tree
read it; nothing that ranks does.

### 9.5 The basis token, and what happens when §4 changes

Every figure carries `complexity_basis`, and `mccabe-1` is this document's table version: **any change to
any of §4's six lists, to §3's unit rule, or to §3.4's arm rule bumps it** (`mccabe-2`, …) in the same
commit as the change, alongside the analyzer-version bump that field 3 requires. Two rules ride on it:

* A figure is only comparable with a figure of the same basis. A `mixed_basis` episode — plausible during
  a rolling re-index, where the base side was counted under `mccabe-1` and the head side under
  `mccabe-2` — is `partial`, names both tokens, and contributes nothing to a sum.
* **No store migration re-counts anything.** Old figures keep their old basis and keep printing, labelled.
  ADR-004 D12 says a migration invalidates rather than migrates, and here even the invalidation is
  optional: the basis makes the old figure honest, which is the whole reason the token exists.

### 9.6 Coverage, and never printing a figure for a file that was never parsed

A half-filled column is a figure that lies quietly, so `fathom doctor` reports the new column's coverage
over the index (`rows with a figure / rows that could have one`, split by reason code), and the rule that
makes that meaningful is §7.4's fourth row: a `NULL`-with-`''` row prints `?` and the coverage line, and
the fill path is either the next index run over that revision or nothing. A lazy fill must be **observable
while it runs**: partial progress prints as `~ (filling: 61 % of rows)`, never as a confident number over
a third of the store.

---

## 10. The one word every surface uses

### 10.1 The word

**`complexity`.** Noun, lowercase in JSON and in labels, spelled in full. The figure is "the file's
complexity", "the episode's complexity delta". This is the word the rail, the card, the diff tree,
`fathom analyze --json`, `fathom export`, the MCP result, `docs/` and this ADR all use, and nothing else
uses it for anything else.

Why not the alternatives:

* **`cx`** (and `cyc`, `cyclomatic`, `vgs`, `wg`): an abbreviation is a vocabulary item, not a
  shortcut — it has to be learned, it collides with nothing so it teaches nothing, and it buys three
  characters in exactly the places where the house rule already is that a cell which cannot fit a word
  prints the number alone. A reviewer who has to be told what `cx` means has been handed a dialect.
* **`cognitive load`**: this is McCabe's number (§3.2), not a psychological claim, and naming a count
  after an effect it correlates with is how a metric acquires an authority it never earned.
* **`effort`** or **`risk`**: both already have owners — `fathom why`'s ranking and the risk axis — and
  borrowing their words for a raw count would put two answers in one slot (ADR-001 §7.10).
* **`complexity Δ`** as the term: the delta is a *property of* the figure, not part of its name; making
  it part of the name is how you end up with "complexity delta delta" when the absolute needs printing
  too.

Where the word does not fit, the number stands alone and the word lives in the legend, the column header,
the help overlay and the JSON key — which is how `+1 204 −904` already works for line counts. The JSON
keys are `complexity` (the figure) with `raw` members `delta`, `absolute`, `measured`, `unmeasurable`,
`coverage_measured`, `coverage_total`; the store columns are `complexity_*` (§9.1); the gap's 35-column
short line is `complexity: {reason}`; the section figure label is `complexity`, matching the `what`
section's existing short lowercase labels (`lines`, `files`, `churn`, `reviewable churn`).

The number's own formatting, so the three states survive a monochrome terminal (ADR-006 §10.2's *colour
never carries a fact alone*, and the reason the sign is a glyph rather than a hue): **`+7` / `−3` / `±0`**
for a measured delta, **`~41 (3 unmeasured)`** for a floor, **`?`** for nothing, and never a bare `-1` or
`0` for an unknown.

### 10.2 The rule for every other use of the word

> The bare word **complexity** denotes the figure defined in this ADR: `V(G)` per unit per §3–§4, basis
> `mccabe-1`. Anything else that is printed, labelled, keyed or documented with that word either **is
> re-fed by this figure** or **carries a qualifier that names what it actually counts**. There is no third
> option, and the qualifier is not a footnote: it is the label.

### 10.3 The three existing collisions, and who fixes which

The word was not empty before this epic. Three shipped things print it, and this ADR assigns each an owner
rather than pretending:

| Site | Today | Decision | Owner |
|---|---|---|---|
| The hotspot axis `complexity` (`hotspots/model.rs`, weight 0.20, knee 120, printed inside `hotspots-1 57 = complexity 15.31 + …`) | fed by `e3.1`'s **proxy** — branches, loops, early returns, nesting — which `hotspots.md` is careful to call a proxy, and which §11.1 shows undercounts Rust and Ruby | **re-fed or renamed, by its owner.** The honest minimal move is to print the qualifier (`structure proxy`) wherever the axis is printed; the honest maximal move is to re-weight the axis on this figure and re-measure its knee. Either way it may not keep the bare word while counting something else. | the hotspots lane (`e3.8`'s owner); *not* a child of this epic — an axis rename is a schema change on that surface |
| The graph heat basis `complexity` (`fathom-tui/src/graph/hand.rs`: *"heat = structural complexity of the module itself"*, and `fathom-cli/src/architecture.rs` currently hands it `Series::unavailable("…not computed at startup")`) | always unavailable, so the seat is empty today | **re-fed**: when the heat map ever draws a complexity channel, it draws this figure. Because it is `unavailable` today, this costs nothing now and prevents the drift later; `e8.6` may light it from the stored columns if it wants the graph pane in scope, and if it does not, the sentence remains *unavailable* and says who owns it | `e8.6` if it takes the graph pane; otherwise the graph lane |
| `structure::Complexity`, the shipped per-declaration proxy **type** (`body_lines`, `branches`, `loops`, `params`, `returns`, `members`, `nesting`) | a public type in the same crate `e8.2` adds `V(G)` to, whose own doc comment says *"nothing here is a cyclomatic-computation claim"* | **renamed** to something that does not contain the word (`e8.2`'s choice — `StructureProxies` reads truest to what it counts). The word in this crate belongs to the figure this ADR defines; two public types named for one word in one crate is a collision a reader hits on the first `use` | `e8.2`, mechanically, in the commit that adds the second type |

What is *not* a collision: `fathom why`'s risk axes are named `surfaces`, `size`, `deletions`, `breadth`,
`author` today, and `e8.7`'s new axis takes the name `complexity` — same word, same quantity, which is the
point. Its printed string must name its model version (`spine-2 …`, or whatever `e8.7` calls it), so a
number that used the axis is traceable to the arithmetic that made it.

### 10.4 The test that keeps the word

Each surface owes its own check that it prints this word for this figure and no other — the same shape as
the existing contract suites, not a new idea:

* `e8.5`: `fathom analyze --json` has a `complexity` figure in the `what` section, and
  `schemas/episode-delta.schema.json`, regenerated by `fathom analyze --schema --write`, says so.
* `e8.6`: the rail legend, the card row label and the diff-tree sort name say `complexity`, asserted in
  the ADR-006 generated-layout check that already diffs column sets against the code.
* `e8.7`: `fathom why`, the card's ranking section and the `--plain` frame print one identical vector
  string containing the axis name, which is that ticket's existing acceptance criterion.
* `docs`: `tools/check_docs.py` already fails a docs page that cites a plan key the plan does not have;
  the vocabulary check here is one assertion in the structure-complexity contract suite: no page under
  `docs/` uses the word for a quantity that is not this one, with the two §10.3 exceptions listed in the
  test as exceptions with their qualifiers.

---

## 11. Measured facts

Three things were checked rather than asserted. All three are re-runnable from a checkout, and the numbers
here came out of the runs rather than being typed in.

### 11.1 The node-kind audit: fourteen kinds the shipped proxy counts and the grammars never emit

`e3.1` ships a complexity **proxy** (`structure::Complexity`) that counts node kinds per declaration, and
its lists are the nearest thing to a decision-point table in the tree. Checking every kind those six lists
name against the `node-types.json` shipped by the exact crate versions `Cargo.lock` pins — plus real parses
of probe files through the shipped bindings — finds **fourteen kinds that the pinned grammars do not emit
at all**, i.e. entries that count 0 forever:

| Grammar | Kinds its list counts that the grammar never emits | Effect on the shipped proxy |
|---|---|---|
| `tree-sitter-rust` 0.24.2 | `if_expr`, `if_condition`, `while_condition`, `let_else` (branches); `for_expr`, `for_loop`, `while_loop` (loops) | Rust `branches` counts only `match_arm` and `else_clause`; **an `if` in a Rust file adds nothing to its proxy score**. Rust `loops` counts only `loop_expression`: a `for` and a `while` add nothing |
| `tree-sitter-python` 0.25.0 | `if_expression` (real kind: `conditional_expression`); `for_clause` (real kinds: `for_in_clause`, and comprehension filters are `if_clause`) | Python ternaries and comprehension `for`s are counted as 0 |
| `tree-sitter-ruby` 0.23.1 | `when_clause` (real: `when`); `rescue_clause` (real: `rescue`); `ternary` (real: `conditional`); `call_block` (real: `block`/`do_block`) | Ruby `case`/`when`, `rescue` and ternaries are counted as 0 |
| `tree-sitter-typescript` 0.23.2 | `conditional_expression` (real: `ternary_expression`) | TS ternaries are counted as 0 |
| `tree-sitter-go` 0.25.0 | *none* | clean |
| `tree-sitter-java` 0.23.5 | *none* | clean |

Two things follow, and they are the reason this ADR is allowed to be this specific.

First, for `e8.2`: **C-KIND (§4)** — every kind an implementation lists must be in the pinned grammar's
`node-types.json` *and* produced by at least one corpus file. A list is not evidence that the grammar
emits what the list says, and the failure mode is invisible: the figure comes back low, plausible, and
confident, which is the worst combination this product can ship.

Second, for the hotspot lane: its `complexity` axis is fed by that proxy (§10.3), so the axis today
understates precisely the languages where the misses are — Rust, Python, Ruby, TypeScript — which is a
statement about the ranking's shape, not about those languages' difficulty. This ADR does not fix it (it
is another owner's axis with its own corpus and its own knee), but it does record that the axis and this
figure are not the same quantity and may not both be called `complexity`.

Reproduce it (needs only the vendored registry, no build):

```bash
# which kinds does a pinned grammar actually emit?
python3 - <<'PY'
import glob, json, os, pathlib
cargo_home = pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
for crate in ("rust-0.24.2", "python-0.25.0", "typescript-0.23.2/typescript", "go-0.25.0",
              "ruby-0.23.1", "java-0.23.5"):
    found = glob.glob(f"{cargo_home}/registry/src/*/tree-sitter-{crate}/src/node-types.json")
    kinds = {n["type"] for n in json.load(open(found[0]))}
    print(f"{crate:16} {len(kinds):4} kinds   if_expr={('if_expr' in kinds)!s:5} if_expression={('if_expression' in kinds)}")
PY

# and what does a real parse look like, through the shipped bindings?
cargo run -p fathom-analyze --features langs-all --example s1_spike -- --sexp rust /tmp/probe.rs
```

### 11.2 Spike `s1`'s verdict table, quoted as the evidence for §4 and §7.3

From [structure-extraction.md](../concepts/structure-extraction.md) (generated from the committed report,
so these are measurements and not this document's opinions):

| Language | Verdict | Definitions P / R | Recorded condition |
|---|---|---|---|
| rust | `go_with_conditions` | 100.0 / 96.7 | macro-generated items are invisible; a symbol absent from the parse is not a symbol absent from the crate |
| python | `go_with_conditions` | 100.0 / 96.8 | class bodies rewritten by a decorator are only partly visible; assignment-as-definition is invisible unless the value looks like a function |
| typescript | `go` | 100.0 / 100.0 | none recorded |
| go | `go` | 100.0 / 100.0 | none recorded |
| ruby | `go_with_conditions` | 100.0 / 94.0 | a file's declaration list is not the set of methods the type ends up with; methods whose names are computed at runtime cannot be listed |
| java | `go_with_conditions` | 100.0 / 95.7 | accessors the compiler synthesises are not in the tree |

Three rows of that table are load-bearing here. Every definition miss `s1` recorded is a *reduced recall*
of the same kind: `macro_generated` 0/3, `dataclass_generated` 0/2, `define_method_loop` 0/4,
`record_accessor` 0/2, each tagged irreducible — **the facts are not in the file**. That is §7.3's entire
argument, and it is why the complexity figure treats a macro-written branch as a floor rather than a miss.
The `go` verdicts for Go and TypeScript are why §4.3/§4.4 add no scope caveats for either beyond the two
already in `e3.1b` (`ambient_merge`, `build_constraint`).

`s1`'s cost table is the other half: **Ruby measures 0.612 MiB/s against 11–16 MiB/s for its neighbours**,
which is why `e3.1` already ships a per-run scan budget for it, why `e8.4`'s ceilings are expressed in that
existing vocabulary (`scan_budget`, naming the key that ran out) rather than a new one, and why §12.6
refuses a second parse of any file.

### 11.3 `e2.4`'s pairing tiers, quoted as the evidence for §6.3

From [rename-pairing.md](../concepts/rename-pairing.md), measured over 37 review windows and 67
hand-authored pairings: the fact tier (git's own rewriter at ≥ **0.75**) emitted 40 pairings with
**0 untrue** (precision 1.0000, recall 0.6452 of claimable); the guess tier emitted 21 with 1 untrue; no
grouping — split, fold, copy — ever reaches the fact tier (gate `X1`), and path similarity is display-only
because `s4` ablated it and adding a 0.15 path weight took wrong pairings from 1 to 36 at the same bar
(gate `G39`; gate `P4` is the shipped check that the weight stays absent). That is why §6.3 pairs
on facts and reviewer accepts, and on nothing else: the tier that is 1.0000 precise is exactly the tier
this metric can afford to build arithmetic on, and the tier that is 0.95 is exactly the tier whose one
error would be a sign flip a reviewer cannot see.

---

## 12. Alternatives considered

1. **Cognitive Complexity (Ann Campbell's formulation) instead of McCabe.** Rejected: it scores nesting
   and "structural" penalties with weights that differ per implementation, which fails ADR-001 §7.9 (a
   reviewer cannot reproduce a weighted nesting penalty from the source by eye) and duplicates the nesting
   signal `e3.1` already counts and the hotspot axis already spends. McCabe is one sum of pointable
   things; that simplicity is the feature.
2. **Halstead metrics, or the maintainability index.** Rejected: operator/operand *counts* (not
   distinctness-based volume) are unstable under formatting and renaming, and the maintainability index is
   a weighted blend of three such measures rescaled by a constant chosen to make the output land on a
   0–100 dial — the archived form of the threshold problem §8.3 refuses. Neither survives
   "defend the number line by line".
3. **Line counts of the function body, already stored.** Rejected, and it is a real competitor: the
   extractors already count `body_lines`. A body's length measures what is written; this measures what
   forks. `s1`'s own framing is the reason — the whole point of the structural lane is that declarations
   and control flow are different facts from bytes — and the hotspots lane already treats size and
   complexity as two axes with two weights rather than one.
4. **Count per pattern alternative (§3.4's rejected reading).** Rejected for the three reasons there:
   by-eye checkability, six grammars that all model arms, and gameability.
5. **One absolute figure on the rail, not a delta.** Rejected in §5.3, and worth keeping on the list
   because it is the default a UI ticket reaches for when the delta is awkward: an episode that touches a
   file it cannot read has no delta, and an absolute lets it print a number anyway — which is §7's refusal
   arriving through the back door.
6. **A second parse pass over the file.** Rejected: the pass runs inside `e3.1`'s existing traversal of the
   same tree, so the marginal cost is a per-node kind test. A second parse would double `e8.4`'s whole
   problem (and, for Ruby, double the one language `s1` measured as already 20× its neighbours), and it
   would drift from the structure extractor's own view of what a function is — two traversals, two
   opinions, ADR-001 §7.10.
7. **Per-function figures on the spine, not derived rows.** Rejected on size (§8.7, §9.4): the same
   content stored once per revision that touched it.
8. **Letting each surface compute its own number.** Refused by the epic itself (ADR-002 §5.3, lint `M02`:
   `fathom-tui` renders values it is handed; the arithmetic lives in `fathom-analyze`) and by ADR-001
   §7.10. Not really an alternative; listed because it keeps being proposed in review.
9. **Storing the delta as a column.** Rejected (§9.1): it is `after − before`, both of which are stored.
10. **A threshold with a sensible default baked in, "just for a starting point".** Rejected (§8.3), and
    this is the alternative that will be proposed again: the sentence "just a default, it is configurable"
    is how a metric acquires a meaning nobody argued about in the place where the consequence lives.

---

## 13. Consequences

* **`e8.2`** gets a spec and a gate: six lists it may not extend quietly (C-KIND), a corpus whose
  `truth.json` must contain a file exercising every kind it counts, the truncation flag honoured
  (§7.2), and a public-type rename (§10.3). Its "100 % on definitions the extractor can see, every miss a
  blind spot this ADR already named" bar is now checkable against §11.2 rather than against optimism.
* **`e8.3`** gets six columns, two `CHECK`s, one claim to assert in a test (**zero derived rows evicted**,
  §9.3), a join instead of a roll-up column (§9.2), and a doctor coverage line it already asked for.
* **`e8.4`** gets a reason-code list it must fill rather than invent, an existing vocabulary to reuse
  (`scan_budget`, `too_large`) in the e7.3 ceiling shape it asked for, and the fact that content-addressing
  (§9.3) is what makes its budget satisfiable on a monorepo.
* **`e8.5`** gets the figure's exact `raw` keys, its label, its 35-column gap line prefix, and the
  constraint that the delta document stays additive at its current version.
* **`e8.6`** gets the glance decision (delta, signed), the formatting that survives `NO_COLOR`, the
  overflow constraint that a dropped column must not be confusable with an unknown, and one word for the
  column it is adding to ADR-006 §5.2.
* **`e8.7`** gets an axis named `complexity` with no threshold attached to it, and a G3 re-measure it must
  report honestly in either direction — §8.3's refusal means the axis can be declined on evidence without
  the primitive being defective.
* **Nobody gets to say the metric is validated by this document.** It defines a count and the conditions
  under which the count is refused. Whether the count predicts anything a reviewer cares about is `e8.7`'s
  measurement, and the outcome "the axis does not earn its weight" is a legitimate result of it (§8.5).
* **A reviewer gets a new column whose every number can be traced** — to a file and a decision point in it
  (`e8.2`'s dump), to a pairing verdict (`e2.4`), or to a reason code naming what this build could not
  read. That traceability is the deliverable; the number is the index into it.
* **Two shipped surfaces are now known to be mislabelled** (§10.3's hotspots axis and heat-basis sentence).
  They are named with owners rather than silently fixed by a child of this epic, because renaming a printed
  axis is a schema change on a surface this ticket does not own.

---

## 14. Verification — the commands the children owe

This ADR has no code of its own, so its acceptance criteria are the documents and the audit (§11.1). What
follows is what each named child must make true, and the command that shows it:

| Child | Must make true | Command that shows it |
|---|---|---|
| `e8.2` | six lists implemented as written; C-KIND enforced; truncation honoured; corpus `truth.json` names every figure; unknowns carry a §7.1 code | `cargo run -p fathom-analyze --features langs-all --example complexity_harness -- --check` (the spike `s1` harness shape: a committed report, a byte-exact `--check`, one CI step placed per the `e11` triage) |
| `e8.3` | six columns via `ADDED_COLUMNS`; the two `CHECK`s; the "zero rows evicted" claim in a comment **and** a test; the roll-up join; doctor coverage | `cargo test -p fathom-store` (the cache-key assertion lives beside the existing `ROW_SCHEMA_VERSION`-against-migration-list check) |
| `e8.4` | every §7.1 code that a budget can produce is produced, counted and reported; ceilings declared as config keys; the cost measured on a real repository | its committed `benchmarks/reports/` artefact + `fathom doctor` |
| `e8.5` | a `complexity` figure in the `what` section, built through the evidence constructor; additive at the unchanged delta-document version | `python3 tools/check_delta_compat.py check`, `cargo test -p fathom-cli --test delta_schema`, `fathom analyze --schema --write` diffing clean |
| `e8.6` | the rail column (`+7 (4/5)`, `?` with its reason), the card row, the diff-tree sort, ADR-006 §5.2 regenerated together with the layouts | `PYTHONPATH=src python3 -m recommender.tui layout …` + `cargo test -p fathom-tui --test timeline_golden --test diff_golden`, every changed golden line named in the commit message |
| `e8.7` | the axis, its cap, its printed model string, and an honest G3 report in whichever direction it lands | `cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check`, `PYTHONPATH=src python3 -m pytest -q tests/test_g3_calibration_contract.py` |
| all of them | one word, everywhere (§10) | the structure-complexity contract suite, plus `python3 tools/check_docs.py` |

And this document's own two checks, run before it was committed:

```bash
python3 tools/check_docs.py                       # plan keys, bead ids, ADR references, links
python3 tools/check_docs_site.py                  # the page is reachable, its anchors resolve, it renders
```

---

**Amendment note.** Any change to §4's lists, §3's unit rule or §3.4's arm rule bumps the basis token
(§9.5) in the same commit, and adds a row to this note saying which figures move under it. A change that
does not bump the token is not a refinement, it is a silent redefinition — which is the only failure mode
this ADR has left itself vulnerable to, so it is written down twice.
