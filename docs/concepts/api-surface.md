# Public API surface: what changed, which change drove it, and what a parser cannot know

- **Status**: **shipped as an engine, and now enforced.** `fathom check` (**reco-ngk**, `e6.6`) is here: its
  `api` check diffs the surface across the range, reports every breaking change (and the *indeterminate*
  class at `note`), and takes its volume from `[check.severity] api`. There is still no `fathom api` verb —
  the surface is read by the gate and by the artefact rather than printed on its own. Nothing here writes
  `.fathom/ratchet.toml`; `[api] mode = "deny"` is recorded and honoured by the report, and the exit code is
  the gate's.
- **Decided by**: [structure extraction](structure-extraction.md) (this module reads the same
  tree-sitter source of truth and inherits its honest holes: a `macro_rules!` expansion is invisible
  to both), [change classification](change-classification.md) (file-level classes, which are a
  different question — that page counts edits, this one judges contracts), [the episode](episode.md)
  (every change is keyed to the episode that made it), [the ratchet](ratchet.md) (the baseline a
  breaking change is measured against, when a baseline exists), [config](config.md) (the `[api]`
  table and the warning channel), [ADR-002](../adr/0002-workspace.md) §6.5 (grammars behind feature
  flags), [ADR-004](../adr/0004-storage.md) §5 (deterministic digests, sorted lists, no clocks).
- **In code today**: `crates/fathom-analyze/src/api/` — `policy.rs` (the `[api]` table, and every
  refusal raised while reading it), `testpath.rs` (the built-in answer to "is this a test file?",
  per language, and the one key that can overrule it), `textscan.rs` (bracket-aware scanning of a
  declaration), `signature.rs` (a declaration decomposed into parameters, return, receiver,
  members), `package.rs` (which package a file belongs to, per language), `extract.rs` (one
  revision's surface), `surface.rs` (the surface, its digests, and the fast paths), `diff.rs` (two
  revisions paired), `classify.rs` (what happened, and whether it breaks a caller), `semver.rs` (the
  bump and the change that drove it), `episode.rs` (which episode to blame), `report.rs` (the lines
  and the JSON). 29 unit tests, `tests/api_surface.rs` (28), and a two-revision fixture corpus under
  `crates/fathom-analyze/api/corpus/` for Rust, Python, TypeScript, Go, Ruby and Java.
- **Read this if**: you want to know why a rename is printed as a removal and an addition in Go and as
  a rename in Python, what makes `impl AsRef<str>` additive while `Option<T>` is not, why a vanished
  name is sometimes reported as "may have shrunk" instead of "removed", and which parts of a public
  API this tool will tell you it cannot see.

A review tool that cannot answer "did this episode change the public API" is asking a human to read
diffs for a living. The naive version of this question — diff the files, look for lines that touch a
`pub`/`export`/capitalised declaration — produces an answer that is wrong in both directions often
enough that people stop reading it, and the reason it is wrong is that **a public API is a relation
between a package and its callers, not a set of lines**. This module reads the relation: which names
a consumer can reach, what shape each has, and what happened to that shape between two revisions.

## 1. Four ways to answer, and the answer says which one it used

The first fact about an API diff is that most episodes do not have one. So the work is staged, and the
report states the rung it stopped at, because "nothing changed" printed after reading every file is a
different claim from "nothing changed" printed after comparing nine digests:

| Rung | What was compared | Reported as | Typical cost |
| --- | --- | --- | --- |
| bytes | the digests of the files that can carry a surface | `Answered::Bytes` | no file parsed, no grammar loaded |
| digest | one digest per item, per package | `Answered::Digest` | both revisions extracted, nothing paired |
| items | the items themselves, paired by identity | `Answered::Items` | the full comparison |
| incomplete | as much as could be read | `Answered::Incomplete` | a gap, and no clean answer |

