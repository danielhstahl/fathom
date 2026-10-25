# What six tree-sitter grammars can actually tell `e3.1`

Epic 3 spends most of its budget on a claim nobody had tested: that a structural extractor can
read six languages well enough to draw a module graph, an API surface and a set of architecture
panes from source. `e3.1` (`reco-zoz`) proposed "tree-sitter structural extraction across the six
grammar features", ADR-002 §6.5 gated each grammar behind its own feature and wrote that
"**`s1` decides whether any of the six is not worth shipping**", and ADR-004 §13 refused to pay
for grammars before somebody measured what they return. Spike `s1` (`reco-alj`) is that
measurement: six throwaway extractors, one per language, scored row-by-row against a
hand-annotated corpus of the constructs each language uses to build an API.

**The short answer: all six grammars ship, and not one of them is a dealbreaker — but four of the
six lose declarations in a way no extractor can fix, and they all lose them for the same reason.**
Every miss in the measurement is a declaration that a *program* wrote rather than a programmer: a
`macro_rules!` expansion, a `@dataclass` generated `__init__`, a `record` accessor, a
`define_method` over a list of names. A parser that never runs the compiler cannot see those, and
`e3.1` has to be designed as if it knows that, because a dependency graph that silently omits the
macro-generated half of a crate is worse than one that admits it does not know. Separately, one
grammar (Ruby) is an order of magnitude slower than its neighbours and cannot hold the floor,
which is a feature-gate fact for ADR-002 §6.5 rather than an accuracy one.

<!-- BEGIN GENERATED: s1_verdict -->
| Language | Verdict | Definitions P / R | Exports P / R | Imports P / R | Conditions carried |
| --- | --- | --- | --- | --- | --- |
| **rust** | `go_with_conditions` | 100.0 / 96.7 | 100.0 / 92.7 | 100.0 / 100.0 | macro-generated items are invisible; a symbol absent from the parse is not a symbol absent from the crate |
| **python** | `go_with_conditions` | 100.0 / 96.8 | 100.0 / 100.0 | 100.0 / 100.0 | class bodies rewritten by a decorator are only partly visible; assignment-as-definition is invisible unless the value looks like a function |
| **typescript** | `go` | 100.0 / 100.0 | 100.0 / 100.0 | 100.0 / 100.0 | none recorded |
| **go** | `go` | 100.0 / 100.0 | 100.0 / 100.0 | 100.0 / 100.0 | none recorded |
| **ruby** | `go_with_conditions` | 100.0 / 94.0 | 100.0 / 100.0 | 100.0 / 100.0 | a file's declaration list is not the set of methods the type ends up with; methods whose names are computed at runtime cannot be listed; assignment-as-definition is invisible unless the value looks like a function; a class produced by a library call is visible only as the call |
| **java** | `go_with_conditions` | 100.0 / 95.7 | 100.0 / 100.0 | 100.0 / 100.0 | accessors the compiler synthesises are not in the tree |
<!-- END GENERATED: s1_verdict -->

## What was measured, and what the numbers are not

Three relations were scored, per file, as exact set comparisons:

- **definitions** — one row per named declaration at file top level or inside a type/module
  container, members included, function-local declarations excluded;
- **exports** — one row per top-level symbol *nameable from outside the declaring unit*, per the
  language's own access rules (`pub` and re-export visibility in Rust, `__all__` then leading
  underscore in Python, the capitalisation rule in Go, `export` in TypeScript, `public` plus
  `module-info.java` in Java, the degenerate top-level surface of Ruby);
- **imports** — one row per `(file, resolved file, symbol)` dependency **resolvable inside the
  repository**, with `*` for a whole-file/package/module dependency.

The corpus is `crates/fathom-analyze/spike/s1/corpus/<language>/`: small synthetic files written
to be the awkward cases, not the easy ones, plus a `truth.json` naming every declaration, export
and resolved edge by hand. It is ~30 KB of source and 625 annotated rows — 373 definitions, 149
exports and 103 resolved import edges — which is small on
purpose — the question was "can the grammar express this relation at all", and a large corpus
measures the annotation budget instead.

<!-- BEGIN GENERATED: s1_corpus -->
| Language | Files | Bytes | Truth rows: definitions | exports | import edges |
| --- | --- | --- | --- | --- | --- |
| rust | 10 | 7,516 | 90 | 41 | 24 |
| python | 7 | 6,072 | 62 | 28 | 19 |
| typescript | 9 | 4,488 | 53 | 38 | 25 |
| go | 11 | 3,869 | 54 | 26 | 14 |
| ruby | 6 | 3,517 | 67 | 8 | 7 |
| java | 10 | 3,897 | 47 | 8 | 14 |
| **total** | 53 | 29,359 | 373 | 149 | 103 |
<!-- END GENERATED: s1_corpus -->

Three things these numbers are not, stated before anybody reuses them:

1. **They are an upper bound, not a prediction.** The extractors are throwaway (`crates/fathom-analyze/examples/s1_spike/`),
   written to interrogate the grammar, and the code `e3.1` shipped (`crates/fathom-analyze/src/structure/`,
   six languages and a corpus harness of its own) is not that code. A number that the
   spike *failed* was a floor on what `e3.1` had to clear; a number it passed was only a promise that
   the grammar can be made to say it.
