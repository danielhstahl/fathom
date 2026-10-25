# The change classifier: what every changed path in an episode *is*, with a confidence and a reason

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-dsh` (`e2.1`)
- **Implements**: the glossary entries [`Path taxonomy`](thesis.md) and
  [`Delta`](thesis.md) as applied per changed file; ADR-002 §9 (team 5 owns
  `fathom-analyze/src/taxonomy/` — this is the first analysis epic 2 builds on);
  ADR-004 §5.1/§5.5 (an analyzer whose cache key names everything it reads); and
  [the path policy](paths.md), whose `Taxonomy.classify` is this classifier's first input
  and whose §9 refusal it honours rather than reverses.
- **Executes as**: `src/recommender/change/` (`markers.py`, `evidence.py`, `policy.py`,
  `engine.py`, `episode.py`, `cache.py`, `corpus.py`, `eval.py`, `explain.py`, `cli.py`),
  `tests/test_change_classifier_contract.py`, and `python3 -m recommender.change …`.
  The Rust home is the same one `e0.9` named: `fathom-analyze::change` (team 5).

Everything downstream in epic 2 — `e2.2`'s noise suppression, `e2.3`'s folder delta,
`e2.8`'s non-code surfaces, `e2.9`'s test surface — reads the label this ticket produces.
That is the reason it emits a confidence and a sentence per file and not just a class name.

---

## 1. What this decides, and what it refuses to

One question per changed path: **given the path *and* the bytes, what kind of file is this?**
`e0.9` answers the path half and refuses to look at bytes
([paths.md](paths.md) §9: a rule in the table has to stay overrulable by a person's
`fathom.toml`, and a sniff is not overrulable). This ticket is the other half of that
sentence: content evidence lives *above* the table, where the table can still overrule it.

Four rules shape everything else:

1. **The path table is consulted first and never rewritten.** With content evidence switched
   off, the classifier's answer is byte-for-byte `e0.9`'s answer on every fixture row and on
   every row of `e0.9`'s own 235-path corpus — a test, not an intention
   (`test_the_classifier_never_disagrees_with_the_path_table_when_content_is_off`). A second
   opinion about what `vendor/` means is the duplication paths.md exists to prevent, and this
   ticket is the most likely place to reintroduce it.
2. **A person's assertion outranks a byte.** `taxonomy.classes.*`, `test.globs` and
   `noise.globs` in a repository's own `fathom.toml` outrank every piece of content evidence
   there is, including a magic-number sniff. That is what keeps e0.9's promise true after
   this ticket: a hand-maintained file that *looks* generated stays whatever you told it.
3. **Ambiguity is `other`, never a guess.** Two claims too close to separate, or no claim at
   all, and the answer is `other` with both claims in the reason. The asymmetry is
   deliberate: guessing `source` inflates the reviewable line count and *lowers* the noise
   share, which is the failure mode that makes a risk score lie. Answering `other` costs a
   reviewer one glance at a path the table has not been taught.
4. **A label without receipts is a bug.** Every decision carries the rule or marker that won,
   the claims that lost with their scores, the facts read from the blob, a confidence, a band,
   and one sentence that says all of it in order.

## 2. The ladder: who outranks whom

Weights live in `policy.py` and are constants, not config — a tunable ambiguity threshold
would be a way to make a classifier *look* right on one repository.

| claim | weight | why it sits there |
|---|---|---|
| a path rule from a config layer (`flag`/`env`/`repo`/`user`) | 0.98 | somebody wrote this down about their own tree; it wins, always |
| a built-in path rule claiming `vendored` or `lockfile` | 0.97 | a statement about **why the file is in the tree**, which no header inside it can refute |
| "these bytes are not text" (NUL / undecodable head) | 0.94 | a fact about the bytes; a recognised container (ELF, PNG, Parquet) speaks for its own class instead |
| decisive markers and parsed shapes | 0.75 – 0.95 | a generated-code banner, a resolver's lock header, `-- +goose Up`, an OpenAPI document, a test attribute |
| a built-in path rule (by specificity) | 0.65 – 0.74 | the table's own guess about the shape of the tree |
| suggestive content, and path hints | 0.45 – 0.62 | minification, an extension convention, a file named by a build config |
| nothing | → `other` | no claim reached the floor (0.50), or two claims tied |

Per class, the score is the strongest claim for it plus a corroboration bonus (0.04 per
agreeing claim, capped at 0.12), clamped below 1.0 — no rule-based classifier is certain and
a `1.0` teaches reviewers to stop looking. Bands: `confirmed ≥ 0.85`, `likely ≥ 0.70`,
`weak ≥ 0.50`, `ambiguous < 0.50`. Two classes within 0.06 with neither decisive is
`ambiguous`, and `ambiguous` is answered `other`.

Three consequences worth stating because they are the ones a reviewer asks about:

* a protoc banner beats `**/*.rs` (`src/client.py` full of generated code is `generated`);
* `vendor/` beats the same banner (the file is not ours whatever wrote it — and the banner is
  still printed in the receipts, capped, with the reason it was capped);
* your `fathom.toml` beats both.

## 3. The content evidence

| source | where it reads | what it can say |
|---|---|---|
| header markers (`markers.py`, 37 of them, `MARKERS_VERSION`) | first 40 lines / 4 KiB | generated-code banners (protoc, Thrift, FlatBuffers, openapi-generator, jest snapshots, .NET designers), a resolver's lock header, migration annotations (goose, bun, alembic, Rails, Flyway), schema declarations (proto, Prisma, Thrift, GraphQL, SQL DDL), test attributes, a shebang, a wavefront mesh header |
| parsed content (`evidence.py`) | up to 256 KiB | decodes as text or not; container/executable magic; JSON / TOML / YAML shape, including `openapi`, `$schema`, Avro `record` and `lockfileVersion`; minification and `sourceMappingURL`; a top-level array as data |
| extension (`EXT_CLASS`) | the path | a hint only: corroboration, or a rescue for a path the table has no rule for |
| build-config membership (`BuildIndex`) | the build configs the caller already has | `Cargo.toml path =`, `package.json main/bin/files`, `tsconfig include`, `CMakeLists.txt`/`Makefile`/`meson.build` source lists, `Dockerfile COPY`, `.csproj Compile Include` — a list a human maintains by hand to build or ship the thing is evidence the thing is hand-written |

Two deliberate refusals live here. **A malformed document is a note, never an exception**: a
`package.json` that does not parse is reported as unparsed and classified from what is left,
because a detector that throws hides the episode (ADR-001 §7.6). **Prose is not a class**:
a text file in an unknown location with no structure in it stays `other`, because
"it looks like writing" is the sort of guess that later gets trusted.

`reformat` — the one noise level that needs content — is *not* produced here. It is a claim
about a **diff** (this change is whitespace-only), which is `e2.2`'s job; this layer makes
claims about a file.

## 4. The answer, and where a reviewer reaches it

```
$ python3 -m recommender.change explain --repo . src/api.pb.go

path    src/api.pb.go
CLASS   generated — confidence 0.99 (confirmed)
  why        content evidence — the banner protoc's own plugins write — matched
             `Code generated by protoc-gen-go` on line 1; the path rule `gen.protobuf`
             says `generated`, which the content agrees with
  path       generated ← rule gen.protobuf (layer defaults, taxonomy v1)
  scores     generated 0.99 · source 0.59
  evidence   5 claims:
             generated  rule:gen.protobuf        0.71  path rule gen.protobuf matched …
             generated  marker:gen.protoc        0.95  the banner protoc's own plugins …
             generated  marker:gen.banner        0.90  a generated-code banner: a comment …
             source     marker:src.language      0.45  ordinary declarations in a …
             source     ext:.go                  0.55  the `.go` extension is conventionally
  noise      generated (0.85) — rule gen.protobuf
  content    text=True size=105 lines=7 parsed=—
             marker gen.protoc     line 1   Code generated by protoc-gen-go
             marker gen.banner     line 1   // Code generated by protoc-gen-go. DO …
  ui         badge 'generated 0.99' · detail 'content evidence — the banner protoc's …'
```

That is AC 2 in full: the class, the confidence, the band, the marker that decided with the
line it matched, the rule that agreed, the claim that lost and what it scored. The confidence
and the class are printed once each, and the `why` line is the sentence rather than a
repetition of them — a reviewer reads the sentence, a grep reads the fields.

`-q` prints the one-line table a pane renders; `--json` prints a wrapper
(`classifier_version`, `taxonomy_version`, `table_digest`, `markers_version`, `cache`,
`decisions[]`) where each decision carries `cls`, `confidence`, `band`, `reason`, `winner`,
`votes[]` (`kind`, `source`, `class`, `weight`, `decisive`, `note`), `scores`, `facts`,
`path_rule`/`path_class`/`path_layer`, `noise`/`noise_rule`/`noise_score`, `reviewable`,
`diffable`, `ambiguous`, `notes`, `oid`, `overrode_path` and `cache_key` — because a TUI must
not parse prose; `--ui` prints exactly the four fields the diff tree pane gets (`badge`,
`detail`, `title`, `color`; ADR-006 §6.2 groups that pane by taxonomy class, ideal width 28,
minimum 20, and the badge fits the minimum). `episode` answers per change set instead of per
path — `--manifest <file>` with `{"id", "title", "changes": [{"path", "content" |
"content_base64", "change", "from"}]}`, or bare paths against `--repo` — and adds the
aggregations the summary card eats: class counts, noise counts, noise share, test-surface
ratio, band counts, the low-confidence list, the overridden list and the `other` list.

**`other` is a report, not a shrug.** The episode view prints unclassified paths under the
heading "no rule matched — `other` is the answer, and a missing row in the table is the fix",
which is the only honest affordance: the fix belongs in `taxonomy/builtins.py` plus a row in
`taxonomy/corpus.py`, never in a heuristic here.

## 5. Cost, and the per-blob cache

`classify` is the taxonomy lookup plus a bounded read of the blob's head: **≈70 µs per file
cold, ≈38 µs cached** (`bench`, this repository, `--repeat 4`), which is 1 000 changed files
in about 70 ms — inside the per-file budget ADR-004 §11 sets for an index pass. Markers never
scan past the head window, so a committed 400 MB model costs the same as a 2 KB source file.

The cache is `change.classify`, an analyzer in `ADR-004 §5.1`'s sense:

| field | value | why |
|---|---|---|
| scope | `blob` | as §5.1 says for taxonomy and generated-file detection — and keyed by **path and blob together**, the same correction `store/git.py` documents for `taxonomy.path`: a classification is a function of both, so a key naming only the content would let two paths sharing a blob share one wrong answer |
| `depends_on` | `taxonomy.path` | a table bump cascades here with no invalidation code, per §5.4 |
| `config_keys` | the four the taxonomy reads | §5.5: the key names everything the payload reads, and nothing it does not |
| input hash | path + blob oid + taxonomy digest + marker digest + classifier version | the digest is in the input as well as in `dep_digest`, as defence against a table edit that forgot its version bump — a stale classification is invisible, so this is the one place worth double-covering |

Payloads are canonical JSON with no clock in them, so **the same inputs produce identical
bytes in a different process** — asserted by running the whole corpus in two subprocesses and
diffing the encoded cache. The cache is bounded and cleared wholesale on overflow: a
classification is microseconds to redo, so an LRU would be complexity that buys nothing.

## 6. The labelled fixture set, and what "95 %" means here

`corpus.py` is 207 labelled paths across 16 repository shapes, and each row is
`(path, content, class)` — content in the fixture, because content evidence is what this
ticket added and a row answerable from the path alone would be testing `e0.9`. Labels are
written against the **built-in** tables with no configuration, exactly as the taxonomy corpus
is, so a local `fathom.toml` cannot move the number (`--with-config` scores the resolved table
as a diagnostic, never as the gate).

```
$ python3 -m recommender.change score --min 0.95
fathom change score · classifier v1 · taxonomy v1 (digest 726290bf) · markers v1
207 labelled paths over 16 repository shapes

accuracy  100.0% (207/207)  ·  bar 95%  ·  PASS
          the same set scored from paths alone: 91.8% — 17 rows need the content half of the
          classifier to be answered correctly
```

Two numbers gate or guard:

* **accuracy ≥ 95 %** is AC 1 and fails CI below the bar, printing per-class rows, per-shape
  rows, every misclassification with its reason, and the confusion pairs;
* **path-only accuracy** is the anti-gaming check: the corpus contains 17 rows that the content
  half decides — 7 of them are `other` in the `e0.9` table (`.obj` meshes, `.bin` and `.a`
  containers, a stripped `bin/deploy`, a build output whose ELF header says `binary` and not
  the `generated` its `obj/` path says), and the rest are generated-code banners, a jest
  snapshot, a bun-style `sql/patch_02.sql` and an NUnit test file. A test asserts that scoring
  without content is worse than scoring with it. A fixture set that a content-blind classifier
  could pass is not testing this ticket.

The `other` rows are in the corpus on purpose (`.obj` with prose in it, a zero-byte file in a
directory the table does not know, a file with nothing but prose and a random extension),
because AC 3 is only a test if the set contains paths that could have been guessed.

## 7. Adding to it, and what a change costs

* **A new marker**: add it to `MARKERS` with a stable `id`, a weight and a note a reviewer can
  read, bump `MARKERS_VERSION`, and add a corpus row. The digest pin in
  `tests/test_change_classifier_contract.py` fails until the version moves — the same bargain
  as `e0.9`'s table digest, and for the same reason: cached classifications quote it.
* **A new class claim is not available here.** The twelve classes are `taxonomy/classes.py`;
  a thirteenth would be a value no view knows how to render.
* **A weight change** fails `test_the_precedence_ladder_is_strictly_ordered`, which is the
  point: the ladder is the design, and it should only move deliberately.
* **A path-level convention** (a new `migrations/` spelling, a new lockfile name) belongs in
  `taxonomy/builtins.py` plus `taxonomy/corpus.py`, not here. This package's own tables are
  for what content can say, not for what a path can.

## 8. Ownership, and what this document refuses

ADR-002 §9 gives `fathom-analyze/src/taxonomy/` and `src/noise/` to team 5, and this is the
same team's second module: the classifier is `fathom-analyze::change`, and the Python fixture
layout mirrors it one-for-one (`markers`, `evidence`, `policy`, `engine`, `episode`, `cache`,
`corpus`, `eval`, `explain` pure; `cli` the only half that touches files, per §4.5).

**Refused.**

- *A learned classifier.* Same refusal as `e0.9`'s, and for the same reason: this label drives
  how much attention a human is asked to pay, and an unexplainable class is not a label
  everything downstream can be trusted to trust. The marker table and the ladder are reviewable
  in a way a model checkpoint is not.
- *Config-tunable thresholds.* They would let a repository tune the ambiguity floor until the
  accuracy report went green.
- *Content evidence that outranks a person.* That is the one thing paths.md §9 made a promise
  about, and it survives this ticket intact.
- *Per-file IO inside the engine.* The engine is a function of what it is handed; `cli.py`
  reads blobs, bounded by `MAX_BLOB_BYTES`. That is what makes the cache key honest.
- *Claiming `reformat`.* It is a property of a diff, not of a file, and the pass that decides a reformat
  is `e2.2`'s; a second answer from the classifier would be a second opinion, not a fact.

## 9. The executable half

```bash
python3 -m recommender.change explain src/api.pb.go             # receipts for one path
python3 -m recommender.change explain --repo . -q Cargo.lock …  # one line per path, real bytes
python3 -m recommender.change explain --json src/api.pb.go      # for a pane, not for eyes
python3 -m recommender.change episode --manifest ep.json        # per-episode answer
python3 -m recommender.change score --min 0.95                  # the AC 1 gate
python3 -m recommender.change classes                           # the twelve classes
python3 -m recommender.change markers                           # the marker table + digest
python3 -m recommender.change bench --repeat 8                  # the numbers in §5
python3 -m pytest tests/test_change_classifier_contract.py      # the contract, as tests
```

## 10. The Rust seam that already exists

`fathom-ingest`'s record layer (`e1.2`, `record.rs`) carries the taxonomy decision as
`PathTaxonomy { path_class, noise, taxonomy_version }` — ADR-004 §4.2's three columns — behind a
`Taxonomy` trait whose default implementation is `Unclassified`: every path `unknown` at version
`0`, which §4.7's `v_unknowns` makes visible instead of pretending is informative. The trait's own
doc comment says the engine that answers it is not ingest's — `e0.9`'s policy is the owner, `e2.1`
layers content evidence on top — and that `Unclassified` stays the default until a Rust engine is
wired in, which is a seam, not a ticket. Three consequences for whoever writes the Rust half:

* **The mapping is one-for-one and adds no columns.** `Classification.cls` → `path_class`,
  `Classification.noise` → `noise`, `Taxonomy.version` → `taxonomy_version`. The confidence, the
  band and the reason are **not** columns and must not become any: they are the classifier's own
  receipts, stored in the `change.classify` analyzer row keyed per blob (§5), and ADR-004 §6.6 R3
  means the *class* is what a downstream view reads without re-deriving it.
* **`version()` must move when the table does.** `taxonomy_version` travels in the export header so
  a golden file names the classifier that produced it; a Rust table bump that leaves it at `0` or
  at a stale number makes a golden diff meaningless. The Python side keeps the same bargain:
  `CACHE_VERSION == CLASSIFIER_VERSION`, and the marker digest is inside every cache key.
* **`fathom-analyze` may implement the trait directly.** ADR-002 §9 lets analyze depend on core,
  store and ingest, so `fathom_analyze::change::Policy` can `impl fathom_ingest::record::Taxonomy`
  and be handed to the ingest pipeline at the composition root (`e1.10`) — the trait does not have
  to be filled inside ingest, and ingest must not grow a path table of its own (§1.1 of
  [paths.md](paths.md): no per-analyzer glob list).