The bytes rung is the one that matters in practice. `fingerprint` digests only the paths that can
carry a surface — the source files of a tracked language, per `[api] ignore` — so a documentation
episode, a test-only episode, a lockfile bump, or a body-only change inside one function answers
`NoPublicApiChange` without a parse. A markdown file whose bytes changed while its surface cannot have
is not evidence of anything, and saying so explicitly (a document that changed **is** counted, in
`other_changed`) is what keeps the fast path from being a fast *wrong* path.

The digest rung is the second stage: a rename of a private helper, a comment moved into a signature,
a body edit that touches a surface file — all of them force extraction, and then every package's
item digests come out equal, and the answer is "the surface did not change" with the cost line to
prove it. Note which files are counted as surface-bearing: a path matched by `[api] ignore` is
reported as an exclusion in the same answer that says "no change", so the exclusion is a stated fact
rather than a silent hole.

The fourth rung exists because a clean answer from an incomplete read is the worst output this module
could produce. A file that could not be read makes `Answered::Incomplete`, and an incomplete answer
cannot be reported as "no public API change" — see §6.

## 2. Identity: which two things are the same thing

Two revisions have to be paired before they can be compared, and "the same thing" means something
different in each language. Getting this wrong produces the most confident false positives a surface
tool can generate: a moved file reported as a deleted API.

| Language | Identity | A file move is | Members live | Package is |
| --- | --- | --- | --- | --- |
| Rust | the path a consumer imports from (`shapes::geo`) | a removal **and** an addition, unless a `pub use` forwards it | on the type | the `Cargo.toml` package name |
| Python | the module path, following `__init__.py` | a removal and an addition | in the class | the package directory |
| TypeScript | the specifier, following barrels | a removal and an addition unless the barrel still forwards it | in the interface/class | `package.json` `name` |
| Go | the package | **nothing**: a file inside a package is not part of a caller's import | on the type | `go.mod` path + directory |
| Ruby | the file, for `require` | a removal (`entry_file_removed`) | — (see §5) | the gem root |
| Java | the package (package-private) | nothing a caller compiled against | in the class | the module |

Two rows in that table are the interesting ones. In Go, `Draw` moving from `shape.go` to `draw.go` is
not a moved *name* at all, because a Go caller imports the package; the diff reports only what
actually changed about `Draw`. In Rust, a move that the barrel still forwards is a move and a widened
reachability set, while a move that dropped the `pub use` is reported as
`reachability_narrowed` — nothing was deleted, and callers still break, which is precisely the class
of change a line-diffing tool cannot see.

The reachability set is computed per item: the declaring file plus every file that re-exports the
name, walking the export edges the structure model already carries. That is what makes a re-export a
forwarding fact rather than a mystery.

## 3. Export rules: what a consumer is allowed to name

Each language is read by its own rule rather than by a heuristic about naming:

* **Rust** — `pub` on the item, `pub` on the *module chain* (a `pub` item inside a private module is
  not reachable), and `pub` on the field for a struct field. `pub(crate)`, `pub(super)` and
  `pub(in path)` are carried as visibility but are not surface: they are the crate's own business.
  `pub use` is a forwarder, never an item.
* **Python** — `__all__` where it exists is the export list, exactly, in its written order; with no
  `__all__`, a name is public unless it begins with `_` — except the dunders, which are public by
  definition (`__init__` is the most-called function in most Python libraries). A function nested in a
  function is unreachable; a method nested in a class is not.
* **TypeScript** — a name needs `export` to be public. A `.d.ts` file is a declaration of an existing
  API, and an `export *` makes the scope unauditable by itself, so the package is marked with a
  caveat: the surface is at least this, and the report says the wildcard is why it cannot be more.
* **Go** — the first rune's case decides it. A lowercase `helper` is not surface however it is
  declared; an uppercase name in a package is, no matter which file it sits in.
