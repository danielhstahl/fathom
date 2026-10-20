# Test surface delta and coverage proxy: the testing contract per episode, in names and locations

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-6pl` (`e2.9`)
- **Implements**: the plan's `e2.9` acceptance criteria; ADR-002 §9 (team 5 owns
  `fathom-analyze::testsurface` — this is the fourth analysis epic 2 builds on, after
  `e0.9`'s path half, `e2.1`'s classifier and `e2.2`'s noise half); the thesis's rule that a
  metric a reviewer cannot audit is a metric that will be gamed ([thesis.md](thesis.md) §4,
  §7); and [change-classification.md](change-classification.md), whose per-path `test` /
  `source` label is one of the inputs to every number here.
- **Executes as**: `src/recommender/testsurface/` (`scan.py`, `parsers.py`, `public.py`,
  `diff.py`, `policy.py`, `delta.py`, `render.py`, `explain.py`, `corpus.py`, `eval.py`,
  `cli.py`), `tests/test_test_surface_contract.py`, and `python3 -m recommender.testsurface …`.
  The Rust home is `fathom-analyze::testsurface` (team 5), with the card in
  `fathom-cli::verbs::testsurface`.

---

## 1. What this decides

One question per episode, and it is not "what is the coverage percentage":

> **Did this episode honour its testing contract, and can you point at the lines that say so?**

Four claims answer it, and each is computed from one pair of revisions and nothing else:

1. **The split.** How many of the lines this episode *added* are test lines and how many are
   production lines — the ratio **and both absolutes**, always together.
2. **The cases.** Which named tests appeared, disappeared, started being skipped, stopped
   being skipped, were focused, or had their assertions weakened — per language, with the
   declaring `path:line`.
3. **The gap.** New public logic that no test in the episode covers, against a definition
   whose every word is a config key ([§7](#7-new-public-logic-no-new-test-the-tunable-definition)).
4. **The honesty.** What fraction of all that is actually *measured* rather than counted, and
   a sentence saying which it is ([§9](#9-degradation-and-stated-confidence)).

The ticket's framing is worth keeping in sight, because it is the reason this is not a
coverage number:

> Track the testing contract per episode, not raw coverage percentages.

Coverage percentages are a property of a repository at a moment, are almost always read as a
grade, and are produced by a tool the review pipeline does not run. An episode-level test
delta is a property of *the change under review*, is produced from the same two blobs the
rest of the card is produced from, and fails in the direction a reviewer can check: "you
deleted `test_roundtrip_overflow`, and here is the line it was on".

Three refusals follow, and all three are load-bearing:

- **No framework is never zero tests.** A repository of Ruby, Kotlin or CMake has no parser
  here; it gets a line-count proxy with a stated confidence of 0.3, not a `0` that reads like
  a fact ([§9](#9-degradation-and-stated-confidence)).
- **A ratio is never printed alone.** `0.02` from `+4 test / +180 prod` and `0.02` from
  `+400 / +19 600` are different episodes; the absolutes ride in the same row and the
  auditor fails a render that drops them.
- **A finding with no location is not a finding.** Every finding carries a `path`, and a
  `Finding` constructed without one raises ([§10](#10-the-card-and-the-auditor)).

---

## 2. What a test line is

The unit is a **line**, attributed to one of three surfaces: `test`, `production`, `other`.
Attribution is per *line*, not per file, because the interesting cases are exactly the ones
where a file is not one thing:

| Situation | Rule |
| --- | --- |
| path classified `test` by `e0.9`/`e2.1` | every changed line is a test line |
| path classified `source`, but a Rust test module lives inside it | lines inside `#[cfg(test)] mod … { … }` are test lines, lines outside are production lines |
| path classified `config`, `docs`, `vendor`, `lock`, `generated` | counted as `other`, so they cannot move the ratio in either direction |
| a file the classifier is unsure about | attributed by its winning class, with `class_confidence` kept on the row |

