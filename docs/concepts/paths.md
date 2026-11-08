# Path taxonomy and noise policy: what a path *is*, how noisy it is, and which rule said so

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-5vz` (`e0.9`)
- **Implements**: the glossary entries [`Path taxonomy`](thesis.md) and
  [`Noise / noise suppression`](thesis.md) — plus ADR-002 §9 (team 5 owns `src/taxonomy/` and
  `src/noise/`), ADR-004 §4.3 (`git_commit_file.path_class`, `.noise_class`, `.taxonomy_version`),
  ADR-004 §5.5 and §6 (the `taxonomy.path` analyzer, its config keys, and what a table change
  invalidates), and [the configuration contract](config.md) whose `noise.globs`, `test.globs` and
  `taxonomy.classes.*` keys are the user's half of the table
- **Executes as**: `src/recommender/taxonomy/` (`classes.py`, `noise.py`, `rules.py`,
  `builtins.py`, `engine.py`, `explain.py`, `corpus.py`, `cli.py`),
  `tests/test_taxonomy_contract.py`, and `python3 -m recommender.taxonomy …`

The built-in table's digest is pinned in the test suite. A pattern change without a version bump
fails CI, on purpose: see [§9](#9-cost-ownership-and-what-this-document-refuses-to-do).

---

## 1. What this decides

One question, answered once per path, in one place: **what kind of file is this, and does its
content carry reviewable signal?**

Before this document the answer existed twice. `src/recommender/store/git.py` (the `e0.4`
storage fixture) had a twelve-line `startswith` list that filled `path_class`, and
`fathom.toml` had `noise.globs`. Two lists that can disagree is the state this ticket exists to
end: the store's list said `dist/` was generated, `noise.globs` said it was noise, and neither
could say which rule won. Now there is one table, the store fixture asks it, and `noise.globs` is
an *input* to it rather than a rival.

The consequences worth writing down:

1. **Nothing downstream encodes its own glob list.** `e2.1` (`reco-dsh`, the change classifier)
   consumes `Taxonomy.classify`; it does not get to have an opinion about what `vendor/` means.
   Neither does the store, the diff renderer, nor the TUI. A detector that needs a path
   predicate asks the taxonomy, and if the taxonomy has no rule it gets `other` and says so.
   `e2.1` built on top of that rule rather than beside it — its content evidence sits above the
   table and can never outrank a configured assertion, which is the subject of
   [change-classification.md](change-classification.md) §2.
2. **Every answer carries receipts.** A decision is a `Verdict`: the class, the noise level, the
   rule that won, the pattern that actually matched, the layer that declared it, the file and line
   it came from, and every rule that lost with the reason each one lost. "The classifier said
   generated" is not a sentence a reviewer can act on; the glossary's noise clause ("the rule that
   hid it is printed inline") is a promise about output, and §5 is where it is kept.
3. **An unknown path is `other`, not `source`.** `e2.1`'s acceptance criterion says an
   unclassifiable file must not be silently counted as code. The fallback rule
   (`fallback.unclassified`) is a real rule with a real id, so `explain` prints it like any other
   and a recurring `other` in a repository is legible as *a missing rule in this table*.
4. **Noise is a view, never a deletion.** Nothing here drops a file from a diff. The taxonomy
   returns a level and a score; the renderer decides what to fold, and the rule is printed beside
   it. `noise_share(paths)` is the line-weighted fraction the risk score consumes (`e2.2`), not a
   filter.

## 2. The two vocabularies, and why there are two axes

**Class** — what a path *is*. Twelve names, eleven from the glossary plus the fallback:

| Class | Default noise | Diffable | Reviewable | Meaning |
|---|---|---|---|---|
| `source` | informative | yes | yes | code a human wrote or is expected to read as logic |
| `test` | informative | yes | yes | the test surface: suites, specs, fixtures, benches, stories |
| `docs` | informative | yes | yes | prose a reviewer reads: markdown, rst, adoc, RFCs, ADRs, changelogs |
| `config` | informative | yes | yes | build, tool, CI and runtime configuration |
| `schema` | informative | yes | yes | interface contracts: OpenAPI/JSON Schema/GraphQL/protobuf/Avro/thrift |
| `migration` | informative | yes | yes | ordered, append-only schema history |
| `generated` | generated | often | rarely | derived from something else; editing it loses the next run |
| `vendored` | vendored | no | no | somebody else's code, in-tree |
| `lockfile` | lockfile | yes | rarely | resolved dependency state, machine-rewritten |
| `asset` | informative | no | visually | media and data a review looks at rather than reads |
| `binary` | informative | no | no | not text; a diff is meaningless |
| `other` | informative | yes | yes | nothing matched — an unknown, not a guess |

The table above is `CLASS_INFO` in `classes.py`; the columns are read by `explain`'s footnote and
by nothing else, which is why they live beside the definitions instead of in a config file.

**Noise** — how much of this change is signal. Six levels, and the score is the *risk input*, not
a threshold:

| Level | Score | Claimable by a path rule? | Meaning |
|---|---|---|---|
| `informative` | 0.00 | yes | reviewable signal |
| `generated` | 0.85 | yes | derived from something else |
| `lockfile` | 0.80 | yes | resolved dependency state |
| `reformat` | 0.90 | **no** | whitespace-only change — content evidence, `e2.2` |
| `vendored` | 0.95 | yes | someone else's code |
| `unknown` | 0.75 | yes | noise because a rule said so, with no named category |

`reformat` cannot be claimed by any path rule, and that is not an oversight: no path tells you a
change is whitespace-only. It is in this vocabulary because `git_commit_file.noise_class` stores
it, and the six names here are asserted in the tests to be *exactly* the store's `CHECK` list, in
order — a seventh level here or there fails CI rather than failing an insert at 02:14 during a
rebuild.

**Why two axes instead of one label.** A user's `fathom.toml` may say `.plan/plan.json` is noise
without claiming to know what class of thing it is; the class axis then still answers `config`. A
jest snapshot under `tests/` is test surface *and* machine-written. A single label would have to
average those two claims and would be wrong in both places. So each axis picks its own winner from
the same set of matched rules ([§4](#4-precedence-two-orderings-not-one)).

## 3. The table

A `Rule` is `{id, patterns, class?, noise?, note, layer, source, origin, seq}`. One rule may carry
many patterns, because `gen.dir` covering `dist/`, `build/`, `target/`, `out/` and `storybook-static/`
is one decision with five spellings, not five decisions.

- **102 built-in rules, 533 patterns**, in `builtins.py`, grouped by provenance: language test
  conventions, generated-code conventions, lockfiles per ecosystem, vendor directories, schema
  and migration conventions, media and binary extensions, and the configuration surface.
- Every rule has a stable `id` (`gen.protobuf`, `dep.vendor`, `lock.node`, `mig.dir`, …) because
  `explain` prints it and a user must be able to grep for it.
- **Deliberate omissions are documented in `builtins.py`.** `*.d.ts` is not `generated` (many are
  hand-written), `*.obj` is not `binary` (it is a Wavefront `.obj` mesh *or* a compiler
  intermediate — the taxonomy will not guess), `policy/*.rego` is not `source` by path alone. A
  gap that is written down is a gap somebody can fill; a silent one is a bug report.
- Built-ins live at layer `defaults`, so *any* configuration can move them.

A pattern is a glob in `e0.8`'s dialect (`config/globs.py` — `*`, `**`, `?`, character classes,
`!`-negation), matched against the normalised relative path. No new glob language, no regexes
authored by hand, and the same semantics as every other glob in the product: `dist/` and
`dist/**` both match the directory itself.

### 3.1 Specificity

`specificity()` returns a tuple, biggest wins:

```
(literal segments, literal characters, tail literal, head literal, -magic, -segments)
```

Read it as: a rule that names more of the path is more specific; a literal tail (`.pb.go`) beats a
literal head (`dist_`); more magic is worse; a shorter pattern that said the same amount is
better. `**/vendor/**` therefore outranks `**/*.js`, and `**/*.test.jsx` outranks `**/*.jsx`,
which is most of what people mean when they say "the more specific rule should win" without
having to say it in code.

## 4. Precedence: two orderings, not one

Both keys are built by one function, and the term lists are declared in one table, so the reasons
`explain` prints cannot drift from the comparison that was actually made.

**Class axis** — `layer > specificity > declaration order`. Which surface a path belongs to is a
question about the tree, so the more specific claim about the tree wins.

**Noise axis** — `layer > derived-first > specificity > named-category > declaration order`.

- *derived-first*: a rule that says "this shape is machine-written" (`.snap`, `.min.js`,
  `Cargo.lock`) makes a stronger claim about reviewability than a rule that says "this path lives
  in `tests/`". So on this axis the derived claim goes first, and `tests/x.snap` can be test
  surface *and* noise.
- *named-category*: at everything-equal, a rule that can name *why* it is noise beats one that
  cannot. This is what stops a seed entry in `fathom.toml`'s own defaults list (`**/*.snap`) from
  out-shouting the table rule that knows it is a generated snapshot.

**Layer is first on both axes**, and layer means `e0.8`'s layer: `flag > env > repo > user >
defaults`. A repository's `fathom.toml` therefore always beats a built-in, and a user's always
beats a defaults seed — see [§6](#6-the-users-half-of-the-table).

**Declaration order** breaks a genuine tie: two patterns of identical specificity in one layer,
where the later rule wins. That is the gitignore convention, kept because the alternative
(yesterday's tie broken by today's dictionary order) is worse. `Rule.seq` carries it, and
`explain` names it as the reason when it is what decided.

**`!` is a veto, not a rule.** A negated noise glob that matches a path withdraws noise claims from
its own layer and below; the class answer is untouched. That is what makes
`globs = ["docs/reports/**", "!docs/reports/curated.md"]` mean "and not this", and it is
layer-scoped on purpose: a repo's exception cannot be undone by a user's glob, because a repo
outranks a user here. The cancelled rules stay in the receipts — "these three globs called it
noise and a `!` you wrote cancelled them" is the half of the answer a reader needs.

## 5. `fathom explain <path>`

Two verbs, one engine, one answer. `fathom config explain --path P` answers "which configuration
decided something about this file" and appends the taxonomy's verdict; `fathom taxonomy explain
P…` prints the verdict and its full receipts for one or more paths.

```
$ python3 -m recommender.taxonomy --repo . explain src/App.test.jsx

path    src/App.test.jsx
CLASS  test — The test surface: suites, specs, fixtures, benches, stories
  matched   **/*.test.jsx              test.js                        test      defaults  built-in taxonomy table v1
            rule note: —
  beaten by 1 rule:
            **/*.jsx                   src.js                         source    defaults  built-in taxonomy table v1
            lost: specificity: `**/*.test.jsx` says more than `**/*.jsx`
  NOISE  informative (0.00) — reviewable signal — same rule as above
  agrees (1): src.js **/*.jsx
  diffable  yes · reviewable yes