2. **Four of the six languages were measured on synthetic code for cost.** Only Rust and Python
   have real files in this repository to parse; Go, TypeScript, Ruby and Java cost numbers come
   from a replica corpus generated from the fixture (see §Cost). Accuracy is measured on the
   fixture for all six.
3. **`truth.json` is an opinion, written by one author, about what "declared here" means.** It is
   reviewable and the contract test checks it is *complete* against the corpus, but it is not
   ground truth. Where it is wrong, the report is wrong in the same direction.

## Accuracy, by language and relation

<!-- BEGIN GENERATED: s1_relations -->
| Language | Relation | Found | Correct | Missed | Spurious | Precision | Recall | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| rust | definitions | 87 | 87 | 3 | 0 | 100.0% | 96.7% | 98.3% |
| rust | exports | 38 | 38 | 3 | 0 | 100.0% | 92.7% | 96.2% |
| rust | imports | 23 | 23 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| python | definitions | 60 | 60 | 2 | 0 | 100.0% | 96.8% | 98.4% |
| python | exports | 28 | 28 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| python | imports | 19 | 19 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| typescript | definitions | 53 | 53 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| typescript | exports | 38 | 38 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| typescript | imports | 25 | 25 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| go | definitions | 54 | 54 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| go | exports | 26 | 26 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| go | imports | 14 | 14 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| ruby | definitions | 63 | 63 | 4 | 0 | 100.0% | 94.0% | 96.9% |
| ruby | exports | 8 | 8 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| ruby | imports | 7 | 7 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| java | definitions | 45 | 45 | 2 | 0 | 100.0% | 95.7% | 97.8% |
| java | exports | 8 | 8 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| java | imports | 14 | 14 | 0 | 0 | 100.0% | 100.0% | 100.0% |
<!-- END GENERATED: s1_relations -->

**Rust is where the grammar is the most helpful and the most misleading.** 87 of 90 declarations,
38 of 41 exports, 23 of 23 import edges. The imports are the interesting number: a Rust file's
dependencies are not `import` statements at all but module declarations (`mod file;` is the edge
that makes a directory a module tree), re-exports that move a symbol's provenance
(`pub use crate::geo::point::Point;` in `geo/mod.rs` means the real declaration is in
`geo/point.rs`, and an edge to `geo/mod.rs` is an edge to a *forwarder*), and glob imports whose
path lives in a grammar node called `scoped_use_list` — not `use_list`, which is the other
grouping form. Getting from "we parse uses" to 23/23 meant handling four distinct spellings that
the first version silently dropped: `use a::{b, c}`, `use a::*`, `use a::{self as x}`, and
`use a::{c, d}` nested inside a scoped path. The three misses are `macro_rules!` output (§The
blind spots).

**Python is where a naive walker loses the API.** The first version of the Python extractor
reported ~85 % recall on definitions and looked fine; the misses were `@property`, `@classmethod`,
`@staticmethod` and every method of every `@dataclass`, because those declarations sit inside a
`decorated_definition` node and the walker matched on `function_definition`. A decorator is not
decoration in a graph sense — it is the definition site of half of a real module's surface. After
unwrapping it, 60 of 62 definitions and 28 of 28 exports. The remaining misses are the synthesised
`__init__` of two `@dataclass` classes. Two annotation decisions matter more than the code:
`__all__` is the export authority (a name absent from `__all__` is not exported, however public it
looks), and a star import is recorded as a whole-file dependency with symbol `*`, because no
parser can enumerate what `from models import *` binds.

**TypeScript and Go were clean: 100 % on all three relations.** Neither was easy.
TypeScript's difficulty is that the grammar puts the two most important tokens in a module system
into *anonymous* children: `export default` and `export * from` are marked by `default` and `*`
tokens that a `named_children()` walk cannot see, so a barrel file silently exports nothing and a
default export silently becomes a nameless declaration. Barrels also force resolution to be
iterative: `index.ts → geo/index.ts → geo/point.ts` is only discoverable once the middle hop is
known, which is why the harness resolves in four rounds rather than one. The `.d.ts` case is a
design question as much as a parsing one — `export declare function loadConfig(...)` in an
ambient file *is* the package's only declaration of that symbol for every consumer, so it is
annotated as one, and it lives under an `ambient_declaration` wrapper nobody guesses. Declaration
merging (two `interface Config` blocks, one name) is annotated as one symbol with two
declarations, which is the only reading that keeps "what uses `Config`?" a single answer.

Go was clean because the grammar and the language agree: `T.Method` for a receiver method, one
package name per directory, capitalisation as the export rule, nothing generated. The traps are
not accuracy traps: a Go *package* is a directory, so a file-level Go graph overstates precision
(§What this changes); a `_test` package is a second package in the same directory; and a
`//go:build` pair (`init_linux.go` / `init_other.go`) defines one logical file twice, so a
symbol appears in two files and neither is wrong.

