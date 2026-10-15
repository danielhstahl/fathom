# Configuration: `fathom.toml`, its five layers, and how to find out what won

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-dq3` (`e0.8`)
- **Implements**: [ADR-002 §4.5](../adr/0002-workspace.md) (there is no `fathom-config` crate: types
  in `fathom-core`, discovery and layering in `fathom-cli`), and the glossary entry
  [`fathom.toml`](../concepts/thesis.md) — plus ADR-004 §5.5 (config digests), ADR-004 §8.2 and §11
  (retention and budget defaults), ADR-005 §6.3 (ingest budgets) and ADR-002 §6.3 (network consent)
- **Executes as**: `src/recommender/config/` (`schema.py`, `sources.py`, `layer.py`, `explain.py`,
  `cli.py`), `schemas/fathom.schema.json`, `tests/test_config_contract.py`, and
  `python3 -m recommender.config …`

Normative tables in this document are **generated** from `src/recommender/config/schema.py` by
`python3 -m recommender.config doc --write`; `doc --check` fails CI on drift, and
`schema --check` fails CI if `schemas/fathom.schema.json` is not byte-identical to what the current
key spec generates. A contract restated by hand is a contract that drifts — which is why the key
reference is never typed by hand twice.

---

## 1. What this decides

One question, answered once per process: **what is the configuration right now, and which layer said
so?**

Everything else is machinery for that answer. The consequences worth stating:

1. **One resolver.** `fathom-cli` resolves config into one typed root value and passes it downhill
   as data (ADR-002 §4.5). No crate below the CLI opens `fathom.toml`, reads `FATHOM_*`, or asks
   what size the repository is. A crate that reads the environment to decide how to analyse is a
   crate whose [cache keys](../adr/0004-storage.md) are lying, and lint `W1` is what keeps that from
   rotting back in.
2. **Every value is traceable to a file.** `fathom config explain` prints the winning layer *and*
   the chain it beat, per key, with `file:line`. This is not a nicety: the plan's later features —
   noise policy (`e0.9`), layer rules (`e3.1`), risk weights (`e2.9`) — are all "a function of
   config", so "why did the queue rank this episode at 61?" has to terminate in a file a human can
   open.
3. **Repo config is hostile input.** A cloned `fathom.toml` is reviewed like code and treated like
   an untrusted event (§5). It may not grant network consent, may not carry a listener token, and
   may not point a write path outside the repository.
4. **Unknown keys warn, they do not fail.** With a nearest-name suggestion and a line number. §6.

## 2. Precedence: five layers, no more

```
flag > env > repo fathom.toml > user config > built-in defaults
```

<!-- BEGIN GENERATED: precedence -->
| Layer | Rank | Source | What it is |
|---|---|---|---|
| `flag` | 1 | `--set k=v`, `--extend k=v`, sugar flags | this invocation, and the only layer that is transient by design |
| `env` | 2 | `FATHOM_*` (`__` = path separator) | CI and shell sessions; wins over files so a job can pin a value |
| `repo` | 3 | `<repo>/fathom.toml` | committed, reviewed, and hostile by default (see §5) |
| `user` | 4 | `$FATHOM_CONFIG`, `$XDG_CONFIG_HOME/fathom/config.toml`, `~/.config/fathom/config.toml` | the person's own preferences, outside the repository |
| `defaults` | 5 | compiled-in + the size-class budget row | what ships in the binary; no `/etc` file exists (see §3.1) |
<!-- END GENERATED: precedence -->

Three of the ordering decisions are worth defending, because each has a plausible alternative:

**Env beats repo file.** A CI job has to be able to pin one value without writing a file into a
repository it does not own, and `FATHOM_RISK__TAU=95` on a job step is the smallest thing that does
that. The alternative — repo file wins, so CI edits files — means a pipeline that mutates the tree
it is reviewing, which is the wrong shape of workaround.

**Repo beats user.** The repository is the shared, reviewed answer; your machine is your own. The
reverse ordering means every project's noise policy loses to whatever you once typed for a
different project, and a new clone silently gets your personal globs instead of the project's.

**There is no `/etc/fathom/config.toml`** (§3.1). "System defaults" in the ticket means
compiled-in values plus the budget row for the resolved size class, and that is a better sixth
layer than a root-owned file: it is versioned with the binary, it is visible in `explain`, and it
cannot exist on one machine and not another.

Layers 1–2 are **transient**: they exist for one process and are named as such in every trace. A
value that only exists in `--set` cannot be reproduced by anyone else, and `explain` says so out
loud rather than letting it look like it came from the repository.

## 3. Discovery

`fathom config` never asks you where your config is; it finds it, and `explain` reports what it
found, including what it looked for and did not find.

### 3.1 The repo root, and the repo file

The repository root is the nearest ancestor directory containing `.git`; failing that, the nearest
containing a `fathom.toml`. Not "the nearest `fathom.toml`" alone: that would let a stray
`~/fathom.toml` become the configuration of every directory outside a repository, which is how a
10-second accident becomes a 40-minute mystery. If neither is found, the command exits
`NOT_A_REPOSITORY` (3) unless `--repo` is passed.

### 3.2 The user config

First of: `$FATHOM_CONFIG`, then `$XDG_CONFIG_HOME/fathom/config.toml`, then
`~/.config/fathom/config.toml`. On macOS the XDG path is the default too — one documented location
beats a platform-conditional one that nobody can guess, and `FATHOM_CONFIG` exists precisely so
there is an escape hatch that does not need a rebuild.

`FATHOM_CONFIG` naming a file that **does not exist** is an error (5), not an ignored variable: a
silent fallback to `~/.config` is the worst possible outcome when you deliberately pointed it
somewhere else.

### 3.3 No system config file, and why

A root-owned config directory is a fourth place a value can hide, invisible in `git status` and
different on every host. The failure it would cause is the one Fathom is explicitly built to avoid:
two people, same repository, same binary, different answers, and no `explain` output that says so.
The compiled-in defaults plus the size-class budget row give the same "you did not have to write
anything" property, and are identical on every machine running the same binary.

### 3.4 The size class, which is a discovery result, not a user preference

Budget-shaped keys (§9) default from `benchmarks/budgets.json`, indexed by repository size class
(ADR-004 §11.1: *"a class is a budget row lookup, never an `if` in product code"*). `repo.size_class`
defaults to `auto`, which probes `git rev-list --count HEAD` and `git ls-files --cached` and takes
the **stricter** of the two classes. A probe that fails degrades to `small` and **says so in the
trace**; it does not pretend. Overriding it is a user decision available at every layer
(`--set repo.size_class=xlarge`, `FATHOM_REPO__SIZE_CLASS=xlarge`, or the file).

## 4. Merge: `replace`, `extend`, entry-wise

A layer does not always *win* outright, and the strategy is a property of the key, not of the layer.

| Strategy | Applies to | Semantics | Why |
|---|---|---|---|
| `replace` | scalars, and lists whose **order is the meaning** (`episode.boundaries`) | the higher layer's value is the answer | merging a priority order produces an order nobody chose |
| `extend` | glob lists (`noise.globs`, `test.globs`, `non_code.patterns`) and directory lists | concatenation, lowest layer first, first occurrence kept; `!negations` keep their position | a project adding `**/generated/**` must not silently delete the globs you rely on at home |
| entry-wise (`deep`) | open maps (`layers.<name>`, `risk.weights.<weight>`, `keys.overrides.<mode>.<key>`) | each *entry* has its own provenance, so a repo entry and a user entry coexist | `[layers] api = ["src/api"]` adds a layer; it does not drop yours |

`--set` **overrides** the strategy; `--extend` honours it explicitly. A file and an env var
always merge the way the key says, because a file is a standing decision and the key's owner chose
the strategy; a flag is an imperative for one run, so it gets both verbs. That is the whole
difference between `--set noise.globs=**/x/**` (this run sees exactly one glob) and
`--extend noise.globs=**/x/**` (the run adds one to whatever the files said).

Entry-wise merge is free rather than clever: provenance is tracked per **concrete** path
(`layers.ui`, not `layers`), so `layers.ui` can come from the user file while `layers.core` comes
from the defaults, and `explain` prints two rows with two sources. That is also why the number of
resolved decisions is larger than the number of declared keys — no number is quoted here, because a
count in prose is a count that goes stale: open maps are resolved per entry, since "which layer won"
is only meaningful at that granularity.

## 5. A cloned `fathom.toml` is hostile input

The thesis glossary is unambiguous: *hook events, commit messages, paths, lockfiles and
repo-provided config are hostile-by-default. A clone must never be able to make Fathom execute code
or write outside `.fathom/`.* Four rules, all executable:

| Rule | Refusal | Why it is a rule and not a convention |
|---|---|---|
| `scope = "user"` keys are ignored in a repo file: `privacy.advisories`, `privacy.llm`, `privacy.mcp_http_transport`, `mcp.auth_token` | warning `scope`, value recorded in the trace as `suppressed`, the effective value stays the default | network consent that a repository can grant is not consent (ADR-001 §7.5); a listener token in a cloned file is a token that was already leaked |
| `paths.*` from a repo file must resolve inside the repository | exit 5, naming the offending `file:line` | the store is the one thing Fathom writes; a `..`-shaped value is the write primitive an attacker wants |
| a file declaring a newer `meta.config_version` is refused | exit 5 with "upgrade fathom" | silently ignoring keys from a newer version and then caching analysis under the wrong digest is unrecoverable; ADR-005 §6.6's "never default to the newest version" applied to config |
| nothing in a config file is executed, imported, shell-parsed, or expanded | no rule needed: there is no interpolation syntax at all | every interpolation feature added is a new channel from "reviewed text file" to "code execution". Environment references are `FATHOM_*` variables, which exist because they are read by the *layer*, not by the parser |

A suppressed value is **kept in the trace**. Showing the attempt is strictly more useful than
showing a default: if a project ships `privacy.llm = true` and Fathom silently ignores it, the user
needs to be told it was seen, where, and why — otherwise the honest-looking answer `false` looks
like a bug in the layering.

## 6. Unknown keys: warn loudly, never fail silently

The acceptance criterion is "unknown keys warn with a nearest-name suggestion instead of failing
silently", which is a decision about *which* failure is worse.

```
warning[unknown_section]: unknown key 'layrs' is ignored (/repo/fathom.toml:20)
  suggestion: 'layers', 'blast', 'rss'
warning[scope]: privacy.llm may not be set by a repo file; ignored (/repo/fathom.toml:14)
  suggestion: set it in the user config (~/.config/fathom/config.toml) — consent is yours, not the repository's
warning[unknown_env]: ignoring $FATHOM_RISK__TAUUE: there is no config key 'risk.tauue' ($FATHOM_RISK__TAUUE)
  suggestion: $FATHOM_RISK__TAU
```

Three deliberate choices:

* **Unknown is a warning; wrong-typed is fatal.** A key this binary has never heard of may belong to
  a newer version, and refusing to start over it would make an upgrade a hard dependency for no
  benefit. A `[risk] tau = "eighty"` is the opposite: guessing there would change an analysis.
  Exit `5` (`CONFIG_INVALID`) is ADR-002 §7.3's definition of exactly that sentence.
* **Warnings name a line number.** `tomllib` returns values, not provenance, so `key_lines()` maps
  assignments to lines. "Unknown key `weightz`" without `:12` makes the user grep; with `:12` it is
  fixed in five seconds.
* **The suggestion is a nearest-name match over the key spec**, and for a mistyped *table*
  (`[layrs]`) the candidates are the tables. `difflib` is good enough and is in the standard
  library, and the standard library matters more here than a better edit distance: this code path
  runs on every invocation, in every build, including the ones CI runs with no dependencies.

`fathom config lint --strict` promotes warnings to exit 5 for a project that wants that; `lint`
without `--strict` reports and returns 0, so a newer file's keys never break an older binary's CI.

## 7. Types, coercion, and where the schema lives

`src/recommender/config/schema.py` is the single key spec: every key carries its type, default,
merge strategy, layer scope, units, allowed range, owning ticket, and — for the keys ADR-004 §5.1
declares as analyzer inputs — the analyzer that declares it. `fathom config keys` prints the list;
§10 is the same list as a table. From that spec:

* the **JSON Schema** `schemas/fathom.schema.json` (draft 2020-12, `additionalProperties: false`
  everywhere, `patternProperties` for the open maps, `x-fathom-*` annotations carrying merge/scope/
  units/budget/ticket so editor tooling and our own tooling read the same document);
* **validation** of files, via `json_schema.py` — a deliberately closed subset of JSON Schema
  (no `$ref`, no `oneOf`/`anyOf`/conditionals). Every construct added there is a construct the Rust
  deserializer must reproduce, and `oneOf` in a config schema is a deserializer that cannot tell you
  which branch you meant;
* **coercion** of `--set`/`--extend` and `FATHOM_*` values. Env names are resolved *against the
  schema* (`FATHOM_RISK__WEIGHTS__CYCLE` → `risk.weights.cycle` because that shape is declared), not
  parsed by splitting on underscores. `__` is the segment separator so a key containing `_`
  (`risk.weights.test_ratio`) stays unambiguous;
* **the nearest-name suggestion** (§6) and the type/range errors.

Coercion is strict by design: `FATHOM_RISK__TAU=seventy` exits 5 naming the variable. A silently
defaulted threshold in a CI job is a job that reports confidence it did not earn.

## 8. `fathom config explain`

```
fathom config explain [--path P] [--key K …] [--all] [--json]
```

The default output is *only the keys that are not the compiled-in default*, with the chain each won:

```
fathom config · /tmp/demo/repo
size class: small (probed 1 commits / 1 files) · budget rows: benchmarks/budgets.json
precedence: command-line flag > environment variable > repo fathom.toml > user config > built-in default
layers present: built-in defaults, ~/.config/fathom/config.toml, /tmp/demo/repo/fathom.toml, environment

5 keys differ from the built-in defaults:

risk.tau                won: environment variable · $FATHOM_RISK__TAU
    value: 85
    built-in default      70                                  default       built-in defaults
    user config           75                                  replaced      ~/.config/fathom/config.toml:5
    repo fathom.toml      80                                  replaced      /tmp/demo/repo/fathom.toml:8
    environment variable  85                                  replaced      $FATHOM_RISK__TAU

noise.globs             won: environment variable · $FATHOM_NOISE__GLOBS  [extend]
    value: **/dist/**, **/*.snap, **/*.min.js, **/generated/**, **/vendor/**
    built-in default      **/dist/**, **/*.snap               default       built-in defaults
    user config           **/*.min.js                         extended      ~/.config/fathom/config.toml:2
    repo fathom.toml      **/generated/**                     extended      /tmp/demo/repo/fathom.toml:5
    environment variable  **/vendor/**                        extended      $FATHOM_NOISE__GLOBS

privacy.llm             won: user config · ~/.config/fathom/config.toml:8
    value: true
    built-in default      false                               default       built-in defaults
    user config           true                                replaced      ~/.config/fathom/config.toml:8
    repo fathom.toml      true  SUPPRESSED                    suppressed    /tmp/demo/repo/fathom.toml:14 · scope=user: a repo file may not set it
```

`--path` answers the per-file question the queue and the diff views will keep asking:

```
path: dist/app.js

PATH-RELEVANT KEYS
  noise.globs         1 of 3 patterns match: **/dist/**
                      [x] **/dist/**     built-in defaults
                      [ ] **/*.snap      built-in defaults
                      [ ] **/generated/** /tmp/demo/repo/fathom.toml:5
  layers.core         the path falls in this layer

CACHE IMPACT (ADR-004 §5.5: only declared keys move a digest)
  taxonomy.path      config_digest 0526bdc5b15aef31…  declared: noise.globs, noise.builtins_version
  structure.delta    config_digest 54fdfebda74a3b61…  declared: layers., noise.globs
```

Three limits are stated on purpose:

* **It prints where every entry of a merged glob list came from**, because "which file added the
  glob that made this episode look quiet" is the question, and a merged list with one source is a
  merged list that cannot be debugged.
* **It delegates the taxonomy instead of guessing at it.** `--path` prints the config that feeds the
  decision *and* the verdict of `e0.9`'s one table: the rule that won, the layer it came from, the
  rules it beat and why. A second, slightly different path classifier here is the duplication
  `e0.9` (`reco-5vz`) exists to prevent, so this module calls the engine instead of restating it —
  see [docs/concepts/paths.md](paths.md).
* **Secrets are fingerprinted, never printed.** `mcp.auth_token` renders as
  `<redacted sha256:9f3a1c02>`: the fingerprint keeps the one question a redactor always breaks
  ("did it actually change?") answerable, which is the same reason ADR-005 §6.4 stores a real digest
  instead of asterisks. Home paths render as `~`, so `explain` output is bug-report-safe for the
  same reason `fathom doctor` output is.

`fathom config privacy` renders the consent matrix — capability, `net-*` feature, consent key,
state, and which layer set it — as a function, so `fathom doctor --privacy` (`e1.10`) renders the
same table instead of writing its own opinion. Telemetry is absent from the table because there is
no key for it, and a key for it would be a defect (ADR-001 §8 non-goal 5).

## 9. Budget-linked keys, size class, and the cache key

Five kinds of number in the product are published as *budgets* in ADR-004 §11 rather than as config
defaults: `cache.budget.mib`, `store.wal_cap.mib`, `store.size.mib`, `write.batch_rows`,
`write.batch.ms`, `write.mandatory_checkpoint_per_commit`, `prune.batch_rows`,
`rebuild.disk_ratio` and the `rss.*` set. They are **also** config keys, and their defaults come
from the budget row for the resolved size class — never from a second copy of the number.

<!-- BEGIN GENERATED: budgets -->
| Config key | Budget family (`benchmarks/budgets.json`) | Fallback |
|---|---|---|
| `ingest.size_cap_mib` | `ingest.file_size_cap.mib` | 8 |
| `cache.budget.mib` | `cache.budget.mib` | 512 |
| `store.wal_cap.mib` | `store.wal_cap.mib` | 64 |
| `store.size.mib` | `store.size.mib` | 2048 |
| `rss.analyzer_working_set.mib` | `rss.analyzer_working_set.mib` | 128 |
| `rss.cold_index_peak_mib` | `rss.cold_index.peak_mib` | 500 |
| `rss.sqlite_page_cache.mib` | `rss.sqlite_page_cache.mib` | 8 |
| `rss.daemon.mib` | `rss.daemon.mib` | 220 |
| `rss.tui.mib` | `rss.tui.mib` | 180 |
| `write.batch_rows` | `write.batch_rows` | 5000 |
| `write.batch_ms` | `write.batch.ms` | 50 |
| `write.mandatory_checkpoint_per_commit` | `write.mandatory_checkpoint_per_commit` | 0 |
| `prune.batch_rows` | `prune.batch_rows` | 5000 |
| `rebuild.disk_ratio` | `rebuild.disk_ratio` | 2.0 |
| `watch.cpu_share_pct` | `watch.budget.cpu_pct` | 5.0 |
| `query.frame_store_budget_ms` | `query.frame_store_budget.ms` | 8 |
<!-- END GENERATED: budgets -->

Two names, one value: ADR-004 §5.1 declares the config key `ingest.size_cap_mib` while §11 publishes
the budget as `ingest.file_size_cap.mib`. The alias table in `schema.py` is the only place that says
they are the same number, and a test refuses drift between them. A test also refuses any
cap/ceiling/peak budget family in `benchmarks/budgets.json` from being missing in the config schema,
so "the memory caps are configurable" stays true as the budget table grows.

Because these values feed `config_digest` (ADR-004 §5.5), a size-class change *is* an invalidation
event, and that is correct: an analysis performed under `rss.analyzer_working_set.mib=64` genuinely
is a different computation than one performed under 256. `Config.digest(keys)` reuses
`recommender.store.cache.derive_config_digest` rather than reimplementing it — two implementations of
a cache-key component is the worst possible place for a second opinion.

The declared-key coherence rule, checked in CI: every key in the ADR-004 §5.1 analyzer manifest
(`noise.globs`, `layers.`, `risk.weights`, `risk.tau`, `deps.ecosystems`, `non_code.patterns`,
`test.globs`, `graph.ignored_dirs`, `rename.threshold`, `diff.context_lines`, `episode.boundaries`,
`episode.time_gap_minutes`, `ingest.size_cap_mib`, `detectors.enabled`, `blast.hops`,
`noise.builtins_version`, `ratchet.`) resolves in this schema to a non-`absent` value. The
prefix-with-dot form (`layers.`) is the subtree convention ADR-004 §5.5 requires, and the trap it
guards against — a prefix key silently resolving to `absent`, so a config change that an analyzer
really reads invalidates nothing — is tested from the config side too.

## 10. Key reference

Generated. `default` shows the compiled-in fallback; for budget-linked keys the effective default is
the budget row for the size class (§9). `scope = user` means §5's suppression rule applies.

<!-- BEGIN GENERATED: sections -->
| Section | Owner | What it owns |
|---|---|---|
| `meta` | e0.8 | What the file says about itself, so a newer file is refused rather than half-understood. |
| `paths` | e0.8 / ADR-004 §12 | The four paths Fathom writes to. Every relative path is resolved against the repository root and must stay inside it. |
| `noise` | e0.9 (`reco-5vz`) | Globs for change that carries no reviewable information. `e0.9` owns the built-in table; this is the user half of it. |
| `taxonomy` | e0.9 (`reco-5vz`) | Assert what a set of paths IS, relative to the built-in table: one open map keyed by class. The engine and the built-ins are `reco-5vz`'s; only the overrides live here. |
| `test` | e1.7 / e2.6 | What counts as a test, for the test-surface analyzer. |
| `non_code` | e2.8 (`reco-bjd`) | Config, env, flags, migrations and CI — the surfaces a reviewer must be told about even though they are not code. |
| `graph` | e3.2 (`reco-8s4`) | Directories excluded from the dependency and structure graph. |
| `layers` | ADR-004 §5.1 / e3.1 | The declared intended architecture: named layers, each a list of path prefixes. An open map, keyed by layer name. |
| `deps` | e2.5 (`reco-j83`) | Which ecosystems' manifests and lockfiles are parsed. Also the switchboard for the advisory lookup's *subject*, not its consent. |
| `risk` | e2.9 / e2.10 | The ranking knobs. Changing these changes `risk.score` and `delta.compose` and nothing else (ADR-004 §6). |
| `detectors` | e2.7 | Which finding detectors run. |
| `blast` | e2.10 | How far the blast-radius walk goes before it declares a partial. |
| `rename` | e1.6 | Similarity threshold for calling two paths the same file. |
| `diff` | e1.6 | Per-file diff knobs. |
| `episode` | ADR-003 / e1.3 | Boundary sources in priority order, and the idle gap that ends an episode when nothing better is available. |
| `ingest` | e1.2 | The size cap: a bigger blob is recorded as oversized, never slurped. |
| `cache` | ADR-004 §11 | The class-R budget. Defaults come from the size class, not from a magic number. |
| `store` | ADR-004 §7, §11 | WAL ceiling and the soft store-size ceiling `doctor` warns at. |
| `rss` | ADR-004 §11.5 | The RSS budgets. Per ADR-004 §11.8 these are *never* scaled by a machine factor, so the only knob is the size class. |
| `write` | ADR-004 §7.3 | Batching knobs for the single writer. |
| `prune` | ADR-004 §8.1 | Prune batch size. |
| `rebuild` | ADR-004 §8.6 | Free-disk-ratio required before a rebuild starts. |
| `retention` | ADR-004 §8.2, ADR-005 §6.3 | Windows on the live epoch. ADR-004 §8.2 states these numbers as `fathom.toml`'s `[retention]` defaults; they are restated here from that table and a test refuses drift. |
| `harness` | ADR-005 §6.3 | The per-session ingest budget that quarantines a runaway hook stream. |
| `keys` | ADR-006 / e5.4 | Per-mode user overrides **only**. The canonical binding table is ADR-006's; conflict detection and the cheatsheet are `e5.4`'s. |
| `mcp` | ADR-005 §4, ADR-002 §6.3 | Transport and bind address. The token lives here and is user-scoped and redacted: a listener credential from a cloned repo is a gift. |
| `privacy` | ADR-001 §7.5, ADR-002 §6.3 | The runtime half of the no-network-by-default claim. Every key here is user-scoped: a clone may not consent on your behalf. |
| `query` | ADR-004 §7.6, §11 | The interrupt budget for one store call on the keystroke path. Exceeding it is a stated partial, not a hang. |
| `watch` | e1.9 (`reco-949`) | What share of one core `fathom watch` may spend while work is flowing. Past it the poll stretches toward its ceiling and says so, because a watcher that overruns the machine it is watching is the failure this ticket exists to avoid. |
| `ratchet` | ADR-004 §5.1 / e3.10 (`reco-5ht`) | The baselined pre-existing violations graph.cycles reads. ADR-004 §5.1 declares `ratchet.` as an analyzer input, so the subtree exists here; `e3.10` may extend it, and the digest test refuses a silent absence. |
| `repo` | ADR-004 §11.1 | The size class the budget defaults are chosen from. |
<!-- END GENERATED: sections -->

<!-- BEGIN GENERATED: keys -->
| Key | Type | Default | Merge | Scope | Units | Declared by | Ticket |
|---|---|---|---|---|---|---|---|
| `meta.config_version` | `integer` | `1` | `replace` | `any` | version | — | `e0.8` |
| `paths.store` | `path` | `.fathom/store.sqlite3` | `replace` | `any` | — | — | `e1.1` |
| `paths.objects` | `path` | `.fathom/objects` | `replace` | `any` | — | — | `e1.2` |
| `paths.events` | `path` | `.fathom/events.jsonl` | `replace` | `any` | — | — | `e6.1` |
| `paths.export` | `path` | `.fathom/export` | `replace` | `any` | — | — | `e5.7` |
| `noise.globs` | `glob_list` | `**/dist/**, **/*.snap` | `extend` | `any` | — | taxonomy.path | `e0.9` |
| `taxonomy.classes.<class-name>` | `glob_list` | `—` | `extend` | `any` | — | taxonomy.path | `e0.9` |
| `noise.builtins_version` | `integer` | `1` | `replace` | `any` | version | taxonomy.path | `e0.9` |
| `test.globs` | `glob_list` | `tests/**, **/test_*.py` | `extend` | `any` | — | test.surface | `e2.6` |
| `non_code.patterns` | `glob_list` | `**/migrations/**, .github/workflows/**` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `graph.ignored_dirs` | `string_list` | `node_modules` | `extend` | `any` | — | graph.build | `e3.2` |
| `layers.<layer-name>` | `string_list` | `core=['src/core'], store=['src/store'], ui=['src/ui']` | `deep` | `any` | — | structure.delta | `e3.1` |
| `deps.ecosystems` | `string_list` | `python, node, rust` | `extend` | `any` | — | deps.resolve | `e2.5` |
| `risk.weights.<weight>` | `number` | `blast=0.3, size=0.4, test_ratio=0.3` | `deep` | `any` | — | findings.detect | `e2.9` |
| `risk.tau` | `integer` | `70` | `replace` | `any` | score | risk.score | `e2.10` |
| `detectors.enabled` | `string_list` | `test_deletion, untested_new_logic, debug_leftover` | `extend` | `any` | — | findings.detect | `e2.7` |
| `blast.hops` | `integer` | `1` | `replace` | `any` | hops | blast_radius | `e2.10` |
| `rename.threshold` | `integer` | `60` | `replace` | `any` | percent | diff.rename_detect | `e1.6` |
| `diff.context_lines` | `integer` | `3` | `replace` | `any` | lines | diff.per_file | `e1.6` |
| `episode.boundaries` | `string_list` | `hook, trailer, author` | `replace` | `any` | — | episode.segment | `e1.3` |
| `episode.time_gap_minutes` | `integer` | `30` | `replace` | `any` | minutes | episode.segment | `e1.3` |
| `episode.force_split` | `string_list` | `—` | `extend` | `any` | — | episode.segment | `e1.3` |
| `episode.force_merge` | `string_list` | `—` | `extend` | `any` | — | episode.segment | `e1.3` |
| `ingest.size_cap_mib` | `integer` | `8` | `replace` | `any` | MiB | ingest.normalize | `e1.2` |
| `cache.budget.mib` | `integer` | `512` | `replace` | `any` | MiB | — | `e1.4` |
| `store.wal_cap.mib` | `integer` | `64` | `replace` | `any` | MiB | — | `e1.1` |
| `store.size.mib` | `integer` | `2048` | `replace` | `any` | MiB | — | `e1.10` |
| `rss.analyzer_working_set.mib` | `integer` | `128` | `replace` | `any` | MiB | — | `e0.11` |
| `rss.cold_index_peak_mib` | `integer` | `500` | `replace` | `any` | MiB | — | `e0.11` |
| `rss.sqlite_page_cache.mib` | `integer` | `8` | `replace` | `any` | MiB | — | `e1.1` |
| `rss.daemon.mib` | `integer` | `220` | `replace` | `any` | MiB | — | `e6.3` |
| `rss.tui.mib` | `integer` | `180` | `replace` | `any` | MiB | — | `e5.1` |
| `write.batch_rows` | `integer` | `5000` | `replace` | `any` | rows | — | `e1.1` |
| `write.batch_ms` | `integer` | `50` | `replace` | `any` | ms | — | `e1.1` |
| `write.mandatory_checkpoint_per_commit` | `integer` | `0` | `replace` | `any` | checkpoints | — | `e1.1` |
| `prune.batch_rows` | `integer` | `5000` | `replace` | `any` | rows | — | `e6.1` |
| `rebuild.disk_ratio` | `number` | `2.0` | `replace` | `any` | ratio | — | `e1.10` |
| `watch.cpu_share_pct` | `number` | `5.0` | `replace` | `any` | percent of one core | — | `e1.9` |
| `retention.cache_epochs` | `integer` | `2` | `replace` | `any` | epochs | — | `e6.1` |
| `retention.blob_epochs` | `integer` | `3` | `replace` | `any` | epochs | — | `e6.1` |
| `retention.index_run_keep_last` | `integer` | `20` | `replace` | `any` | runs | — | `e6.1` |
| `retention.usage_snapshot_days` | `integer` | `90` | `replace` | `any` | days | — | `e6.1` |
| `retention.store_event_days` | `integer` | `30` | `replace` | `any` | days | — | `e6.1` |
| `retention.events_days` | `integer` | `90` | `replace` | `any` | days | — | `e6.1` |
| `harness.max_bytes_per_session_mib` | `integer` | `64` | `replace` | `any` | MiB | — | `e6.1` |
| `keys.overrides.<mode>.<binding>` | `string` | `—` | `deep` | `any` | — | — | `e5.4` |
| `mcp.transport` | `string` | `stdio` | `replace` | `any` | — | — | `e6.4` |
| `mcp.bind` | `string` | `127.0.0.1` | `replace` | `any` | — | — | `e6.4` |
| `mcp.auth_token` | `string` | `` | `replace` | `user` | — | — | `e6.4` |
| `query.frame_store_budget_ms` | `integer` | `8` | `replace` | `any` | ms | — | `e5.1` |
| `ratchet.baseline` | `path` | `.fathom/ratchet.toml` | `replace` | `any` | — | graph.cycles | `e3.10` |
| `ratchet.mode` | `string` | `warn` | `replace` | `any` | — | graph.cycles | `e3.10` |
| `privacy.advisories` | `boolean` | `False` | `replace` | `user` | — | — | `e2.7` |
| `privacy.llm` | `boolean` | `False` | `replace` | `user` | — | — | `e4.7` |
| `privacy.mcp_http_transport` | `boolean` | `False` | `replace` | `user` | — | — | `e6.4` |
| `repo.size_class` | `string` | `auto` | `replace` | `any` | — | — | `e0.11` |
<!-- END GENERATED: keys -->

<!-- BEGIN GENERATED: aliases -->
| Config key | Budget id | Why two names |
|---|---|---|
| `ingest.size_cap_mib` | `ingest.file_size_cap.mib` | ADR-004 §5.1 declares the key, §11 publishes the budget; one value |
| `query.frame_store_budget_ms` | `query.frame_store_budget.ms` | ADR-004 §5.1 declares the key, §11 publishes the budget; one value |
| `rss.cold_index_peak_mib` | `rss.cold_index.peak_mib` | ADR-004 §5.1 declares the key, §11 publishes the budget; one value |
| `watch.cpu_share_pct` | `watch.budget.cpu_pct` | ADR-004 §5.1 declares the key, §11 publishes the budget; one value |
| `write.batch_ms` | `write.batch.ms` | ADR-004 §5.1 declares the key, §11 publishes the budget; one value |
<!-- END GENERATED: aliases -->

## 11. The executable half

```bash
python3 -m recommender.config explain --repo .            # what won, and what it beat
python3 -m recommender.config explain --path src/foo.rs   # the per-file decision chain
python3 -m recommender.config explain --key risk --json   # machine-readable, for a bug report
python3 -m recommender.config get noise.globs
python3 -m recommender.config schema --check              # CI: generated schema is not stale
python3 -m recommender.config schema --validate fathom.toml
python3 -m recommender.config lint --strict
python3 -m recommender.config privacy
python3 -m recommender.config doc --check                 # CI: this document is not stale
python3 -m recommender.config init > fathom.toml          # commented, from the key spec
```

`tests/test_config_contract.py` is the acceptance criteria as tests: precedence for all five layers,
merge strategies including negation ordering and `--set` overriding a strategy, the hostile-repo
rules, unknown-key suggestions with line numbers, schema drift and self-validation, the ADR-004
cross-contract coherence checks (declared analyzer keys, digest equality with the store fixture,
every cap-like budget row configurable), and the import-boundary rule from §1 — checked from the
syntax tree, with a per-module import whitelist, because a lint that fires on prose stops being
trusted in review. Exit codes are asserted, not assumed: wrong type → 5, bad flag → 2, no repository
→ 3.

## 12. Decisions refused, and the two open questions

* **A `fathom-config` crate.** Refused by ADR-002 §4.5 and still the right call: types are domain
  vocabulary, discovery is IO, and a crate that is both becomes the place everyone's PR lands.
* **Six layers with a `/etc/fathom/config.toml`.** Refused (§3.3).
* **TOML interpolation / `${env:VAR}` syntax.** Refused (§5's fourth row). It is the smallest
  possible bridge between "reviewed config" and "code execution", and `FATHOM_*` env vars already
  cover the legitimate use case with an audit trail.
* **Unknown keys fatal by default.** Refused (§6). `--strict` exists for teams that disagree.
* **A `[keymap]` section that owns bindings.** Refused: `keys.overrides.*` carries overrides only;
  the canonical table is ADR-006 §7.3's, and the cheatsheet is generated from the merged table so
  neither can lie. ADR-006 §8.2 is the promise this refusal buys: eleven chords Fathom never binds,
  so a user's personal chord cannot be silently taken, and §8.3's merge order says what happens when
  an override collides with a binding — `e5.4` reports it at load, it does not last-write-win.
* **Open: `mcp.bind` beyond loopback with consent.** Accepted *with* consent for now; if `e6.4`
  finds people binding on shared hosts, the right answer may be to refuse non-loopback binds
  outright and require a reverse proxy, rather than to grow an ACL.
* **`ratchet.mode` and `ratchet.baseline` ship now, and only just barely.** ADR-004 §5.1 declares
  `ratchet.` as a `graph.cycles` input, and a declared input that resolves to `absent` is exactly
  the trap §5.5 of that ADR names, so the subtree has to exist. The two keys in it are the two whose
  defaults are defensible without `e3.10`'s design: where the baseline lives (the class-M
  `.fathom/ratchet.toml` that ADR-004 §12 already commits) and what a new violation does
  (`warn`, because a ratchet that blocks a first-day review teaches nothing about the review). If
  `e3.10` wants different semantics for these two, it changes them in its own change set; the digest
  test in §9 only refuses silence.