```

That is acceptance criterion 2 in full: **the matching rule** (`test.js`, and the pattern that
actually matched, `**/*.test.jsx`), **its source layer** (`defaults`, and where a config layer
would instead print the file and line it came from), and **the runners-up**, split into the rules
that *lost* — each with a machine-checkable reason (`lost: layer`, `lost: specificity`,
`lost: derived-first`, `lost: named-category`, `lost: declaration order`) — and the rules that
*agreed*, which are not noise but confidence.

`--json` emits the same content structurally (`class_rule`, `class_matches[]`, `noise_matches[]`,
each with `rule`, `pattern`, `layer`, `source`, `outcome`), because a TUI must not parse prose. The
renderers are text on purpose and are tested as text, in the style of
`recommender.config.explain`: a diagnostic nobody can read is a diagnostic nobody can test.

Other verbs: `classes` (the vocabulary table, with what the running configuration adds to each
class), `rules [--class C]` (the effective table, built-ins and user rules together), `share
PATH…` (the line-weighted noise share, the number `e2.2` feeds to risk), and `bench` (see §9).

## 6. The user's half of the table

Everything in `fathom.toml` that can move a decision, with `e0.8`'s merge semantics (each key
`extend`s, so a repo file adds to the table rather than replacing it):

| Key | Effect | Merge |
|---|---|---|
| `taxonomy.classes.<class>` | assert these paths are this class | extend |
| `test.globs` | shorthand for `taxonomy.classes.test` | extend |
| `noise.globs` | demote these paths to noise (`unknown`), `!` to re-promote | extend |
| `noise.builtins_version` | which built-in table version this repo is written against | replace |

`taxonomy.classes.<class>` is an open map keyed by the class names in §2, and it is validated:
`[taxonomy.classes] documentation = [...]` produces
`warning[unknown_key]: unknown key 'taxonomy.classes.documentation' … suggestion:
'taxonomy.classes.<class-name>'`, and the engine checks again for a resolved dict that never met
the schema. A made-up class name is never quietly ignored.

**Promotion and demotion are the same mechanism, pointed both ways.** A class assertion outranks
the built-in because it is at a higher layer, not because it is newer:

```toml
# vendor/ is vendored by default. Our fork is ours, so it is source:
[taxonomy.classes]
source = ["vendor/our-fork/**"]        # promote: vendored → source

# …and everything under src/protos is codegen, whatever it looks like:
generated = ["src/protos/**"]          # demote: source → generated

[noise]
globs = ["docs/reports/**",           # demote to noise, reason unnamed
         "!docs/reports/curated.md"]   # …except this one: re-promoted to informative
```

A `!` in a *class* list is a user error rather than a no-op: a class list has nothing to
re-promote to, so the engine warns (`taxonomy_negation_ignored`) and says what to use instead.

## 7. Where the answers are stored

`taxonomy.path`, one of ADR-004 §5's analyzers, is the durable form of a classification:

- `git_commit_file.path_class`, `.noise_class`, `.taxonomy_version` are filled from the verdict
  (`store/git.py` calls `classify_path(path, taxonomy)`; the fixture's own list is gone).
- The analyzer payload carries the table `version`, the table `digest`, and per path the class,
  level, and **rule id** — so "why did this file look unimportant in March" is answerable after
  the table has moved on.
- The cache key declares `noise.globs`, `noise.builtins_version`, `test.globs` and the
  `taxonomy.classes.` prefix (ADR-004 §5.5: a key must name everything the payload reads), and
  its version went 1 → 2 with this ticket because the payload's contents changed.
- ADR-004 §6 lists "path taxonomy table change" as an invalidation event. Its mechanical form is
  the digest pin in [§9](#9-cost-ownership-and-what-this-document-refuses-to-do).

## 8. Adding a row, and what happens when you do

The decision table lives in the package (`corpus.py`), not only in the test file, because `bench`
and `explain` also want real paths: **235 paths across 24 well-known repository shapes** — Rust
workspace, Django, Rails, Go monorepo, React/TS monorepo, Python data science, Kubernetes/IaC,
JVM/Gradle, .NET, Flutter, Rust+protobuf gRPC, C++/CMake, Ruby gem, Elixir/OTP, docs-only, and
ten more. Every one of the twelve classes is exercised; every row is asserted against the
**built-in table with no configuration**, so a failure is a defect in the table and never in
somebody's `fathom.toml`.

To add a rule: add the rule to `builtins.py`, add the path with its expected `(class, noise)` to
`corpus.py`, run the suite. If you changed an existing pattern instead, the digest pin will fail:

```
test_builtin_table_digest_is_pinned_to_its_version
  AssertionError: assert '9f2c…' == '726290bf9c166d1d'
```

That is the honest moment. The two options are "revert it" and "bump `BUILTINS_VERSION`, and
accept the re-analysis it buys" (ADR-004 §6: a table change cascades everything downstream of
`taxonomy.path`). What is not available is changing the table quietly.

## 9. Cost, ownership, and what this document refuses to do

**Cost.** `classify` is O(depth + extensions + segments + |broad|) dictionary probes against an
index built once, then a glob match against the surviving candidates. No regex is compiled in the
hot path, and no path pays for the whole table:

| Measure | Value (`bench`, this repo, `--repeat 4`) |
|---|---|
| cold lookup (memo defeated) | ≈19 µs → 1 000 changed files in ~19 ms |
| memo hit | ≈0.17 µs |
| index probes per lookup | ≈12.6, of 533 patterns in the table |
| buckets | exact 0 · prefix 5 · dirseg 66 · suffix 448 · **broad 11** |

The `broad` bucket is the one linear scan — patterns with no literal suffix to key on, like
`**/Dockerfile*`. The suite asserts `broad ≤ 16` and mean probes ≤ 40, because "cheap per changed
file" is the acceptance criterion and a table that grows to 200 broad patterns is 200 regex
matches per file, which is exactly the blowup the criterion names. The memo is per-path and
bounded (`MEMO_LIMIT = 8192`), because a long-lived daemon's RSS must not track repository size
(ADR-004 §11.5).

One more bound, because the broad bucket is paid by every path: a `Glob` reads the literal runs its
pattern insists on (`**/*.egg-info/**` insists on `.egg-info/`) and refuses a path missing one before
any engine runs. That matters most for the pattern too wildcard-dense for `re` — the state machine
that stands in for it was four fifths of a cold lookup on this corpus, answering "no" over and over,
and the per-file budget went red on a CI runner that is slower than the one that wrote this table
(`reco-ghao`). `Glob.required` is that list of literals, and
`tests/test_config_contract.py` checks both that it says something for every pattern the broad bucket
carries and that it never refuses a path either engine accepts.

**Ownership.** ADR-002 §9 puts `src/taxonomy/` and `src/noise/` with team 5 (analyze-change). The
`noise` axis lives in `taxonomy/noise.py` rather than a separate module for one reason: a level and
a class are chosen by the same rule at the same moment, and splitting them would recreate the
two-lists problem one directory over. The Rust crate split stays as ADR-002 drew it
(`fathom-analyze/{taxonomy,noise}`); this is the Python contract fixture.

**Refused.**

- *A learned classifier.* No path heuristic gets to be a black box here: the acceptance criterion is
  explainability, and an unexplainable "is this noise" directly drives how much attention a human
  is asked to pay. Rule table now; if `e2.1` later wants a model *feature*, it consumes these labels.
- *Content-sniffing for the class axis.* Shebangs and NUL bytes are cheap, but a file that *looks*
  generated and is hand-maintained is exactly the case a user must be able to overrule, and a path
  rule is overrulable in a way a sniff is not. Content evidence arrives in `e2.2` as `reformat` —
  the one level that needs it, on the one axis that can use it.
  **How this refusal survived `e2.1`**: the classifier does read bytes, but it reads them in its
  own layer, above the table, and `policy` ranks a rule from a config layer above every piece of
  content evidence it has — so "overrulable by a person" stays true, and the table itself is
  still path-only. See [change-classification.md](change-classification.md) §1–§2; the property
  is asserted by a test, not promised here.
- *A per-analyzer glob list.* This is the refusal the rest of the document is about.
- *Deleting noise.* Suppression is a view. See §1.4.

## 10. The executable half

```bash
python3 -m recommender.taxonomy --repo . explain dist/app.min.js     # receipts for one path
python3 -m recommender.taxonomy explain -q src/a.rs dist/x.js        # one line each
python3 -m recommender.taxonomy explain --json src/a.rs              # for a TUI, not for eyes
python3 -m recommender.taxonomy classes                            # the vocabulary, as configured
python3 -m recommender.taxonomy rules --class lockfile             # the effective table
python3 -m recommender.taxonomy share src/a.rs Cargo.lock           # the risk input
python3 -m recommender.taxonomy bench --repeat 8                   # the numbers above
python3 -m recommender.config --repo . explain --path src/foo.rs   # config chains + verdict
python3 -m pytest tests/test_taxonomy_contract.py                  # the contract, as tests
```

`tests/test_taxonomy_contract.py` is the acceptance criteria in order: the vocabularies, the
decision table (AC 1), `explain` (AC 2), promote/demote across every layer including `!` (AC 3),
the cost bounds and the digest pin (AC 4) — then the cross-contract work: the noise vocabulary
against the store's `CHECK` constraint, the analyzer's declared keys against what the engine
actually reads, `store/git.py` having no glob list of its own, `fathom.toml`'s seed globs agreeing
with the table they predate, and this repository explaining itself with zero warnings.