The Rust case is why line-level attribution exists at all. Rust tests are *inside* the file
they test, so a path taxonomy — however good — calls `crates/store/src/snapshot.rs` a source
file, and a whole test module inside it would be counted as production code. The parser
finds those regions, and they outrank the path label:

```
    note: 7 added line(s) attributed to test code inside the file (`25–47`), not to
    production: the parsed test module outranks the path label here
```

Ranges are kept on the row (`FileSurface.regions`) rather than collapsed into a total, so the
card can say *which* lines it moved between columns and so `e2.10` can point the diff viewer
at them.

Deleted lines are counted too (`test_lines_deleted`, `prod_lines_deleted`) but never enter
the ratio: the ratio is about what the episode **added**, because that is the part a reviewer
is being asked to accept.

---

## 3. The ratio, always with its absolutes

```
ratio = test lines added / (test lines added + production lines added)
```

`other` is out of the denominator by design: a commit that regenerates a 4 000-line lockfile
must not look like an episode that skipped its tests. When the denominator is empty the ratio
is `None`, not `0` or `1` — "nothing was added" is a different fact from "nothing was
tested", and the card says so in words (`no test or production lines added`).

The same fact is printed in three shapes, because reviewers scan differently:

| Shape | Example | Where |
| --- | --- | --- |
| quotient | `0.222` | summary row, ratio row, JSON `ratio.ratio` |
| test-to-production | `1 : 3.5` | ratio row, JSON `ratio.test_to_prod` |
| words | `0.29 test lines per production line` | headline row |
| absolutes | `test 4 / prod 14 lines added` | ratio row, always |

The JSON keeps `basis` on the ratio object: *lines added, attributed by taxonomy class,
Rust `#[cfg(test)]` regions counted as test*. A ratio whose denominator nobody can name is
decoration.

---

## 4. Named cases: the parser table

A **case** is the thing a developer would name in a stand-up: a `#[test]` function, a
`def test_*`, a `TestXxx`, an `it(...)`. Cases are identified by **qualified name**, never by
line number — a case that moved to a new file is the same case, and a case identified by line
number turns a reformat into a deletion plus an addition.

| Language | Extension | Frameworks recognised | What counts as a case |
| --- | --- | --- | --- |
| Rust | `.rs` | `std::test`, `tokio::test`, `test-case` | `#[test]`, `#[tokio::test]`, `#[test_case(…)]` rows (each row is its own case, named `fn#k`), qualified by the enclosing `mod` |
| Python | `.py`, `.pyi` | `pytest`, `unittest`, `hypothesis` | `def test_*` and `Test*` methods, parsed with the stdlib `ast` |
| Go | `.go` | `testing`, `testify` | `TestXxx`, `BenchmarkXxx`, `FuzzXxx`, `ExampleXxx`; `t.Run` sub-tests are evidence on the parent, not separate cases |
| TypeScript / JavaScript | `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs` | `jest`, `vitest`, `playwright`, `mocha`, `node:test`, `tape` | `describe`/`it`/`test` including `.each` rows, `it.only`, `xit`, `test.skip` |
| Java | `.java` | `junit5`, `junit4`, `assertj` | `@Test`, `@ParameterizedTest`, `@Disabled` |

Two rules about what happens at the edges:

- **A language with no parser reports `unsupported`, not `0 cases`.** The state is on the
  row, in the JSON, and in the confidence statement. "No parser" and "no tests" are different
  sentences and must never render as the same one.
- **A file that fails to parse reports `failed`.** Python's `ast` raises on a syntax error;
  the episode then degrades to `partial` and says which file refused. A parse failure is never
  silently a zero.

Skip markers are parsed in the same pass: `#[ignore]`, `@pytest.mark.skip` / `skipif` /
`xfail`, `@unittest.skip`, `t.Skip`, `t.Skipf`, `it.skip`, `xit`, `xdescribe`, `@Disabled`,
plus a `t.Skip` inside a body (a runtime skip with a reason). A case carrying a skip marker
is a case that **does not run**, which is why it is counted next to deletions and not next to
additions.