**Ruby is the language where the relation itself is the problem.** 63 of 67 definitions, 8
exports for 67 declarations, and all 7 import edges. The export number is not an extractor
failure: Ruby has no per-symbol access modifier, so "what this file exports" is nearly "what this
file defines at top level", and the relation carries almost no information. Worse for `e3.1`'s
plans, a Ruby file's declaration list is not the set of methods a type ends up with — `reopen`,
`class_eval`, `attr_accessor`, `Struct.new` and `define_method` mean the truth about `Shape` lives
in four files and part of it in a loop over `%i[side1 side2]`. The four misses are exactly the
loop-generated methods. The extractor learned to say which category it hit rather than staying
silent: `attr_reader` and `define_method(:perimeter)` produce symbols, `define_method(n)` from a
collection produces a blind-spot note.

**Java is where the corpus had to annotate an edge that has no syntax.** 45 of 47 definitions, 8
of 8 exports, 14 of 14 import edges — but three of those 14 edges are *same-package* references
(`sealed interface Shape permits Circle` in `Shape.java` with no import statement to read, because
Java does not need one). They are in `truth.json` with tag `same_package` because `e3.2`'s graph
needs them; finding them means scanning type references and resolving them against the package
index, which is the one place in the spike where false-positive risk is a feature rather than an
accident. The corpus is small and the measured FP rate is 0; that is a reason to try the approach,
not a reason to ship it unexamined. The two misses are `record Circle(String name, double r)`'s
generated accessors. Java also taught the export rule its own limits: `public` is not the export
authority inside a module — `module-info.java` is, and a `public` type in a package the module
does not export is nameable inside the module and not outside it.

## Which constructs survive

Per-construct recall, listing only tags that lost anything. Everything not listed was found in
full — including the awkward ones the spike was mostly written to test: cfg-gated Rust items,
re-exports, associated consts, async/unsafe/extern items, relative and conditional Python imports,
`__init__.py` package resolution, TypeScript barrel chains, default exports and namespaces,
generic Go types, embedded interfaces and build tags, Ruby singleton methods, `alias`, operator
methods and `class_eval`, Java annotation types, sealed interfaces and nested types.

<!-- BEGIN GENERATED: s1_tags -->
| Language | Construct tag | Expected | Found | Recall |
| --- | --- | --- | --- | --- |
| rust | `macro_generated` | 3 | 0 | 0.0% |
| rust | `export` | 41 | 38 | 92.7% |
| python | `dataclass_generated` | 2 | 0 | 0.0% |
| python | `method` | 23 | 21 | 91.3% |
| typescript | *every annotated construct found* | — | — | 100% |
| go | *every annotated construct found* | — | — | 100% |
| ruby | `define_method_loop` | 4 | 0 | 0.0% |
| ruby | `method` | 43 | 39 | 90.7% |
| java | `record_accessor` | 2 | 0 | 0.0% |
| java | `method` | 32 | 30 | 93.8% |
<!-- END GENERATED: s1_tags -->

## The blind spots, and which ones are the language's fault

Every extractor could record *why* it lost something. That distinction — "my extractor is
incomplete" versus "the source does not contain this fact" — is the most useful output of the
spike, and it is the thing `e3.1` should copy rather than the accuracy numbers.

<!-- BEGIN GENERATED: s1_blind_spots -->
| Language | Recorded blind spot | Occurrences | Irreducible? |
| --- | --- | --- | --- |
| rust | `associated_type_binding` | 2 | no — extractor gap |
| rust | `macro_definition` | 2 | yes |
| python | `decorated_class` | 3 | yes |
| python | `monkeypatch` | 2 | yes |
| python | `nested_function` | 2 | no — extractor gap |
| typescript | *none recorded* | 0 | — |
| go | *none recorded* | 0 | — |
| ruby | `alias` | 1 | yes |
| ruby | `class_eval` | 1 | yes |
| ruby | `define_method_dynamic` | 2 | yes |
| ruby | `macro_method` | 7 | yes |
| ruby | `monkeypatch_or_data` | 1 | yes |
| ruby | `struct_new` | 3 | yes |
| java | `constructor` | 6 | no — extractor gap |
| java | `record_accessor` | 2 | yes |
<!-- END GENERATED: s1_blind_spots -->

**Irreducible** means the fact is not in the file: `macro_rules!` output (Rust), decorator-written
methods and monkeypatching (Python), compiler-synthesised accessors (Java), runtime-computed
method names and library-produced classes (Ruby). Nothing not on that list is a property of the
language; if the extractor misses `attr_reader :name`, that is a bug.

There is one more irreducible category that the table cannot show because it is not a miss: a
star import, a wildcard Java import and a `pub use *` are *real dependencies whose contents the
parser cannot name*. They are recorded as `*`, and `e3.2` has to decide what that means for
"who depends on X" — see §What this changes.

## Cost, per grammar

One thread, one process, release profile, parse-only and parse-plus-extraction, over the corpus
that represents the language: real files from this repository where it has them (Rust, Python),
the replica otherwise. `fixture` is the 3–7 KB hand-written corpus; `replica` is the fixture
multiplied to ~2 MB of ~800-byte files; `repository` is real source.

