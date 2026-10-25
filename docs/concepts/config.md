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

Two keys live only here, and only here is where they can be read: `[ui] editor` and `[diff] difftool`
name programs this process *executes* ([the exits out of the reader](handoff.md#4-where-a-program-name-may-come-from)).
Everything else in a user config changes what Fathom says; these two change what it runs, which is why
the repository layer is refused them (§5) and why `fathom doctor` prints which layer won.

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
| `scope = "user"` keys are ignored in a repo file: `privacy.advisories`, `privacy.llm`, `privacy.mcp_http_transport`, `mcp.auth_token`, `mcp.approval_token` | warning `scope`, value recorded in the trace as `suppressed`, the effective value stays the default | network consent that a repository can grant is not consent (ADR-001 §7.5); a listener token in a cloned file is a token that was already leaked |
| `paths.*` from a repo file must resolve inside the repository | exit 5, naming the offending `file:line` | the store is the one thing Fathom writes; a `..`-shaped value is the write primitive an attacker wants |
| a file declaring a newer `meta.config_version` is refused | exit 5 with "upgrade fathom" | silently ignoring keys from a newer version and then caching analysis under the wrong digest is unrecoverable; ADR-005 §6.6's "never default to the newest version" applied to config |
| nothing in a config file is executed, imported, shell-parsed, or expanded | no rule needed: there is no interpolation syntax at all | every interpolation feature added is a new channel from "reviewed text file" to "code execution". Environment references are `FATHOM_*` variables, which exist because they are read by the *layer*, not by the parser |
| `[ui] editor` and `[diff] difftool` in a repo file are ignored, with a notice naming the key and the value | `fathom open` / `fathom difftool` print `fathom.toml's ... was ignored: a clone does not get to name a program this process runs` and use the user's own choice | these two keys name a program Fathom *executes*, which is the one thing a theme, a threshold and a glob do not do. The theme rule ([theming](theming.md)) and ADR-001 §4.6 already treat clone-provided config as untrusted; execution is the strongest reason in that family, and it is the reason the keys are `scope = user` in spirit even though they are read by the CLI rather than by the config schema |

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

Four deliberate choices:

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
* **A member of an open map has no nearest name in the schema, so the reader that knows the members
  says it instead.** `risk.weights.<weight>` is declared open, which is what lets
  `[risk.weights] comms = 0.15` resolve happily — and what lets a name the vector does not have
  (`new_dependency`, where the axis is `dependency`) sit in a `fathom.toml` unnoticed as a decision nothing
  is obeying. Closing the map is the wrong fix, for the same reason as the bullet above: a
  key belonging to a newer vector must not break this binary. So the vector refuses it where it is
  read — `fathom_analyze::risk::weights` names the key, says it is not a feature and prints the
  twelve — and `fathom doctor` prints that sentence as the finding `config_ignored_key`, untidy, with
  its remedy, and `--strict` leaves `5` for it (**reco-gna**). That is the verb a person runs to ask
  "did my edit do anything" without needing a score on the screen; `fathom why` still says the same
  line beside the score it came from, because a score has to name the weights it did not use.

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
| `test` | e2.9 (`reco-6pl`) | What counts as a test, which test-surface findings fire, and the numbers that define 'new public logic, no new test'. The finding policy is here rather than in `[detectors]` because these findings are one definition with knobs, not a detector registry. |
| `non_code` | e2.8 (`reco-bjd`) | Config, env, flags, migrations and CI — the surfaces a reviewer must be told about even though they are not code. |
| `structure` | e2.3 (`reco-5fn`) | Where an episode's code landed. The thresholds that decide when a set of pairings is a moved directory, a split or a flattening, the weights behind the structure-churn score, and one switch per finding kind. `fathom structure definition` prints these numbers as sentences. |
| `extract` | e3.1 (`reco-zoz`) | What one file may cost the structural extractor, and how far resolution may chase a re-export chain. Every one of these is a *bound the extractor reports on*, not a switch that turns a feature off: a file refused for size comes back with a reason and its file-level measurement, and a chain cut short comes back with `converged: false` rather than a guess. |
| `graph` | e3.2 (`reco-8s4`) | Directories excluded from the dependency and structure graph. |
| `layers` | ADR-004 §5.1 / e3.1 | The declared intended architecture: named layers, each a list of path prefixes. An open map, keyed by layer name. |
| `layering` | ADR-004 §5.1 / e3.6 (`reco-cfp`) | What the declared layers are allowed to depend on: refusals, their exceptions, boundary rules and the cycle rules. `[layers]` says where things live; this says which edges between them are violations. A rule that cannot match anything is refused when the file is read, with the reason, rather than becoming a silent no-op. |
| `api` | e3.7 (`reco-p5u`) | Which paths' exports the rest of the world compiles against, and how loudly a change to them speaks. A workspace of nine crates usually has two whose `pub` items are somebody else's contract, and a breaking change reported in the third is noise - the ignore and private sets are how the owner says which ones count, and `api.track_tests` is where the built-in answer to `is this a test file?` (`e3.12`) can be overruled. Reading the source is `[extract]`'s business; this table is the claim about the repository. |
| `check` | e6.6 (`reco-ngk`) | What `fathom check` enforces in this repository, and where the run turns red. One table holds the layout version, the gate's volume, the threshold that exits 7, each check's severity and each family's own numbers. It is per-repository and it is read strictly: a value that will not parse, a key this build does not know, or a layout version it cannot read is a refusal (exit 5), never a defaulted pass — a gate that half-reads a policy is a gate that passes what it never looked at. |
| `hotspots` | e3.8 (`reco-wky`) | What the hotspot score is made of: the six metric weights, the knee each saturating curve bends at, the churn window, and how long a new symbol may go unreferenced before it is a candidate. A hotspot is "expensive to change and widely felt", not "bad", and every knob here moves an explainable number rather than switching a report on or off. |
| `scorecard` | e3.9 (`reco-al1`) | What the architecture score is made of: the five weights, the window a per-window score covers, how much movement still counts as flat, how many revisions a printed trend keeps, and the four published constants the curves divide by. The curve *shapes* are not knobs — they are the model — so every number a reviewer can recompute by hand stays recomputable after a tune. `fathom architecture --explain` prints what is in force. `e3.9`. |
| `deps` | e2.5 (`reco-j83`) / e2.7 (`reco-yxi`) | Which ecosystems' manifests and lockfiles are parsed, and what the dependency-risk answer does with them: the advisory cache and its freshness bars, the licence matrix, and how hard provenance looks for the episode that asked for a package. Consent to reach the network is not here - it is `privacy.advisories`, because a cloned repository must not be able to grant it. |
| `risk` | e2.9 / e2.10 | The ranking knobs. Changing these changes `risk.score` and `delta.compose` and nothing else (ADR-004 §6). |
| `detectors` | e2.7 | Which finding detectors run. |
| `events` | e6.7 (`reco-raw`) | Whether a run publishes the analysis lifecycle — what started, what each detector did, what every finding said, how the run ended — as versioned JSONL an external tool can read. Off until the repository says otherwise; the path is `paths.analysis_events`. |
| `blast` | e2.10 | How far the blast-radius walk goes before it declares a partial, the confidence attached to each basis, and the path-heuristic weights that decide what counts as reached. |
| `card` | e2.10 (`reco-4ey`) | The one-screen answer per episode. Row caps, how loud a finding has to be to earn an attention row, and one switch per section - because a repository narrowing its own card is a different thing from a card quietly losing a question, and the card prints which happened. |
| `rename` | e1.6 / e2.4 (`reco-a0v`) | Two questions with two bars. `rename.threshold` is what the diff pane prints as a rename; the `fact_bar`/`guess_floor` pair decides what the pairing engine asserts, what it labels a guess, and what it costs. |
| `diff` | e1.6 | Per-file diff knobs. |
| `episode` | ADR-003 / e1.3 | Boundary sources in priority order, and the idle gap that ends an episode when nothing better is available. |
| `attribution` | ADR-003 §8 / e4.1 (`reco-w7q`) | Who wrote a commit: the registry of known agents, the marker table, and the confidence bar below which Fathom prints `?` instead of guessing. Every value here is handed to the classifier as data — no crate below the CLI opens this file. |
| `intent` | ADR-003 §8 / e4.2 (`reco-c54`) | What an episode was *for*: the ladder that answers that question (the harness prompt, the `Agent-Intent:` trailer, a `Task:` line, a prompt digest) and how much of the answer is kept. The ladder's ORDER is not configurable, only its membership — an intent a user reordered is an intent nobody can explain, which is the same refusal `[episode] boundaries` and `[attribution]` make. |
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
| `mcp` | ADR-005 §4, ADR-002 §6.3 | Transport, bind address, the write gate (`allow_writes`) and the size one response may reach. The two secrets live here and are user-scoped and redacted — `mcp.auth_token` for a listener, `mcp.approval_token` for a write an agent may make — and a credential a cloned repo could write is a credential already given away. |
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
| `paths.analysis_events` | `path` | `.fathom/analysis-events.jsonl` | `replace` | `any` | — | — | `e6.7` |
| `noise.globs` | `glob_list` | `**/dist/**, **/*.snap` | `extend` | `any` | — | taxonomy.path | `e0.9` |
| `taxonomy.classes.<class-name>` | `glob_list` | `—` | `extend` | `any` | — | taxonomy.path | `e0.9` |
| `noise.builtins_version` | `integer` | `1` | `replace` | `any` | version | taxonomy.path | `e0.9` |
| `noise.suppress.reformat` | `boolean` | `True` | `replace` | `any` | — | noise.suppress | `e2.2` |
| `noise.suppress.generated_churn` | `boolean` | `True` | `replace` | `any` | — | noise.suppress | `e2.2` |
| `noise.suppress.lockfile` | `boolean` | `True` | `replace` | `any` | — | noise.suppress | `e2.2` |
| `noise.suppress.vendored` | `boolean` | `True` | `replace` | `any` | — | noise.suppress | `e2.2` |
| `noise.suppress.user_glob` | `boolean` | `True` | `replace` | `any` | — | noise.suppress | `e2.2` |
| `noise.collapse.noise_only` | `boolean` | `True` | `replace` | `any` | — | noise.suppress | `e2.2` |
| `noise.keep` | `glob_list` | `—` | `extend` | `any` | — | noise.suppress | `e2.2` |
| `noise.expand` | `string` | `b` | `replace` | `any` | — | noise.suppress | `e2.2` |
| `test.globs` | `glob_list` | `tests/**, **/test_*.py` | `extend` | `any` | — | test.surface | `e2.6` |
| `test.findings.test_removed` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.findings.test_file_removed` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.findings.test_skipped` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.findings.assertion_weakened` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.findings.assertion_removed` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.findings.untested_public` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.findings.focused_test` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.untested.min_body_lines` | `integer` | `4` | `replace` | `any` | lines | test.surface | `e2.9` |
| `test.untested.require_logic` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.untested.visibility` | `string` | `public` | `replace` | `any` | visibility | test.surface | `e2.9` |
| `test.untested.scope` | `string` | `episode` | `replace` | `any` | scope | test.surface | `e2.9` |
| `test.untested.name_overlap` | `integer` | `1` | `replace` | `any` | tokens | test.surface | `e2.9` |
| `test.untested.path_overlap` | `integer` | `1` | `replace` | `any` | tokens | test.surface | `e2.9` |
| `test.untested.mention_in_test` | `boolean` | `True` | `replace` | `any` | — | test.surface | `e2.9` |
| `test.untested.exclude` | `glob_list` | `—` | `extend` | `any` | — | test.surface | `e2.9` |
| `test.proxy.confidence.parsed` | `number` | `0.9` | `replace` | `any` | confidence | test.surface | `e2.9` |
| `test.proxy.confidence.partial` | `number` | `0.6` | `replace` | `any` | confidence | test.surface | `e2.9` |
| `test.proxy.confidence.lines_only` | `number` | `0.3` | `replace` | `any` | confidence | test.surface | `e2.9` |
| `test.card.max_findings` | `integer` | `12` | `replace` | `any` | rows | test.surface | `e2.9` |
| `non_code.patterns` | `glob_list` | `**/migrations/**, .github/workflows/**` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.env_added` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.env_removed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.env_declared_unread` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.env_stale_reference` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.env_renamed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.env_value_committed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.flag_added` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.flag_removed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.flag_default_changed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.flag_read_no_default` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.migration_added` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.migration_forward_only` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.migration_destructive` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.migration_with_model` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.deploy_coordination` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.ci_secret_new` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.ci_permission_escalation` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.ci_oidc_changed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.ci_runner_changed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.ci_trigger_main_push` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.iac_resource_added` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.iac_resource_removed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.iac_policy_widened` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.container_base_changed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.container_base_unpinned` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.container_supply_chain` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.findings.container_privilege_changed` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.env.declaration_globs` | `glob_list` | `.env.example, .env.sample, .env.template, .env.default, .env.dist, **/.env.example, **/.env.sample, **/.env.template, **/*.env.example, **/*.env.sample, config/env.example, docs/env.example` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.env.committed_globs` | `glob_list` | `.env, .env.local, .env.production, **/.env, **/.env.production, **/.env.staging, config/env.ini, **/secrets.yaml, **/secrets.yml` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.env.placeholder_values` | `string_list` | `, changeme, change-me, change_me, your-value-here, your_value_here, your-secret-here, xxx, xxxx, xxxxx, <value>, <secret>, placeholder, sample, example, examples, test, dummy, todo, none, null, true, false, 0, 1, secret, password, admin, foo, bar, undefined, REPLACE_ME, insert-here, redacted` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.env.secret_name_pattern` | `string` | `(?i)(^|_)(secret|token|password|passwd|pwd|api[_-]?key|access[_-]?key|private[_-]?key|credential|auth[_-]?key|dsn|_key|_cert|_certificate)($|_)|(^|_)(aws|gcp|azure|oidc)(_|-)?(key|secret|token|credential)` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.env.reference_scope` | `string` | `episode` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.env.rename_similarity` | `number` | `0.6` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.flags.globs` | `glob_list` | `flags.yaml, flags.yml, flags.json, feature-flags.yaml, feature-flags.json, **/flags.yaml, **/flags.yml, **/flags.json, **/features.yaml, **/features.json, **/featureflags.json, **/flip-flop.json, **/unleash-toggles.json, config/flags/**, **/config/flags/**, **/feature_flags.py, **/flags.php` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.flags.require_default` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.migration.globs` | `glob_list` | `**/migrations/**, **/migration/**, db/migrate/**, db/migrations/**, **/alembic/versions/**, **/migrations/versions/**, prisma/migrations/**, **/flyway/sql/**, src/main/resources/db/migration/**, **/migrations/**.sql, **/migrations/**.up.sql, resource/migrations/**` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.migration.model_globs` | `glob_list` | `**/models.py, **/models/**.py, **/model.py, db/schema.rb, db/schema.sql, app/models/**, **/entities/**.java, **/Entity/**.php, **/*.prisma, **/schema.sql, **/models/**.go, internal/model/**.go, **/Models/**.cs, **/migrations/**/schema.rs` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.migration.destructive_ops` | `string_list` | `drop table, drop column, drop index, drop constraint, drop database, truncate table, truncate , set not null, alter column, modify column, rename to, delete from, update , alter type, drop materialized view, drop_table, remove_column, remove_index, change_column_null, delete_all, destroy_all, execute ` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.ci.default_branches` | `string_list` | `main, master` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.ci.escalating_permissions` | `string_list` | `contents:write, actions:write, security-events:write, id-token:write, pull-requests:write, deployments:write, packages:write, statuses:write, checks:write, issues:write, discussions:write, pages:write, organization-secrets:write, organization-administration:write, write-all` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.iac.globs` | `glob_list` | `**/*.tf, **/*.tfvars, **/*.hcl, cloudformation*.y*ml, **/cloudformation*.y*ml, **/cfn-*.y*ml, k8s/**.y*ml, **/k8s/**.y*ml, **/kubernetes/**.y*ml, **/manifests/**.y*ml, **/helm/**/values.y*ml, **/Chart.yaml, ansible/**.y*ml, **/*.stack.yaml, Pulumi.*.yaml, **/*.cue` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.iac.widened_patterns` | `string_list` | `"*", "arn:aws:iam::aws:policy/AdministratorAccess", "0.0.0.0/0", ::/0, "Principal": "*", associate_public_ip_address = true, cidr_blocks = ["0.0.0.0/0"], Action = ["*"], --privileged` | `extend` | `any` | — | non_code.surface | `e2.8` |
| `non_code.container.require_digest_pin` | `boolean` | `True` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `non_code.card.max_actions` | `integer` | `12` | `replace` | `any` | — | non_code.surface | `e2.8` |
| `structure.code_classes` | `string_list` | `source, test` | `extend` | `any` | path class | — | `e2.3` |
| `structure.metrics.top_dirs` | `integer` | `3` | `replace` | `any` | directories | — | `e2.3` |
| `structure.move.min_files` | `integer` | `3` | `replace` | `any` | files | — | `e2.3` |
| `structure.move.min_share` | `number` | `0.6` | `replace` | `any` | share | — | `e2.3` |
| `structure.move.require_emptied` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.move.infer_content` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.move.min_similarity` | `number` | `0.5` | `replace` | `any` | 0-1 | — | `e2.3` |
| `structure.move.fact_bar` | `number` | `0.75` | `replace` | `any` | 0-1 | — | `e2.3` |
| `structure.split.min_destinations` | `integer` | `2` | `replace` | `any` | dirs | — | `e2.3` |
| `structure.split.min_files_per_destination` | `integer` | `2` | `replace` | `any` | files | — | `e2.3` |
| `structure.flatten.min_levels` | `integer` | `1` | `replace` | `any` | levels | — | `e2.3` |
| `structure.churn.weight.moved_files` | `number` | `0.25` | `replace` | `any` | weight | — | `e2.3` |
| `structure.churn.weight.moved_lines` | `number` | `0.25` | `replace` | `any` | weight | — | `e2.3` |
| `structure.churn.weight.directory_churn` | `number` | `0.2` | `replace` | `any` | weight | — | `e2.3` |
| `structure.churn.weight.stories` | `number` | `0.2` | `replace` | `any` | weight | — | `e2.3` |
| `structure.churn.weight.shape_shift` | `number` | `0.1` | `replace` | `any` | weight | — | `e2.3` |
| `structure.churn.story_ceiling` | `number` | `2` | `replace` | `any` | stories | — | `e2.3` |
| `structure.churn.depth_scale` | `number` | `4` | `replace` | `any` | levels | — | `e2.3` |
| `structure.churn.band.local` | `number` | `0.15` | `replace` | `any` | score | — | `e2.3` |
| `structure.churn.band.structural` | `number` | `0.35` | `replace` | `any` | score | — | `e2.3` |
| `structure.depth.mean_delta` | `number` | `0.5` | `replace` | `any` | levels | — | `e2.3` |
| `structure.depth.max_delta` | `integer` | `2` | `replace` | `any` | levels | — | `e2.3` |
| `structure.concentration.delta` | `number` | `0.15` | `replace` | `any` | share | — | `e2.3` |
| `structure.card.max_dirs` | `integer` | `12` | `replace` | `any` | rows | — | `e2.3` |
| `structure.card.max_stories` | `integer` | `6` | `replace` | `any` | rows | — | `e2.3` |
| `structure.card.max_evidence` | `integer` | `4` | `replace` | `any` | lines | — | `e2.3` |
| `structure.findings.subtree_moved` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.churn_high` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.package_renamed` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.module_split` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.directory_flattened` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.directory_removed` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.nesting_deepened` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.concentration_up` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.files_moved` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.directory_added` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `structure.findings.move_unconfirmed` | `boolean` | `True` | `replace` | `any` | — | — | `e2.3` |
| `graph.ignored_dirs` | `string_list` | `node_modules` | `extend` | `any` | — | graph.build | `e3.2` |
| `graph.snapshot_cache_mib` | `integer` | `32` | `replace` | `any` | MiB | graph.build | `e7.3` |
| `graph.enabled` | `boolean` | `True` | `replace` | `any` | — | graph.build | `e3.11` |
| `graph.window` | `integer` | `32` | `replace` | `any` | revisions | graph.build | `e3.11` |
| `graph.max_corpus_mib` | `integer` | `64` | `replace` | `any` | MiB | graph.build | `e3.11` |
| `extract.max_file_bytes` | `integer` | `2000000` | `replace` | `any` | bytes | structure.extract | `e3.1` |
| `extract.complexity.max_nodes` | `integer` | `20000` | `replace` | `any` | syntax nodes | structure.extract | `e3.1` |
| `extract.resolution.max_passes` | `integer` | `4` | `replace` | `any` | passes | structure.extract | `e3.1` |
| `extract.resolution.max_hops` | `integer` | `4` | `replace` | `any` | hops | structure.extract | `e3.1` |
| `extract.ruby.max_files_per_run` | `integer` | `4000` | `replace` | `any` | files per run | structure.extract | `e3.1b` |
| `extract.ruby.max_bytes_per_run` | `integer` | `24000000` | `replace` | `any` | bytes per run | structure.extract | `e3.1b` |
| `extract.ruby.priority` | `string_list` | `lib, app, ext, spec, test` | `replace` | `any` | — | structure.extract | `e3.1b` |
| `extract.java.same_package_scan` | `string` | `imports_plus_package` | `replace` | `any` | — | structure.extract | `e3.1b` |
| `layers.<layer-name>` | `string_list` | `core=['src/core'], store=['src/store'], ui=['src/ui']` | `deep` | `any` | — | structure.delta | `e3.1` |
| `layering.mode` | `string` | `warn` | `replace` | `any` | — | — | `e3.6` |
| `layering.order` | `string_list` | `—` | `replace` | `any` | — | — | `e3.6` |
| `layering.forbid` | `string_list` | `—` | `extend` | `any` | — | — | `e3.6` |
| `layering.allow` | `string_list` | `—` | `extend` | `any` | — | — | `e3.6` |
| `layering.only` | `string_list` | `—` | `extend` | `any` | — | — | `e3.6` |
| `layering.ignore` | `string_list` | `—` | `extend` | `any` | — | — | `e3.6` |
| `layering.no_upward` | `boolean` | `False` | `replace` | `any` | — | — | `e3.6` |
| `layering.no_cycles` | `boolean` | `False` | `replace` | `any` | — | — | `e3.6` |
| `layering.no_cycle_within_layer` | `boolean` | `False` | `replace` | `any` | — | — | `e3.6` |
| `layering.max_violations` | `integer` | `50` | `replace` | `any` | violations | — | `e3.6` |
| `api.mode` | `string` | `warn` | `replace` | `any` | — | — | `e3.7` |
| `api.ignore` | `string_list` | `—` | `extend` | `any` | — | — | `e3.7` |
| `api.package` | `string_list` | `—` | `extend` | `any` | — | — | `e3.7` |
| `api.private` | `string_list` | `—` | `extend` | `any` | — | — | `e3.7` |
| `api.track_tests` | `boolean` | `False` | `replace` | `any` | — | — | `e3.12` |
| `api.max_items_per_package` | `integer` | `20000` | `replace` | `any` | items | — | `e3.7` |
| `check.version` | `integer` | `1` | `replace` | `any` | layout version | — | `e6.6` |
| `check.mode` | `string` | `warn` | `replace` | `any` | — | — | `e6.6` |
| `check.fail_on` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.base` | `string` | `` | `replace` | `any` | — | — | `e6.6` |
| `check.head` | `string` | `HEAD` | `replace` | `any` | — | — | `e6.6` |
| `check.require` | `string_list` | `—` | `replace` | `any` | — | — | `e6.6` |
| `check.ignore` | `string_list` | `—` | `extend` | `any` | — | — | `e6.6` |
| `check.max_findings` | `integer` | `40` | `replace` | `any` | findings per check | — | `e6.6` |
| `check.api_files` | `integer` | `6000` | `replace` | `any` | files per side | — | `e6.6` |
| `check.risk_episodes` | `integer` | `40` | `replace` | `any` | episodes | — | `e6.6` |
| `check.max_episodes` | `integer` | `200` | `replace` | `any` | episodes | — | `e6.6` |
| `check.severity.layering` | `string` | `error` | `replace` | `any` | — | — | `e6.6` |
| `check.severity.cycles` | `string` | `error` | `replace` | `any` | — | — | `e6.6` |
| `check.severity.api` | `string` | `error` | `replace` | `any` | — | — | `e6.6` |
| `check.severity.dependencies` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.severity.tests` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.severity.sign-off` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.tests.min_new_lines` | `integer` | `200` | `replace` | `any` | added lines | — | `e6.6` |
| `check.tests.min_tested_ratio` | `number` | `0.5` | `replace` | `any` | share of new logic | — | `e6.6` |
| `check.dependencies.new_direct` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.dependencies.major_change` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.dependencies.new_source` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.dependencies.downgrade` | `string` | `warning` | `replace` | `any` | — | — | `e6.6` |
| `check.dependencies.forbid` | `string_list` | `—` | `extend` | `any` | — | — | `e6.6` |
| `check.risk.threshold` | `integer` | `70` | `replace` | `any` | score | — | `e6.6` |
| `scorecard.weight.boundaries` | `number` | `0.3` | `replace` | `any` | weight | — | `e3.9` |
| `scorecard.weight.coupling` | `number` | `0.15` | `replace` | `any` | weight | — | `e3.9` |
| `scorecard.weight.acyclicity` | `number` | `0.25` | `replace` | `any` | weight | — | `e3.9` |
| `scorecard.weight.api` | `number` | `0.15` | `replace` | `any` | weight | — | `e3.9` |
| `scorecard.weight.hotspots` | `number` | `0.15` | `replace` | `any` | weight | — | `e3.9` |
| `scorecard.window` | `integer` | `30` | `replace` | `any` | revisions | — | `e3.9` |
| `scorecard.flat_within` | `integer` | `2` | `replace` | `any` | points | — | `e3.9` |
| `scorecard.trend_cap` | `integer` | `240` | `replace` | `any` | points | — | `e3.9` |
| `scorecard.coupling_ceiling` | `number` | `8.0` | `replace` | `any` | edges/module | — | `e3.9` |
| `scorecard.api_knee` | `number` | `4.0` | `replace` | `any` | exports | — | `e3.9` |
| `scorecard.hotspot_bar` | `integer` | `70` | `replace` | `any` | score | — | `e3.9` |
| `scorecard.alert_below` | `integer` | `50` | `replace` | `any` | score | — | `e3.9` |
| `hotspots.window` | `integer` | `30` | `replace` | `any` | revisions | — | `e3.8` |
| `hotspots.min_history` | `integer` | `2` | `replace` | `any` | revisions | — | `e3.8` |
| `hotspots.top` | `integer` | `20` | `replace` | `any` | rows | — | `e3.8` |
| `hotspots.dead_code_after` | `integer` | `3` | `replace` | `any` | episodes | — | `e3.8` |
| `hotspots.duplicate_block_lines` | `integer` | `12` | `replace` | `any` | lines | — | `e3.8` |
| `hotspots.god_bar` | `number` | `0.5` | `replace` | `any` | — | — | `e3.8` |
| `hotspots.flags` | `string_list` | `—` | `extend` | `any` | — | — | `e3.8` |
| `hotspots.ignore` | `glob_list` | `—` | `extend` | `any` | — | — | `e3.8` |
| `hotspots.weights.<weight>` | `number` | `churn=0.2, complexity=0.2, fan_in=0.25, fan_out=0.1, instability=0.1, size=0.15` | `deep` | `any` | — | — | `e3.8` |
| `hotspots.knee.<weight>` | `number` | `churn=2.0, complexity=120.0, fan_in=12.0, fan_out=12.0, size=600.0` | `deep` | `any` | — | — | `e3.8` |
| `deps.ecosystems` | `string_list` | `python, node, rust` | `extend` | `any` | — | deps.resolve | `e2.5` |
| `deps.advisories.enabled` | `boolean` | `True` | `replace` | `any` | — | deps.advisories | `e2.7` |
| `deps.advisories.mode` | `string` | `cache` | `replace` | `any` | — | deps.advisories | `e2.7` |
| `deps.advisories.cache` | `path` | `.fathom/advisories.json` | `replace` | `any` | — | deps.advisories | `e2.7` |
| `deps.advisories.max_age_hours` | `integer` | `24` | `replace` | `any` | hours | deps.advisories | `e2.7` |
| `deps.advisories.stale_limit_hours` | `integer` | `168` | `replace` | `any` | hours | deps.advisories | `e2.7` |
| `deps.advisories.max_packages` | `integer` | `200` | `replace` | `any` | packages | deps.advisories | `e2.7` |
| `deps.advisories.timeout_ms` | `integer` | `1500` | `replace` | `any` | milliseconds | deps.advisories | `e2.7` |
| `deps.advisories.ecosystems` | `string_list` | `crates.io, PyPI, npm, Go, RubyGems, Maven, NuGet, Hex` | `extend` | `any` | — | deps.advisories | `e2.7` |
| `deps.license.deny` | `string_list` | `AGPL-3.0, AGPL-3.0-only, AGPL-3.0-later, SSPL-1.0` | `extend` | `any` | — | deps.license | `e2.7` |
| `deps.license.permissive` | `string_list` | `MIT, MIT-0, Apache-2.0, Apache-1.1, BSD-2-Clause, BSD-3-Clause, BSD-3-Clause-Clear, ISC, Zlib, 0BSD, Python-2.0, PSF-2.0, Unlicense, BSL-1.0, PostgreSQL, SQLite, curl, WTFPL, Beerware, Unicode-3.0, CC0-1.0, MIT-open-group` | `extend` | `any` | — | deps.license | `e2.7` |
| `deps.license.weak_copyleft` | `string_list` | `MPL-2.0, MPL-1.1, LGPL-2.0-only, LGPL-2.0-or-later, LGPL-2.1-only, LGPL-2.1-or-later, LGPL-3.0-only, LGPL-3.0-or-later, CDDL-1.0, CDDL-1.1, EPL-1.0, EPL-2.0, GPL-2.0-only WITH Classpath-exception-2.0, GPL-2.0-or-later WITH Classpath-exception-2.0, GPL-2.0-with-classpath-exception, Universal-FOSS-Exception-1.0` | `extend` | `any` | — | deps.license | `e2.7` |
| `deps.license.strong_copyleft` | `string_list` | `GPL-1.0-only, GPL-1.0-or-later, GPL-2.0-only, GPL-2.0-or-later, GPL-3.0-only, GPL-3.0-or-later, GFDL-1.1-only, GFDL-1.1-or-later, GFDL-1.3-only, GFDL-1.3-or-later, Sleepycat, Ruby, QPL-1.0, Sendmail` | `extend` | `any` | — | deps.license | `e2.7` |
| `deps.license.network_copyleft` | `string_list` | `AGPL-1.0-only, AGPL-1.0-or-later, AGPL-3.0-only, AGPL-3.0-or-later, AGPL-3.0, SSPL-1.0, OSL-3.0, EUPL-1.1, EUPL-1.2, CPAL-1.0, CECILL-2.1` | `extend` | `any` | — | deps.license | `e2.7` |
| `deps.license.library_layers` | `string_list` | `library, lib, core, sdk, api` | `extend` | `any` | — | deps.license | `e2.7` |
| `deps.license.strong_copyleft_in_library` | `string` | `deny` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.strong_copyleft_elsewhere` | `string` | `warn` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.weak_copyleft_in_library` | `string` | `warn` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.weak_copyleft_elsewhere` | `string` | `allow` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.network_copyleft_in_library` | `string` | `deny` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.network_copyleft_elsewhere` | `string` | `deny` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.unknown_layer_policy` | `string` | `strict` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.ignore_dev` | `boolean` | `False` | `replace` | `any` | — | deps.license | `e2.7` |
| `deps.license.card.max_rows` | `integer` | `12` | `replace` | `any` | rows | deps.license | `e2.7` |
| `deps.provenance.require_intent` | `boolean` | `True` | `replace` | `any` | — | deps.provenance | `e2.7` |
| `deps.provenance.intent_sources` | `string_list` | `agent-intent-trailer, plan-item, review-note, manual-note` | `extend` | `any` | — | deps.provenance | `e2.7` |
| `deps.provenance.min_confidence` | `number` | `0.5` | `replace` | `any` | 0-1 | deps.provenance | `e2.7` |
| `deps.provenance.agent_counts_as_intent` | `boolean` | `False` | `replace` | `any` | — | deps.provenance | `e2.7` |
| `deps.provenance.search_previous_episodes` | `boolean` | `True` | `replace` | `any` | — | deps.provenance | `e2.7` |
| `deps.provenance.card.max_rows` | `integer` | `12` | `replace` | `any` | rows | deps.provenance | `e2.7` |
| `deps.card.max_findings` | `integer` | `20` | `replace` | `any` | findings | deps.card | `e2.7` |
| `deps.card.max_evidence` | `integer` | `4` | `replace` | `any` | lines | deps.card | `e2.7` |
| `deps.findings.new_direct_dependency` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.provenance_unknown` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.intent_not_captured` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.vcs_source` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.new_source` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_critical` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_high` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_moderate` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_low` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_fix_available` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_unchecked` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_cache_stale` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.advisory_cache_missing` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.license_denied` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.license_flagged` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.license_unknown` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.license_drift` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `deps.findings.license_added_to_set` | `boolean` | `True` | `replace` | `any` | — | deps.delta | `e2.7` |
| `risk.weights.<weight>` | `number` | `blast=0.3, size=0.4, test_ratio=0.3` | `deep` | `any` | — | findings.detect | `e2.9` |
| `risk.tau` | `integer` | `40` | `replace` | `any` | score | risk.score | `e2.10` |
| `detectors.enabled` | `string_list` | `test_deletion, untested_new_logic, debug_leftover` | `extend` | `any` | — | findings.detect | `e2.7` |
| `detectors.disable` | `string_list` | `—` | `extend` | `any` | — | findings.detect | `e6.7` |
| `detectors.suppress` | `string_list` | `—` | `extend` | `any` | — | findings.detect | `e4.3` |
| `detectors.max_findings` | `integer` | `12` | `replace` | `any` | rows | findings.detect | `e4.3` |
| `detectors.untested.min_added_lines` | `integer` | `20` | `replace` | `any` | lines | findings.detect | `e4.3` |
| `detectors.todo.min_new` | `integer` | `1` | `replace` | `any` | markers | findings.detect | `e4.3` |
| `detectors.churn.min_episodes` | `integer` | `3` | `replace` | `any` | episodes | findings.detect | `e4.3` |
| `detectors.rework.min_overlap` | `number` | `0.6` | `replace` | `any` | 0-1 | findings.detect | `e4.3` |
| `detectors.shotgun.min_clusters` | `integer` | `4` | `replace` | `any` | clusters | findings.detect | `e4.3` |
| `detectors.copy.min_block_lines` | `integer` | `12` | `replace` | `any` | lines | findings.detect | `e4.3` |
| `events.enabled` | `boolean` | `False` | `replace` | `any` | — | — | `e6.7` |
| `events.on` | `string_list` | `—` | `extend` | `any` | — | — | `e6.7` |
| `blast.hops` | `integer` | `1` | `replace` | `any` | hops | blast_radius | `e2.10` |
| `blast.confidence.graph` | `number` | `0.9` | `replace` | `any` | 0-1 | blast_radius | `e2.10` |
| `blast.confidence.heuristic` | `number` | `0.4` | `replace` | `any` | 0-1 | blast_radius | `e2.10` |
| `blast.confidence.narrow_census` | `number` | `0.7` | `replace` | `any` | multiplier | blast_radius | `e2.10` |
| `blast.heuristic.same_directory` | `number` | `0.6` | `replace` | `any` | score | blast_radius | `e2.10` |
| `blast.heuristic.test_pair` | `number` | `0.5` | `replace` | `any` | score | blast_radius | `e2.10` |
| `blast.heuristic.sibling_directory` | `number` | `0.3` | `replace` | `any` | score | blast_radius | `e2.10` |
| `blast.heuristic.package_prefix` | `number` | `0.15` | `replace` | `any` | score | blast_radius | `e2.10` |
| `blast.heuristic.min_score` | `number` | `0.3` | `replace` | `any` | score | blast_radius | `e2.10` |
| `blast.max_rows` | `integer` | `10` | `replace` | `any` | rows | blast_radius | `e2.10` |
| `blast.max_candidates` | `integer` | `5000` | `replace` | `any` | candidates | blast_radius | `e2.10` |
| `card.max_figures` | `integer` | `6` | `replace` | `any` | rows | card.compose | `e2.10` |
| `card.max_findings` | `integer` | `8` | `replace` | `any` | rows | card.compose | `e2.10` |
| `card.max_evidence` | `integer` | `3` | `replace` | `any` | lines | card.compose | `e2.10` |
| `card.attention.severity` | `string` | `notable` | `replace` | `any` | — | card.compose | `e2.10` |
| `card.state.digits` | `integer` | `3` | `replace` | `any` | digits | card.compose | `e2.10` |
| `card.sections.what` | `boolean` | `True` | `replace` | `any` | — | card.compose | `e2.10` |
| `card.sections.where` | `boolean` | `True` | `replace` | `any` | — | card.compose | `e2.10` |
| `card.sections.dependencies` | `boolean` | `True` | `replace` | `any` | — | card.compose | `e2.10` |
| `card.sections.surface` | `boolean` | `True` | `replace` | `any` | — | card.compose | `e2.10` |
| `card.sections.tests` | `boolean` | `True` | `replace` | `any` | — | card.compose | `e2.10` |
| `card.sections.blast` | `boolean` | `True` | `replace` | `any` | — | card.compose | `e2.10` |
| `card.sections.attention` | `boolean` | `True` | `replace` | `any` | — | card.compose | `e2.10` |
| `rename.threshold` | `integer` | `60` | `replace` | `any` | percent | diff.rename_detect | `e1.6` |
| `rename.fact_bar` | `number` | `0.75` | `replace` | `any` | similarity | diff.rename_detect | `e2.4` |
| `rename.fact.allow_content` | `boolean` | `False` | `replace` | `any` | — | diff.rename_detect | `e2.4` |
| `rename.guess_floor` | `number` | `0.15` | `replace` | `any` | similarity | diff.rename_detect | `e2.4` |
| `rename.candidates.top_k` | `integer` | `3` | `replace` | `any` | pairs | diff.rename_detect | `e2.4` |
| `rename.candidates.budget` | `integer` | `4000` | `replace` | `any` | pairs | diff.rename_detect | `e2.4` |
| `rename.candidates.copies_harder` | `boolean` | `False` | `replace` | `any` | — | diff.rename_detect | `e2.4` |
| `rename.copies` | `boolean` | `True` | `replace` | `any` | — | diff.rename_detect | `e2.4` |
| `rename.folds` | `boolean` | `True` | `replace` | `any` | — | diff.rename_detect | `e2.4` |
| `rename.content.shingle_lines` | `integer` | `3` | `replace` | `any` | lines | diff.rename_detect | `e2.4` |
| `rename.content.normalise` | `string` | `lines` | `replace` | `any` | mode | diff.rename_detect | `e2.4` |
| `rename.content.min_shared_shingles` | `integer` | `4` | `replace` | `any` | shingles | diff.rename_detect | `e2.4` |
| `rename.card.rows` | `integer` | `12` | `replace` | `any` | rows | diff.rename_detect | `e2.4` |
| `diff.context_lines` | `integer` | `3` | `replace` | `any` | lines | diff.per_file | `e1.6` |
| `episode.boundaries` | `string_list` | `hook, trailer, author` | `replace` | `any` | — | episode.segment | `e1.3` |
| `episode.time_gap_minutes` | `integer` | `30` | `replace` | `any` | minutes | episode.segment | `e1.3` |
| `episode.force_split` | `string_list` | `—` | `extend` | `any` | — | episode.segment | `e1.3` |
| `episode.force_merge` | `string_list` | `—` | `extend` | `any` | — | episode.segment | `e1.3` |
| `attribution.agents.<agent-id>` | `string_list` | `—` | `extend` | `any` | — | episode.attribute | `e4.1` |
| `attribution.markers` | `string_list` | `—` | `extend` | `any` | — | episode.attribute | `e4.1` |
| `attribution.trust_committer` | `boolean` | `True` | `replace` | `any` | — | episode.attribute | `e4.1` |
| `attribution.ambiguous_below` | `number` | `0.5` | `replace` | `any` | confidence | episode.attribute | `e4.1` |
| `intent.sources` | `string_list` | `hook-prompt, hook-intent, agent-intent-trailer, plan-item, task-trailer, prompt-digest` | `replace` | `any` | — | episode.intent | `e4.2` |
| `intent.digest_counts` | `boolean` | `True` | `replace` | `any` | — | episode.intent | `e4.2` |
| `intent.max_text_bytes` | `integer` | `4096` | `replace` | `any` | bytes | episode.intent | `e4.2` |
| `intent.plan_cap` | `integer` | `64` | `replace` | `any` | items | episode.intent | `e4.2` |
| `intent.scope_cap` | `integer` | `64` | `replace` | `any` | globs | episode.intent | `e4.2` |
| `ingest.size_cap_mib` | `integer` | `8` | `replace` | `any` | MiB | ingest.normalize | `e1.2` |
| `cache.budget.mib` | `integer` | `512` | `replace` | `any` | MiB | — | `e1.4` |
| `cache.protect_recent_minutes` | `integer` | `1440` | `replace` | `any` | minutes | — | `e7.3` |
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
| `write.queue_batches` | `integer` | `2` | `replace` | `any` | batches | — | `e7.3` |
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
| `mcp.allow_writes` | `boolean` | `False` | `replace` | `any` | — | — | `e6.4` |
| `mcp.approval_token` | `string` | `` | `replace` | `user` | — | — | `e6.4` |
| `mcp.audit_reads` | `boolean` | `False` | `replace` | `any` | — | — | `e6.4` |
| `mcp.max_response_bytes` | `integer` | `262144` | `replace` | `any` | bytes | — | `e6.4` |
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
  **`e3.10` (`reco-5ht`) landed and changed neither**: the writer kept `.fathom/ratchet.toml` and `warn`, and
  added no keys of its own. The two numbers it needed — the warning window (30 days, `EXPIRY_WARNING_DAYS`)
  and the grace period a grant defaults to (90 days, `DEFAULT_GRACE_DAYS`) — are published constants in
  `fathom_analyze::ratchet` rather than config, because neither is a thing a repository should tune before it
  has a reason to, and a key nobody has argued for is a key whose semantics nobody will defend.