`CASES_VERSION` bumps when the parser's idea of a case changes, so a corpus label and a
parser can never be silently out of date with each other.

---

## 5. Findings: a deleted test is a first-class finding

Seven finding kinds, each with its own config switch, each rendered with a name and a
location. The first three are the ticket's "deleting or skipping an existing test is a
first-class finding, with the test name and its location":

| Kind | Config key | Severity | Fires when |
| --- | --- | --- | --- |
| `test-removed` | `test.findings.test_removed` | high | a named case that ran before does not exist now — **including the cases lost because the whole file went** |
| `test-file-removed` | `test.findings.test_file_removed` | high | a test file was deleted, printed even when each case inside it is also listed |
| `test-skipped` | `test.findings.test_skipped` | high | a case that used to run is now skipped or `xfail`-marked; a case that was *added* already-skipped is reported as such |
| `assertion-weakened` | `test.findings.assertion_weakened` | high | a comparison became an existence check, on the same subject ([§6](#6-assertion-strength-and-subject-matching)) |
| `assertion-removed` | `test.findings.assertion_removed` | notable | a surviving case lost exact assertions and gained none |
| `untested-public` | `test.findings.untested_public` | notable | new public logic with no covering test ([§7](#7-new-public-logic-no-new-test-the-tunable-definition)) |
| `focused-test` | `test.findings.focused_test` | notable | `it.only`, `fit`, `fdescribe`, `test.only` in committed code |

Three properties are asserted by the contract suite rather than promised:

- **A moved test is not a removed test.** Renaming a file, or lifting a case into a shared
  suite, produces no `test-removed` row: identity is the qualified case name, so the pair
  comparison sees the same key on both sides.
- **A test deleted *and* replaced in the same episode is still reported.** The replacement is
  an `added` row; the deletion is a finding. A reviewer wants to see both halves of a swap,
  including a swap between frameworks (`#[test]` → `#[test_case]`), which is a real refactor
  and shows up as one removal and one addition per row.
- **A macro swap that loses coverage is visible as weakening.** Turning four table-driven
  rows into one `assert!` prints the four additions, the one removal, and every weakening.

`test-file-removed` deliberately overlaps with `test-removed`. Redundancy that costs a row is
cheap; a reviewer who has to scroll to the last case to learn *the suite is gone* is not being
helped.

---

## 6. Assertion strength, and subject matching

"An assertion was weakened" is only checkable if an assertion is a *thing*, so each parsed
case carries an assertion census: `(kind, strength, subject)`.

| Strength | Meaning | Examples |
| --- | --- | --- |
| `exact` | compares the subject against an expected value | `assert_eq!(a(), Some(42))`, `assert x == 1`, `assert.Equal(t, got, 3)`, `expect(x).toBe(y)`, `assertEquals(a, b)` |
| `presence` | the subject exists, is truthy, or was called | `assert!(a().is_some())`, `assert x`, `assert.NotNil(t, got)`, `expect(x).toBeDefined()`, `assertNotNull(a)` |
| `message` | no subject at all | `assert!(true)`, `assert.NoError(err)` |

A **weakening** is a case that survived the pair with an assertion whose subject is unchanged
but whose strength fell: `exact → presence`, `presence → message`. Subject matching is why the
finding is trustworthy — `assert_eq!(report(), Some(42))` and `assert!(report().is_some())`
both have the subject `report()`, so the pair is a weakening, and *not* a removal plus an
addition. Subjects are normalised (whitespace, `self.`, a trailing `()`, a `.clone()`), so
`report()` and `report().clone()` match.

Because matching is subject-based, an assertion the tool has never heard of does not invent a
finding: unrecognised assertions are still counted in the census, and a case that loses
comparable assertions without a matched pair raises `assertion-removed` instead. That finding
is deliberately softer — sometimes the deleted assertion was the one that was wrong.

---

## 7. "New public logic, no new test": the tunable definition

The ticket asks for a *documented and tunable* definition of this phrase. Here it is, in the
order the decision uses it; `fathom testsurface definition` prints it back with the numbers
actually in force.

1. the changed path is `source` (the `e2.1` class) and is not matched by
   `test.untested.exclude`;
2. the symbol is **public** — `pub`, an initial capital, `export`, a name not starting with
   `_`, per language, plus `test.untested.visibility` for Rust's `pub(crate)`;
3. the symbol is **new**: present in the new revision, absent from the old — identity is
   language plus name, so *renaming* a function reads as a new symbol (which is what a
   reviewer wants a rename to do, and the reason the finding is `notable` rather than `high`);
4. it is **logic**: at least one body line that calls, assigns, branches, loops or returns
   (`test.untested.require_logic`), and at least `test.untested.min_body_lines` body lines;
5. and **nothing covers it** ([§8](#8-coverage-the-three-ways-an-episode-can-cover-a-symbol));
6. scope is `test.untested.scope` — `episode` by default.

Every word of that is a key, and each key has a stated failure mode (the full text is in
`fathom testsurface policy` and in [config.md](config.md)):

| Key | Default | Turning it… | …does this |
| --- | --- | --- | --- |
| `test.untested.min_body_lines` | `4` | lower | fires on one-line re-export helpers; higher misses small real functions |
| `test.untested.require_logic` | `true` | off | a new `pub struct` or constant can raise the finding — what a schema-review repository may want |
| `test.untested.visibility` | `public` | to `crate` | `pub(crate)` counts as public, for crates that treat crate-internal as their surface |
| `test.untested.scope` | `episode` | to `repo` | a symbol the repository's existing tests already mention is not flagged; needs `repo_test_mentions` supplied, and the card says when it was asked for and not given |
| `test.untested.name_overlap` | `1` | higher | a new case must share more name tokens with the symbol |
| `test.untested.path_overlap` | `1` | higher | a changed test file must share more path tokens with the source file |
| `test.untested.mention_in_test` | `true` | off | the symbol's name appearing in lines this episode added to a test file no longer counts as coverage |
| `test.untested.exclude` | `**/examples/**`, `**/benches/**`, `**/fixtures/**` | — | paths the finding never fires for (`extend`, so a repository adds) |
| `test.card.max_findings` | `12` | lower | rows per card; the remainder is counted, never silently dropped |

The corpus proves the knobs are real by running the *same episode* under different settings and
asserting the finding appears under one and not the other — `type-only-with-require-logic-off`
and `body-floor-raised` are two rows of the same code with different keys, and the note on each
says which definition changed. The default stays "public plus logic plus four lines" because
that is the setting whose false positives a reviewer will tolerate once per episode; the
tunability is the point, not the value.

---

## 8. Coverage: the three ways an episode can cover a symbol

A new public symbol is **covered** if any of these hold, and the finding carries whichever one
did (or, when it fires, the receipt of each test that failed):

1. **Path**: a changed `test` file shares at least `test.untested.path_overlap` name tokens
   with the source file — `src/store/load.rs` ↔ `tests/store_load_test.rs` share `store`,
   `load`. Cheap, and right for repositories with a convention.
2. **Case name**: a *new* test case's qualified name shares at least
   `test.untested.name_overlap` token with the symbol — `load_snapshot` ↔
   `TestSnapshotLoading` share `snapshot`.
3. **Mention**: the symbol's leaf name appears in the lines this episode added to a test file
   (when `test.untested.mention_in_test` is on). The loosest test, and the only one that
   catches a new function asserted from inside a pre-existing test.

Plus a fourth, only on request: a name in `repo_test_mentions` when
`test.untested.scope = repo`.

Nothing here is a coverage *measurement*. It is a proxy with an explicit definition, and the
card says which test covered a symbol even when the answer is "no finding", because a silence
a reviewer cannot explain is indistinguishable from a bug:

```
  note: write_snapshot is covered: new test case name shares snapshot, write;
  `write_snapshot` is named in lines this episode added to a test file
```

---

## 9. Degradation, and stated confidence

Every episode says how much of its own test story was measured. Four bases, three numbers,
all three config keys:

| Basis | Default confidence | When | What may be claimed |
| --- | --- | --- | --- |
| `parsed-cases` | `test.proxy.confidence.parsed` = `0.9` | every changed test file had a parser and was parsed | case counts, weakenings, skips — the whole of §5 |
| `partial` | `test.proxy.confidence.partial` = `0.6` | some test files parsed, some had no parser or refused | case counts are a **lower bound**, stated as such, naming which files are counted |
| `lines-only` | `test.proxy.confidence.lines_only` = `0.3` | no test framework this tool can parse is present | line counts and file counts only, and a sentence saying a line count is not a coverage number |
| `no-test-change` | `1.0` | no test surface moved at all | the line counts are exact; no case-level claim is made |

The confidence number is not a decoration: it is on the summary row, it is in the JSON, the
auditor fails a render that drops it, and a repository that turns a finding off sees the
suppressed count on the card rather than silence:

```
  2 untested-public finding(s) not reported: untested-public is off (repo fathom.toml)
```

Disabling a finding is a policy decision and has to leave a trace; that is the same rule
[e2.2](noise-suppression.md) enforces for folds.

---

## 10. The card, and the auditor

`fathom testsurface card` prints one card per episode. This is the smallest honest case — a
Rust source file gaining a public function, and a Python suite losing one test and weakening
another — rendered at 78 columns, byte for byte the same on every machine and blessed in
`tests/test_test_surface_contract.py::test_the_card_is_the_blessed_text`:

```
──────── test surface · ep-1
  ep-1 — 6 test line(s) added against 24 production line(s); 0.25 test lines
  per production line
  ep-1 · +6 test / +24 prod lines · ratio 0.200 · 1 tests added, 1 removed, 0
  skipped · 1 assertions weakened, 0 lost · 3 findings · confidence 0.90
  (parsed-cases)
  ratio: 0.200 (1 : 4.0) · test 6 / prod 24 lines added (the ratio denominator
  is test + production only)
  files: 2 changed (0 created, 0 deleted, 0 renamed, 2 modified) · test files:
  0 created, 0 deleted, 0 renamed, 1 modified
  cases: +1 added, −1 removed, 0 skipped, 0 un-skipped, 0 focused ·
  assertions: 1 weakened, 0 lost, 0 added
~ src/store/snapshot.rs (source, rust)
    +24/−0 lines · 0 test / 24 prod / 0 other added
    new public fn compute_total at src/store/snapshot.rs:5 (5 body, 3 logic)
    new public struct Banner at src/store/snapshot.rs:13 (2 body, 0 logic)
    new public fn tiny at src/store/snapshot.rs:25 (2 body, 2 logic)
~ tests/test_cart.py (test, python)
    +6/−5 lines · 6 test / 0 prod / 0 other added
    pytest: +1 cases, −1, 0 skipped, 1 weakened
  findings (3)
    Assertion weakened: TestCart::test_checks · assert → assert:
    `store.get('a')` no longer compared to an expected value · at
    tests/test_cart.py:23 · [python]
      · was: assert store.get('a') == 1
      · now: assert store.get('a')
    Test removed: TestCart::test_removes · removed from pytest · at
    tests/test_cart.py:19 · [python]
      · case TestCart::test_removes
      · 1 assertion(s) went with it (1 exact)
    New public logic, no new test: compute_total · new public fn with 3 logic
    line(s), no covering test in this episode · at src/store/snapshot.rs:5 ·
    [rust]
      · fn at line 5: 5 body line(s), 3 of them logic
      · thresholds in force: min_body_lines=4, require_logic=true,
      scope=episode
      · no changed test file matched its path or name, and its name is not in
      any line this episode added to a test file
  confidence: test surface measured from named cases in python — 1 file(s)
  parsed
    · every changed test file had a parser and was parsed into named cases
    · frameworks seen: pytest
```

`fathom testsurface demo` renders the same card for one episode across four languages
(Rust inline tests, Go, TypeScript and Python at once) in seven files: an inline Rust test
module inside a source file, a deleted Python suite, a weakened Python assertion, a skipped
Go case beside a new one, a focused TypeScript case, and one new exported Go function with
real logic and no test of its own. It is what §2's in-file attribution note and §9's
multi-language confidence line come from, and the contract suite runs it.

The demo prints **every** finding row and says so in a trailing note, unlike a real episode,
which honours `test.card.max_findings` and turns the remainder into "… N more: fathom
testsurface card --all". The exception is deliberate and has a test: the demo exists to show
one finding of each kind, the two least severe kinds sort last, and a documentation card that
ended in "… 2 more" would document neither those two findings nor the cap. `--capped` renders
the same episode under the configured cap, which is what a real review sees.

The invariants, as mechanically-checked claims (`render.audit`, run by the CLI on its own
output and re-run by the contract suite):

1. if a ratio exists, the ratio **and both absolutes** are in the text;
2. every finding row carries its **name** and its **`path:line`**;
3. the confidence row is present, with the number, always — including the `lines-only` case,
   which is when it matters most;
4. findings past `test.card.max_findings` are counted and named as hidden (`… 4 more:
   fathom testsurface card --all`), never dropped;
5. suppressed findings say which key turned them off and where that key came from.

Rows **wrap** rather than clip. The earlier version of this card clipped at the pane width,
which reliably ate the location at the end of a finding row — the one token a reviewer has to
click. Long rows now continue with a hanging indent, and a test renders at 40, 60 and 200
columns asserting the name and the `path:line` survive all three.

`Finding.__post_init__` is the stricter gate: a finding with no `path` raises
`UnanchoredFinding`. An unanchored finding is not ugly; it does not exist.

---

## 11. Policy: keys and layers

Twenty keys are read: the eight `test.untested.*` knobs and `test.card.max_findings`
([§7](#7-new-public-logic-no-new-test-the-tunable-definition)), the seven `test.findings.*`
switches ([§5](#5-findings-a-deleted-test-is-a-first-class-finding)), the three
`test.proxy.confidence.*` ([§9](#9-degradation-and-stated-confidence)), and `test.globs`,
owned by `e0.9` and consumed through the classifier. They are declared in `e0.8`'s schema, so
they layer, validate and explain like every other key:

```
$ fathom testsurface policy    # planned: no `testsurface` subcommand is built yet
test surface policy (highest precedence first)
  findings
  test-removed         on    test.findings.test_removed             built-in default
  …
  definition
  min_body_lines             4          test.untested.min_body_lines             built-in default
  …
```

The flags for finding `fathom.toml` are `e0.8`'s verbatim (`--repo`, `--repo-file`,
`--user-file`, `--no-env`, `--set`, `--extend`, `--offline`), so `testsurface policy` and
`fathom config explain --key test.untested.min_body_lines` cannot disagree about which layer
won. The analysis half takes a resolved config object, never a file path —
`Policy.from_config(config)` — which is what keeps §14's purity claim true.

---

## 12. The labelled corpus, and the gates

`corpus.py` is the ground truth; `eval.py` scores it; `fathom testsurface score` gates CI on
it. Four tables:

| Table | Size | Labelled with |
| --- | --- | --- |
| parse cases | 12 | exact case names, skip markers, parser state, framework |
| pair cases | 10 | added / removed / skipped / un-skipped / focused cases and weakened assertions |
| coverage cases | 20 | which new symbols must fire and which must stay silent, with the knobs in force |
| episode cases | 6 | counts, ratio, proxy basis, stated confidence, and the phrases the card must contain |

Bars (`score`'s defaults, all assertable per invocation):

| Bar | Default | Why that value |
| --- | --- | --- |
| `--min-parse` | `0.95` | one parser's notion of a case is genuinely ambiguous (a `describe` with no `it`), so a hair of slack — and only there |
| `--min-pairs` | `1.0` | a pair case is a claim about a diff; half of it being wrong is not an accuracy score |
| `--min-precision` | `1.0` | a false "you deleted a test" is the failure this ticket exists to avoid; every silence case must stay silent |
| `--min-recall` | `1.0` | a missed deletion is the same failure from the other side |
| `--min-episodes` | `1.0` | the counts, ratio and confidence are arithmetic, not judgement; being wrong here is a bug, not a trade-off |

The corpus is deliberately unkind in four specific ways, each of them a case in the table: a
test that only **moved** (must not read as a deletion), a **macro swap** (`#[test]` →
`#[test_case]`, both halves reported), a new public symbol that **is** covered by a path or a
mention (must stay silent), and an episode in a **language with no parser** (must degrade to
`lines-only` and say so).

---

## 13. Ownership, and what this document refuses

This ticket does not:

- **run a test suite, or claim to.** Nothing here executes user code. ADR-002 §4.5: the
  analysis half is pure, and `tests/test_test_surface_contract.py` greps it for `open`,
  `os.environ`, `subprocess` and friends. Only `cli.py` touches the filesystem, and it takes
  blobs or two trees, so a test can drive every verb from strings.
- **read coverage tooling.** Importing `lcov`, `tarpaulin` or `coverage.py` output would make
  the card depend on a run that may not exist, may be stale, and is not the change under
  review. A coverage number is `e6.5`'s input, not this ticket's.
- **classify a path.** The `test` / `source` / `config` label comes from
  [change-classification.md](change-classification.md) and `e0.9`; this package reads the
  answer and never re-derives it.
- **invent a test.** `untested-public` is a prompt for a reviewer, never an auto-generated
  test, and never a gate that a repository cannot turn off.
- **guess a framework.** `cypress`, `behave`, `gtest`, `pytest` decorators it has not seen: the
  parser says `unsupported`, the proxy says `lines-only`, and the number says `0.3`.

Dependencies, declared in both directions and asserted by a test: `recommender.taxonomy`
(`e0.9` path classes), `recommender.change` (`e2.1` classifier), `recommender.config.globs`
(`e0.8`'s glob matcher, and its resolved config *object* — never the loader). That is the whole
list, and it is the same three-module list `e2.2` keeps; a package that could read the world
could not be re-run on a pair of strings by the Rust port or by a test.

---

## 14. The Rust seam

| Python | Rust home | Team |
| --- | --- | --- |
| `scan.py` | `fathom-analyze::testsurface::scan` | 5 |
| `parsers.py` | `fathom-analyze::testsurface::parsers` | 5 |
| `public.py` | `fathom-analyze::testsurface::public` | 5 |
| `diff.py` | `fathom-analyze::testsurface::diff` | 5 |
| `policy.py` | `fathom-analyze::testsurface::policy` | 5 |
| `delta.py` | `fathom-analyze::testsurface` | 5 |
| `render.py`, `explain.py` | `fathom-cli::verbs::testsurface::{render,explain}` | 5 |
| `corpus.py`, `eval.py` | `fathom-analyze::testsurface::corpus`, `fathom-cli::verbs::testsurface::score` | 5 |

Three things the port must not silently change, which is why they are data and not prose:
`CASES_VERSION` (what a case is), `PUBLIC_VERSION` (what a public symbol is) and
`SURFACE_VERSION` (the shape of the delta object `e2.10` and `e2.11` consume). The blessed card
in the contract suite is the byte-level contract; `--json` is the machine-level one, and its
`counts` dictionary is the shape `e2.10`'s card row and `e5.x`'s risk score will read.