<!-- BEGIN GENERATED: s1_cost -->
| Language | Corpus | Parse MiB/s | Extract MiB/s | p50 µs/file | p95 µs/file | ΔRSS KiB |
| --- | --- | --- | --- | --- | --- | --- |
| rust | fixture (10 files) | 7.22 | 25.74 | 69 | 267 | 88 |
| rust | replica (1900 files) | 10.71 | 33.70 | 69 | 242 | 216 |
| rust | repository (196 files) | 11.69 | 168.91 | 1184 | 4011 | 2,112 |
| python | fixture (7 files) | 7.75 | 46.45 | 104 | 201 | 0 |
| python | replica (1708 files) | 2.34 | 37.06 | 474 | 823 | 0 |
| python | repository (110 files) | 11.88 | 193.37 | 1179 | 4082 | 4 |
| typescript | fixture (9 files) | 7.37 | 37.08 | 62 | 168 | 0 |
| typescript | replica (2493 files) | 11.76 | 51.86 | 64 | 169 | 0 |
| go | fixture (11 files) | 8.73 | 71.02 | 30 | 102 | 0 |
| go | replica (3047 files) | 15.08 | 104.74 | 34 | 105 | 0 |
| ruby | fixture (6 files) | 4.51 | 26.90 | 121 | 237 | 0 |
| ruby | replica (2244 files) | 0.64 | 41.35 | 1310 | 2432 | 384 |
| java | fixture (10 files) | 8.88 | 11.21 | 34 | 152 | 0 |
| java | replica (2880 files) | 15.06 | 17.02 | 36 | 140 | 0 |
<!-- END GENERATED: s1_cost -->

The floor in `thresholds.json` is 5 MiB/s parse-only — deliberately not a performance target,
because ADR-004's `cold.graph.ms` and `warm.graph.marginal.ms` budgets are paid per *changed*
file and any of these numbers clears them by two orders of magnitude. It is a canary against a
pathological grammar. **Ruby trips it: under 1 MiB/s on the replica shape against 11–16 MiB/s for every
other language** (the exact figure is in the table below, and it jitters run to run). The spike does not claim to know why (the fixture at 5.57 MiB/s says it
is not simply the grammar; the shape of the replica — many small declarations per file — says the
Ruby grammar's cost is per syntactic construct rather than per byte, which is a bad property for
the method-heavy files Ruby turns out to be made of). What it claims is that `e3.1` must budget
Ruby separately, and that ADR-002 §6.5's per-grammar feature gates are the right place to put that
decision.

The other cost that matters is not in this table: **six grammars are six C builds.** That is the
thirty-second-versus-four-minute build difference ADR-002 §6.5 describes, and it is why everything
in this spike lives behind `--features langs-all` (`required-features` on the example, so the
default `cargo test` never compiles a grammar to read a report about one).

## Dealbreakers

None. For the record, the closest calls, stated as the thing `e3.1` must not conclude:

| Language | What the grammar must not be used to conclude |
| --- | --- |
| Rust | that a symbol a macro wrote does not exist, or that a re-export is a declaration |
| Python | that a decorated class has no methods, or that `from x import *` imports nothing |
| TypeScript | that a default export is unnamed, or that a barrel exports nothing |
| Go | that a Go dependency is a file-to-file fact (it is package-to-package) |
| Ruby | that "this file's public symbols" is a meaningful question |
| Java | that a dependency without an `import` statement is no dependency |

## What this changes for `e3.1`, and for the tickets after it

1. **Model "the file declares things I cannot enumerate" as a state, not an absence.**
   Every language has declarations that only the running program knows (`e3.1`). Concretely: an
   extractor seeing `macro_rules!`, a decorator, a `record`, or a `define_method` over a
   collection marks the enclosing scope `synthetic_eligible`. `e3.7`'s breaking-change report then
   says "may have shrunk", not "removed"; `e3.2` does not claim a complete caller list for those
   symbols. This is the single highest-value change, because the alternative is a graph that is
   confidently wrong about the most architecturally interesting code in the repository.
2. **Wildcard dependencies need their own edge kind.** A star import, a `pub use *`, a barrel
   `export *` and a Java wildcard import are all "this file depends on that unit, contents
   unknown" (`e3.1`, `e3.2`). If the graph stores them as nothing, `who depends on models.py?`
   silently answers "nobody", which is worse than an unknown. The spike resolved every wildcard
   edge it annotated, so the resolution rules are known; the graph just has to keep the `*`.
3. **Ruby's API surface is the file, not the symbol** (`e3.7`). With 8 exports for 67 declarations
   and no per-symbol access control, the only honest Ruby surface question is which files a
   gem's entry points load. Say that in `e3.7` rather than shipping a Ruby column that is a lie.
4. **Go edges are drawn at package granularity, or labelled as such** (`e3.2`, `e3.5` layering).
   A file-level Go graph says `main.go → shape/circle.go` where the language says
   `main → shape`. Both are "true"; only one is a dependency anyone can reason about, and the
   architecture panes in `e3.5` will be read as language-shaped claims.