* **Ruby** — see §5.
* **Java** — `public` on the declaration is the export rule, and a package-private member of an
  exported type is carried too, with its visibility marked `scoped`, because under unit identity
  the package *is* the unit and package-private is reachable inside it. It is never folded into
  `private`, and the report says which of the two a change was.

`[api] private = ["fathom-fixtures"]` is the last word: a private package is still extracted and still
reported, and the recommendation says `internal` rather than proposing a version for a package
published to nobody.

### A test file is not a surface (`e3.12`)

Before the export rule is applied, the file is asked a prior question: *is this file something a
caller compiles against at all?* A test file is not, and the claim is each language's rather than a
convention this tool invented.

The rule was extracted from a false positive on this repository. The api check reported:

```text
breaking test_the_gate_can_go_red_twelve_ways tests/test_release_contract.py:78
  — public item removed — fn test_the_gate_can_go_red_twelve_ways was last seen at
  tests/test_release_contract.py:78 (class: breaking, access: Symbol)
```

The function had been renamed to say what it now covered, which is the ordinary business of test
maintenance. Nothing outside that file imports it; the only thing that loads it is the runner that
collects it by name. The extractor had been reading `tests/` as an exported package because nothing
had told it not to, and the consequence of leaving it there is not one wrong line: it is the wholesale
`[api] ignore` of a quarter of the repository to silence a recurring lie, after which the four real
removals in the library go unreported too. A gate that objects to maintenance is a gate that loses its
authority.

| Language | What is declined, and why nothing can import it |
| --- | --- |
| Rust | `<crate>/tests/**` — cargo compiles each file there as its own crate; nothing can `use` it |
| Python | `tests/**`, `__tests__/**`, `test_*.py`, `*_test.py`, `conftest.py` — pytest's own collection names |
| Go | `**/**_test.go`, `tests/**` — built only for `go test` |
| Ruby | `test/**`, `spec/**`, `*_test.rb`, `*_spec.rb` — minitest's and RSpec's layouts (§5) |
| Java | `src/test/**`, `*Test.java`, `*IT.java` — not in the published artifact |
| TypeScript | `test/**`, `__tests__/**`, `spec/**`, `*.test.ts`, `*.spec.tsx` — collected by the runner |

Three edges of the rule are the substance of it, and each is tested:

* **The unit is the file.** A test file contributes no items; a source file contributes its exports as
  before. No per-symbol guess about what "looks like a test" is made.
* **The name convention is consulted only where the runner consults it.** pytest collects `test_*.py`
  files, so a `test_*` callable *inside* one is a case; a function named `test_square` in
  `src/recommender/testsurface/corpus.py` is a function with a test-shaped name in a module no runner
  will collect, and it stays on the surface. This repository keeps sample corpora shaped exactly like
  that on purpose, and a rule about identifiers instead of about callers would have eaten them.
* **`tests` is a directory marker; `test` is only sometimes one.** Node, Ruby and Maven put tests in
  `test/`, and Python puts published API there — `django.test` is documented, supported surface, and
  a rule that swallowed it is the opposite of this one. So Python gets the plural, plus the file
  names.

The exclusion is **stated, not assumed**, exactly like `[api] ignore`: declined paths are counted and
named with the rule that claimed them (`tests/test_release_contract.py (test_dir:tests)`), on the byte
fast path as in the report — which is what lets the cheapest answer say *"1 test file is not surface
and was not read"* instead of silently not reading it. It is a caveat rather than a gap, because a
decision about what a surface is does not make the answer incomplete.

`[api] track_tests = true` is the way back, for the rare repository whose tests *are* the published
artefact — a compliance suite shipped as a package, a conformance suite other projects link. The
conventions are in code as `api::testpath`, next to the argument.

One deliberate duplication: `[test] globs` asks the same *shape* of question — which paths are this
repository's tests — for the coverage findings (`e2.9`), and it is answered separately here on purpose.
They are different claims with different consequences: a path can be a test for coverage purposes and
still be published API (a test-support package), and a key that silently moved both would let somebody
tuning a coverage threshold change what the api gate refuses to merge.