5. **Java same-package edges need type-reference scanning, and that needs a false-positive budget**
   (`e3.1`, `e3.2`). The spike got 14/14 with zero false positives on a 10-file corpus; the same
   technique on a 4 000-file corpus will resolve `Builder`, `Result` and `Config` to the wrong
   package sometimes. Re-measure on a real repository before `e3.2` draws same-package edges at
   all, and scope the scan to imports-plus-declared-package rather than to every identifier.
6. **Resolution is iterative and the budget must assume it.** Re-export chains (`index.ts →
   geo/index.ts → geo/point.ts`, `geo/mod.rs → geo/point.rs`) only close after multiple passes;
   the spike runs four. That is cheap at corpus scale and needs to be bounded per file at
   repository scale, since it is the only super-linear step in extraction (`e3.1`, `e3.2`).
7. **Feature gates stay as ADR-002 §6.5 wrote them.** All six grammars ship in `langs-all`; no
   language is dropped, so no `e3.x` feature list shrinks. What changes is the *scan policy*:
   Ruby's per-construct cost argues for a size/count cap per run (and per-file priority), which
   belongs in `e3.1`'s configuration surface next to the noise policy.
8. **Copy the harness shape, not the extractors.** The durable output of this spike is
   corpus + hand-annotated `truth.json` + thresholds + report + the rule that the committed
   report must reproduce (`--check`, and `tests/test_structure_spike_contract.py` so the prose
   cannot drift from the measurement). If `e3.1` ships "accurate extraction" without a corpus and
   a truth file per language, nobody will be able to tell whether it got worse (`e3.1`, and
   `e6.x`'s regression story).

## The normalisation rules the corpus commits to

`e3.1` inherits these by shipping the corpus; they are the answer to "what is a symbol", which is
the question six languages answer six different ways.

- **Identity** is `(file, kind, dotted name)`. `Shape.area` in `shapes.ts` and `area` in
  `shapes.ts` are two rows; a method's container is part of its name.
- **Kinds** are one vocabulary of ten — `module | class | interface | struct | enum | type | fn |
  method | const | var` — mapped per language. A Rust `trait` and a Java `interface` are both
  `interface`; a Rust `mod` is a `module`; a Go `type X struct` is a `struct`; a Python
  `NamedTuple` is a `class`.
- **Declarations inside a function body are not symbols.** They are visible to the file's own
  analysis and invisible to the graph. Two languages (Python, TypeScript) would otherwise produce
  their largest category of noise.
- **Fields are not symbols anywhere.** Struct fields, class attributes and record components
  produce members only where the language gives them *behaviour* (accessors count as methods in
  the corpus, fields do not).
- **Members are excluded from the export set.** Only top-level symbols can be exported; a public
  method of a private class is not API.
- **An export is a name nameable by code outside the declaring unit**, evaluated with the
  language's own rules — which for Java means reading `module-info.java`, for Python means
  reading `__all__` when present, and for Ruby means the answer is almost always "everything".

## Grammar versions

The grammars are pinned by `Cargo.lock`; the checksums make a grammar bump visible in the report
instead of invisible in a re-run.

<!-- BEGIN GENERATED: s1_versions -->
| Crate | Version | Grammar ABI | Grammar checksum (sha256, first 12) |
| --- | --- | --- | --- |
| `tree-sitter-rust` | 0.24.2 | 15 | `439e577dbe07` |
| `tree-sitter-python` | 0.25.0 | 15 | `6bf85fd39652` |
| `tree-sitter-typescript` | 0.23.2 | 14 | `6c5f76ed8d94` |
| `tree-sitter-go` | 0.25.0 | 15 | `c8560a4d2f83` |
| `tree-sitter-ruby` | 0.23.1 | 14 | `be0484ea4ef6` |
| `tree-sitter-java` | 0.23.5 | 14 | `0aa6cbcdc8c6` |
<!-- END GENERATED: s1_versions -->

## Reproducing it

```bash
# measure, and write the committed report (~2 s)
cargo run --release -p fathom-analyze --features langs-all --example s1_spike -- \
    --repo . --report benchmarks/reports/s1-tree-sitter-accuracy.json

# the gate: the committed report still matches a fresh run (relations and verdicts, not timings)
cargo run -p fathom-analyze --features langs-all --example s1_spike -- --repo . \
    --report benchmarks/reports/s1-tree-sitter-accuracy.json --check

# these tables match the report
python3 tools/gen_structure_tables.py check

# the prose, the thresholds and the report cannot drift apart
PYTHONPATH=src python3 -m pytest -q tests/test_structure_spike_contract.py
```

Three debugging affordances exist because every "why did that not resolve" question ended in the
same place:

```bash
cargo run -p fathom-analyze --features langs-all --example s1_spike -- --probe rust        # node kinds and parse errors
cargo run -p fathom-analyze --features langs-all --example s1_spike -- --dump ruby         # what the extractor said, per file
cargo run -p fathom-analyze --features langs-all --example s1_spike -- --sexp go pkg/shape/shape.go   # the raw tree
```

`--check` compares relation counts and verdicts only, never throughput, so the gate passes from a
debug build against a report written by a release one. The report is committed next to ADR-004's
baseline and `s3`'s snapshot report for the same reason theirs are: a number nobody can re-run is
a number nobody should plan around.

---

## What `e3.1` shipped, and what `e3.2` may now assume

The spike was a measurement with an expiry date. This section is the shipped thing.

`crates/fathom-analyze/src/structure/` extracts, per file: definitions with a span and a complexity
proxy, exported visibility, imports with their resolved targets, implemented traits and inherited
bases, and the two states this document argued for — `synthetic_eligible` scopes and wildcard
dependency edges. It ran Rust, Python and TypeScript; `e3.1b` (`reco-dlp.1`, below) added Go, Ruby
and Java, so all six languages `s1` measured are now extracted. A language the extractor has not
shipped is still a different fact from a language fathom has never heard of — the six that ship are
still joined in `Language::detect` by the ones that do not, each answered with a named reason. The
measured result is
[`benchmarks/reports/e3-1-structure-core.json`](../../benchmarks/reports/e3-1-structure-core.json), and every
number below came out of that file rather than being typed into it.

| language | definitions | exports | imports | supertypes | synthetic scopes | wildcards | verdict |
|---|---|---|---|---|---|---|---|
| Rust | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | 1/1 | 1/1 | `conditions` |
| Python | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | 6/6 | 2/2 | `conditions` |
| TypeScript | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | 5/5 | 5/5 | `conditions` |

Perfect scores on a 26-file corpus are not a claim about production repositories, and `e3.2` should
not read them as one — they are the statement that the *mechanisms* work on cases chosen because
they are hard, and that the harness would notice the first one to break. Each language's verdict is
`conditions` for exactly one reason, and it is a reason with a name: macro-written items in Rust,
`@dataclass` and monkeypatched methods in Python, bundler-written modules in TypeScript. Those rows
are in the truth files, excused by tags declared in
`crates/fathom-analyze/structure/thresholds.json`, and excused *only* in files that also demand a
`synthetic_eligible` scope — `tests/test_structure_core_contract.py` refuses the excuse otherwise.
An extractor is allowed to say it cannot enumerate a scope; what it is not allowed to do is say
nothing.

Five things the next tickets get to rely on, each with the sentence that makes it testable:

* **A file is never lost.** Every input returns a record; one with no structure returns a reason
  (`language_unknown`, `grammar_not_compiled`, `extractor_not_shipped`, `too_large`, `not_utf8`,
  `parse_failed`, `parser`) and its file-level measurement — bytes, lines, code lines, parse errors.
  `e3.2`'s graph may therefore count files it could not read, which is the difference between a
  partial graph and a quietly short one.
* **A dependency of unknown contents is an edge.** `EdgeKind::Wildcard` is its own kind, never a
  named symbol, and `CorpusStructure::dependents_of` answers "who depends on this?" as two lists:
  the ones that named something, and the ones whose contents are unknown. Printing only the first is
  the bug this whole document was written to prevent.
* **A scope that cannot be enumerated says which and why.** `synthetic_scopes` carries the scope, the
  dotted path, and the reasons (`macro_definition`, `decorated`, `monkeypatch`,
  `synthesised_accessors`, `wildcard_reexport`, `ambient_merge`, …), and `may_have_shrunk()` is the
  only sentence a surface may print about it: *may have shrunk, not shrunk*.
* **Resolution is a bounded iteration and reports itself.** `extract.resolution.max_passes` and
  `…max_hops` are configuration keys (see [the config reference](config.md)), and every file reports
  `passes_used`, `converged` and `truncated_chains`. The report contains the corpus re-extracted with
  a bound of one, so the bound is shown biting rather than asserted.
* **The bytes are stable.** `structure::canonical_json` is byte-identical across runs and across
  input order, and the golden at
  `crates/fathom-analyze/tests/golden/structure/core-canonical.json` is those exact bytes — the same
  bytes `--snapshot` writes, which is why `e3.2` can store this shape and diff it a month later.

## Reproducing `e3.1`

The harness needs all six grammars from `e3.1b` on: there is one report, it lists six languages, and
a build that speaks three cannot reproduce it — that is the price of "one harness, one report", and
it is cheaper than the alternative of two harnesses disagreeing about what measured means.

```bash
# measure, and write the committed report
cargo run --release -p fathom-analyze --features langs-all \
    --example structure_harness -- --repo . \
    --report benchmarks/reports/e3-1-structure-core.json

# the gate: relations, mechanisms and verdicts reproduce (timings deliberately excluded)
cargo run -p fathom-analyze --features langs-all \
    --example structure_harness -- --repo . \
    --report benchmarks/reports/e3-1-structure-core.json --check

# the throughput floor: 1 000 files/s/core, release only
cargo run --release -p fathom-analyze --features langs-all \
    --example structure_harness -- --repo . --bench

# the byte contract, and the corpus/model properties
cargo test -p fathom-analyze --features lang-rust,lang-python,lang-typescript --test structure_core

# the prose, the thresholds, the truth files and the report cannot drift apart
PYTHONPATH=src python3 -m pytest -q tests/test_structure_core_contract.py
```

`--dump <language>` prints what the extractor concluded per file and `--probe <language>` prints
symbol kinds and parse errors; the raw grammar archaeology (`--sexp`) stays spike `s1`'s, and
`s1_spike` still does it against all six grammars. `--max-passes 1` is there to watch the bound bite.

## What `e3.1b` (`reco-dlp.1`) added: the other three grammars, and the scan policy

`s1` measured six grammars and `e3.1` shipped three of them. The tail is the other three — Go, Ruby,
Java — plus the three knobs `s1` left as recommendations, because a language that costs what Ruby
costs is not a reason to gate it out, it is a reason to bound the scan. Everything below is measured
in the same report and by the same harness as the three above; the tail joined that harness rather
than starting a second one, because a second harness is a second set of rules about what "measured"
means.

| language | definitions | exports | imports | supertypes | synthetic scopes | wildcards | verdict |
|---|---|---|---|---|---|---|---|
| Go | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | 2/2 | 14/14 | `pass` |
| Ruby | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | 3/3 | 7/7 | `conditions` |
| Java | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | P 1.000 / R 1.000 | 1/1 | 4/4 | `conditions` |

### Go: an import names a package, and every edge has to admit it

`s1` finding 4, and the finding with the most consequences for `e3.2`: *Go has no file-to-file
dependency*. An import names a directory, so a resolved import fans out to every non-test file of
that package with symbol `*` — a **package-granular** edge, and each one carries `granularity:
package` **and the import path it came from** — because a reader who finds the edge a month later in a stored graph cannot
otherwise tell a package edge from a file one. `Structure::granularity_sentence()` is the wording a
pane must use instead of implying a file-level fact nobody declared:

> 3 of this file's 4 dependency edges are (unnamed) → `example.com/app/pkg/shape` at **package**
> granularity, drawn once per file of the target package; the language's dependency is the package,
> not the file

`dependents_of` splits the same three ways: `named`, `unknown`, and now `package_granular`, so a
caller list can say "these arrived through a package import" rather than merging them into the ones
that named something. Two more Go-specific facts:

* **The module path is deduced, never read.** The extractor is not handed `go.mod` and will not open
  a file it was not given (ADR-004 §5: everything it reads is inside the corpus), so the module path
  is the import-spec prefix that explains the most imports. An import that fits neither that prefix
  nor a corpus-relative path stays unresolved — `fmt`, in the corpus's case.
* **A `//go:build` line is a cannot-enumerate state, not a duplicate definition.** Two files declare
  `metrics.Init` and exactly one is compiled; a parser does not run the build, so both files mark
  their file scope with `build_constraint` and neither may assert emptiness. The `package` clause
  itself is recorded as the file's unit (`Structure::package`) and is not a definition.

### Ruby: a bounded scan, a degenerate export surface, and the question to ask instead

`s1` found Ruby at 0.64 MiB/s against 11–16 for the others, and `e0.12` turned that into a scan
policy instead of a feature gate. The scan budget is three keys, in `[extract.ruby]`: `max_files_per_run` (4 000),
`max_bytes_per_run` (24 000 000), and `priority` (`lib`, `app`, `ext`, `spec`, `test`), so a run that
has to stop early stops having read what an entry point reaches rather than whatever a directory walk
reached first. Whichever cap is spent first ends the scan; files past it come back `scan_budget` with
their file-level measurement, exactly as an oversized file comes back `too_large` — a refused file is
a measured file. The cap is shown biting on the corpus (4 of 6 Ruby files refused at a cap of 2), and
its **cost** is arithmetic: 24 MB at the measured 0.612 MiB/s is 37.4 s, inside ADR-004's 60 s small-class
cold-graph budget. That arithmetic is what buys Ruby's throughput exemption, and
`tests/test_structure_tail_contract.py` refuses the exemption without it.

The rest of Ruby is about not lying in two directions:

* **A resolved `require` names no symbol.** Ruby's dependency is whole-file, so every resolved edge
  is a wildcard and the import relation cannot say which name was used. `require_relative` resolves
  against the file's directory, a bare `require` against the corpus's load root (`lib/` here), and a
  gem stays honestly unresolved rather than being aimed at a stranger.
* **The export set is the file's top-level names and nothing more**, with the note
  `export_surface_is_the_file` attached: 8 exports for 67 declarations, and no per-symbol access
  control anywhere in the language. A surface that prints that list under *public symbols* is
  reporting a distinction Ruby does not have. The question Ruby can answer is
  `CorpusStructure::loads(entry)` — *"loading `lib/demo.rb` pulls in 4 file(s): …"* — and `e3.7` is
  told to print that.
* **A scope a program wrote into is marked**: `dynamic_method` for `define_method` over a constant
  array (four blind-spot rows, the only Ruby ones), `monkeypatch` for a `class_eval` that reopens a
  class this file does not declare. A literal `define_method(:perimeter)`, an `attr_accessor` and a
  `Struct.new` are *in* the file and are extracted; the mark is for what the file cannot enumerate,
  not for unusual syntax.

### Java: the one place a guess is a feature, inside a stated scope

A bare `Builder` in `Builder.java`'s own package is a reference to `Builder.java`, and no import
statement says so. Refusing to resolve it would report no edge at all, so the extractor guesses — and
the guess has a scope, which is `extract.java.same_package_scan` (`imports_plus_package`, the
default; or `all_type_references`). Three rules make the scoped scan what it is: a bare type
reference resolves against the file's **own declared package** and nowhere else; a single-type import
of the same spelling shadows it (JLS §6.5.5) so the import owns that reference; and a name the file
binds itself — a method type parameter — is a mention, not a use. The coarse edge is stated rather
than hidden: bound names are collected per file, not per scope, so a file that binds `<Result>` and
also names the package's `Result` elsewhere loses that one edge, which is a hole rather than a wrong
answer.

The criterion `e0.12` wrote was "re-measure on at least 1 000 files, publish the rate, state the
budget", so the rate is measured on a machine-authored corpus of 1 200 files in 40 packages — and
says so out loud, because nobody hand-annotates 1 200 Java files and a published rate with vague
provenance gets quoted. What makes the number worth having is that the generator plants the traps:
58 type names declared in more than one package, 916 same-name imports that shadow a neighbour, 120
method type variables spelled like a neighbour, and 2 760 real dependencies.

| scan | edges claimed | false-positive rate | recall |
|---|---|---|---|
| `imports_plus_package` (shipped) | 2 760 | **0.000** (budget ≤ 0.02) | 1.000 (floor ≥ 0.95) |
| `all_type_references` (what `s1` ran) | 108 356 | 0.975 | 1.000 |

The wide column is kept as a measurement so the scoping decision stays a number, and so is the
counterfactual for the third rule: the same corpus re-scanned with the bound-name rule switched off
claims 2 880 edges instead of 2 760, a 4.17 % false-positive rate — over the stated budget, on
nothing but method type variables. That is a second scan rather than arithmetic, because arithmetic
about a rule someone later edits out of the code is a number that goes stale in silence. Two Java
details with the same shape as Ruby's: a `record`'s accessors are **not** definitions (they are a
`record_accessor` note and a `synthesised_accessors` mark on the record's own scope), and the export
surface is derived from `module-info.java`, so nothing in `com.acme.app.legacy` is public API.

### The knobs, and the throughput exemptions they buy

| key | default | what it is |
|---|---|---|
| `extract.ruby.max_files_per_run` | 4 000 | files of Ruby one run may parse, in priority order; `0` means no cap |
| `extract.ruby.max_bytes_per_run` | 24 000 000 | bytes of Ruby one run may parse; `0` means no cap |
| `extract.ruby.priority` | `lib, app, ext, spec, test` | which files get read when a cap binds |
| `extract.java.same_package_scan` | `imports_plus_package` | how far the same-package guess reaches |

All four are declared in the config schema with the defaults `policy.rs` uses — a limit only the
library knows about is a limit nobody can set (ADR-004 §5.1) — and a value that is not one of the two
allowed scopes is a notice naming the key, never a silent fallback.

The throughput floor is 1 000 files/s/core and 1 MiB/s, measured on a 4 000-file replica. The tail
needed two *kinds* of exemption, and neither is "this language is allowed to be slow":

* **`via = scan_budget`** (Ruby, 827 files/s and 0.612 MiB/s on the replica): allowed under the MiB/s
  floor because the byte cap bounds what a cold run can spend, and the report states the resulting
  37.4 s against the 60 s budget. Miss the arithmetic and it is a condition on the run.
* **`via = repository_shape`** (Go at 542 files/s and Java at 346 on the replica): the replica is a
  fixture copied 444 times, which for a language whose import names a *package* means packages of
  ~800 files and a fan-out of 563 edges per file instead of the fixture's 14. The exemption is
  judged instead on a machine-authored repository-shaped corpus — 1 240 Go files in 40 packages of
  30 (2 060 files/s), 1 200 Java files in the same shape (1 113 files/s) — and the MiB/s canary is
  scaled by that corpus's own mean file size, because 1 MiB/s at 1 kB per file *is* 1 000 files/s and
  the two floors have to say the same thing on both corpora.

The structural schema version went to **2** with these changes: `granularity` and `package` on every
edge, `package` on every structure, and `scan_budget` in the refusal vocabulary. A version-1 row is
not wrong, it is mute about a Go edge's granularity — which is a question a stored row has to be able
to answer, and the reason the version is stated at all.

## Reproducing the tail

```bash
# measure all six languages and write the committed report
cargo run --release -p fathom-analyze --features langs-all --example structure_harness -- \
    --repo . --report benchmarks/reports/e3-1-structure-core.json

# the gate: relations, mechanisms, verdicts and the same-package rate reproduce (timings excluded)
cargo run -p fathom-analyze --features langs-all --example structure_harness -- \
    --repo . --report benchmarks/reports/e3-1-structure-core.json --check

# the tail's rules as Rust tests: granularity, the budget's refusal path, the same-package scope
cargo test -p fathom-analyze --features langs-all --test structure_tail

# the report, the thresholds, the truth files, the config keys and the CI wiring
PYTHONPATH=src python3 -m pytest -q tests/test_structure_tail_contract.py
```