## 4. Breaking is a direction, not a difference

The whole classification reduces to one question: **does a caller that compiled before still
compile?** That framing is what makes a text difference and an API difference different things:

| Change | Class | Why |
| --- | --- | --- |
| `f(x: string)` → `f(x: string \| number)` | additive | the caller may pass more |
| `f(x: Shape)` → `f(x: any)` | additive | the top type accepts everything |
| `f(x: &str)` → `f(x: impl AsRef<str>)` | additive | the bound admits the old spelling |
| `f(x: string \| number)` → `f(x: string)` | breaking | some caller passed the other half |
| `f(a, b)` → `f(a, b, eps: f64)` | breaking | every existing call site must change |
| `f(a, b)` → `f(a, b, eps: f64 = 0.0)` | additive | no call site has to change |
| `describe(prefix?)` → `describe(prefix)` | breaking | an omission that used to compile does not |
| `fn area() -> Area` → `-> f64` | breaking | the caller names the type it got |
| `&self` → `&mut self`, `(r *Rect)` → `(r Rect)` | breaking | every holder of a shared reference |
| a field, member or variant removed | breaking | it was nameable |
| a struct gains a field | additive | unless construction was exhaustive — the caveat says so |
| `const MAX: u32 = 4` → `= 5` | neutral | the name and type held; the value is behaviour, not contract |
| `type Id = u32` → `= i64` | breaking | every user of the alias sees it |
| `#[deprecated]` arrives | deprecating | its own class: a change a reviewer reads rather than compiles |
| `impl Drop` removed from a type | breaking | something relied on the capability |

Parameters are paired by name where names are what call sites use (Python keywords, TypeScript
properties) and by position where they are not, and the choice is stated in code as a name-overlap
test rather than guessed. A language whose call sites pass by position cannot see a rename as anything
but a rename; a language whose call sites pass by name sees a rename as a removal and an addition,
which is exactly what it is to the caller.

Every change carries **all** the reasons found, each with the source's own text in the note (`was
\`f64\`, now \`Option<f64>\``), and the class is the worst of them. A reviewer who disagrees with the
ranking can still see the reasons; a reviewer given only a verdict cannot check anything.

## 5. Ruby is a file, and saying so is the answer

Ruby has no stable symbol surface a static reader can claim: a method may be written by
`define_method` over a list, a class may be produced by a library call, and `require` is a runtime
statement. So the Ruby surface is the set of files an entry point loads — which is exactly what
`require "gem"` promises a consumer: this file, and everything it loads.

| Ruby claim | Class |
| --- | --- |
| a loadable file disappeared | breaking (`entry_file_removed`) |
| a loadable file appeared | additive (`entry_file_added`) |
| `lib/gem.rb` stops loading a file it used to | breaking (`loads_shrank`) |
| `lib/gem.rb` loads an extra file | additive (`loads_grew`) |
| `lib/gem.rb`'s own contents changed | neutral (`file_contents_changed`), and reported as the file, not as symbols |

That row is the honest shape of the question: the claim is one a static reader can *make*, and the
report never dresses a file-level claim up as a symbol-level one. Java's package-private visibility
gets the same treatment at the other end of the spectrum: it is carried as its own class rather than
folded into "private".

## 6. What a parser cannot know, and where that is written down

Three holes are structural, not bugs, and each has a place in the output rather than a silence:

* **Synthetic scopes.** A `macro_rules!`, a `#[derive]`, a `@dataclass`, a decorator that rewrites a
  class body, an `export *` — these write declarations a parser never sees. [Structure
  extraction](structure-extraction.md) measures the cost: every one of the six grammars loses
  declarations this way. So an item whose scope cannot be enumerated is marked synthetic, and a
  synthetic item that **vanishes** is reported as `may_have_shrunk` (indeterminate) — never as a
  removal. The rule that makes this safe is airtight in one direction: a synthetic scope can only
  *add* names, so an addition inside one is a real addition, and only a disappearance is uncertain.
* **rustdoc JSON.** Where the build produced rustdoc JSON it is fed in, and it is treated as
  add-only: it can name an item the parser missed, and can never remove one. That is what lets a crate
  with heavy macro use report an exact surface where it can and say "at least this" where it cannot.
* **Truncation and unread files.** A package above `[api] max_items_per_package` says it is
  truncated rather than growing without limit; a file that could not be read is a `gap` and forces
  `Answered::Incomplete`. There is a hard rule here: **no fast path may fire over a gap** — an
  incomplete read never produces "no public API change".

*Caveats* and *gaps* are different lists on purpose. A caveat is a limit on the claim ("this package
forwards names with `export *`, so the list is a lower bound"); a gap is a hole that made the answer
incomplete ("`src/legacy.rs` could not be read"). The first belongs in the sentence, the second in the
list a reviewer acts on.

## 7. The recommendation is one change, named

A bump with no reason attached is a guess with a version number on it. So each package's recommendation
carries the counts it rests on and **one driver**: the worst-class change, with its `file:line`, its
reason code, and the episode that made it.

```text
major: breaking — driven by gone, src/geo.rs:10 — item_removed, `pub fn gone() -> u8` was last
seen at src/geo.rs:10 (fn gone() -> u8); would have been smaller if: deprecate it for a release
before removing it, so callers have a version that warns
```

The "would have been smaller" clause is not decoration: a required parameter could have had a default,
a removal could have been a deprecation first. It is a sentence only where a cheaper spelling
actually exists — a removal has none of its own, and the sentence does not invent one. And a change
whose class is `indeterminate` blocks the "no change" claim without accusing anybody, which is the
only correct answer available when the parser is the one that blinked.

Attribution is the episode's, not the diff's: a change is credited to the episode that last touched
its `file:line`, by the same lane the graph and the layering delta ride (`episode.rs`). Where a lane
has no episode for the line — a squashed history, a rebase — the change says so rather than guessing
at the neighbour.

## 8. Configuration

```toml
[api]
mode = "warn"                        # off | warn | deny (`deny` is what `fathom check` reads)
ignore = ["**/tests/**", "crates/fathom-cli/**"]
package = ["crates/fathom-core = fathom-core", "sdk/ts = @acme/sdk"]
private = ["fathom-fixtures"]
max_items_per_package = 20000
track_tests = false                  # the built-in test table; `true` tracks tests as surface
```

A workspace of nine crates usually has two whose `pub` items are somebody else's contract. Reporting a
breaking change in the third is noise, and noise is worse than nothing: the four ignored paths are the
reason the one that mattered was scrolled past. Each knob is refused with a line and a reason when
written wrong — `mode = "warnn"` names the line and suggests the right spelling — because a policy
silently ignored is a policy the reviewer believes they have. `off` is reported as "not examined",
which is deliberately a different answer from "examined and clean": a gate has to be able to tell them
apart. And `ignore` (what *this* owner excludes) is kept distinct from the built-in test table (what
*this language* excludes, §3) in every count, so a report can never read as "nothing was excluded"
because a third of the tree was declined by a rule rather than by a line in a file.

## 9. What is not here

No `fathom api` verb of its own, no ratchet baseline of API debt, and no cross-revision cache: the
surface is rebuilt per comparison and the digests are the cache key when one is built. No body
analysis (a body change is not an API change, and no rule here pretends to know when one is). No
`Patch` bump: nothing in a source-level reader can distinguish "documentation only" from "nothing",
and `Bump::Patch` says so rather than inventing a rule. No call-graph reachability: the surface is what
a consumer can *name*, not what a program can reach. And no claim about a language whose grammar is
not compiled in: a missing grammar is a gap that names itself, same rule as
[structure extraction](structure-extraction.md) applies.
