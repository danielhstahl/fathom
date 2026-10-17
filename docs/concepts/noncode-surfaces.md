# Non-code surface changes: config, env, flags, migrations and CI — the operator-action checklist per episode

- **Status**: Accepted, and implemented (executable half below)
- **Ticket / plan key**: `reco-bjd` (`e2.8`)
- **Implements**: the plan's `e2.8` acceptance criteria; ADR-002 §9 (this is the fifth episode-2
  analysis surface, after `e0.9`'s path table, `e2.1`'s classifier, `e2.2`'s noise half and
  `e2.9`'s test surface); ADR-004 §5.1 (an analyzer declares the config keys it reads); and
  [change-classification.md](change-classification.md), whose per-path class is one of the three
  reasons a file is on a surface here.
- **Executes as**: `src/recommender/noncode/` (`surfaces.py`, `env.py`, `flags.py`,
  `migrations.py`, `ci.py`, `iac.py`, `container.py`, `yamltext.py`, `policy.py`, `episode.py`,
  `checklist.py`, `render.py`, `explain.py`, `corpus.py`, `eval.py`, `cli.py`),
  `tests/test_noncode_contract.py`, and `python3 -m recommender.noncode …`. The Rust home is
  `fathom-analyze::noncode` (team 5), with the card in `fathom-cli::verbs::noncode`.

---

## 1. What this decides

One question per episode, and it is not "what does the diff look like":

> **What changed that is not code, and who has to do something about it before this ships?**

A review that reads only the code misses the half of the change that runs in somebody else's
system: the variable that has to exist in production, the flag that has to be created in a
provider, the migration that has to run before the pod that needs it, the secret that has to
be in the environment before the job that reads it, the base image whose Python moved under
the app. None of those are visible as a failing test, and all of them are visible in the diff
— if somebody looks for them.

So this module looks for them, in six surfaces, and turns what it finds into **one checklist
per episode** where every row is a verb, an owner and a location:

| Surface | What is read | Findings |
| --- | --- | --- |
| **env** | `.env*`, Dockerfile `ENV`/`ARG`, Terraform, compose, Kubernetes `env:`, plus environment *reads* in seven languages and five substitution shapes | `env-added`, `env-removed`, `env-declared-unread`, `env-stale-reference`, `env-renamed`, `env-value-committed` |
| **flag** | flag files with their defaults (`flags.yaml`, LaunchDarkly-style JSON, `config/flags/**`) and call sites in five languages | `flag-added`, `flag-removed`, `flag-default-changed`, `flag-read-no-default` |
| **migration** | nine spellings across SQL, Ruby and Python: plain SQL, goose, sql-migrate, Flyway, Rails, Alembic, Django, knex/Sequelize JS/TS, Prisma | `migration-added`, `migration-forward-only`, `migration-destructive`, `migration-with-model`, `deploy-coordination` |
| **CI** | GitHub Actions (plus pattern-level GitLab/CircleCI/Jenkinsfile) | `ci-secret-new`, `ci-permission-escalation`, `ci-oidc-changed`, `ci-runner-changed`, `ci-trigger-main-push` |
| **IaC** | Terraform, CloudFormation, Kubernetes manifests, Helm values, Compose | `iac-resource-added`, `iac-resource-removed`, `iac-policy-widened` |
| **container** | Dockerfiles, and image references in compose/Kubernetes | `container-base-changed`, `container-base-unpinned`, `container-supply-chain`, `container-privilege-changed` |

Twenty-seven kinds, each with its own switch ([§9](#9-the-knob-table-seventeen-numbers-and-twenty-seven-switches)),
its own severity, its own default owner and its own default action. Three refusals are
load-bearing:

- **A finding with no verb does not exist.** `Finding.__post_init__` raises
  `UnanchoredFinding` when a finding has no name, no path, no verb or no confidence
  ([§8](#8-the-checklist-ac-4)). A row a reviewer cannot turn into a task is a row that gets
  skimmed and forgotten.
- **"Nothing reads this" is scoped, and says its scope.** A diff cannot see the repository.
  The claim is made about the episode's files, at a lower confidence, and the finding prints
  which basis it used ([§4](#4-the-cross-check-not-the-list)).
- **A surface the reader could not read is a partial, not a clean sweep.** A Jenkinsfile, a
  flag file in a shape nobody documented, a workflow whose YAML confuses the scanner — each
  prints as a partial and pulls the episode's stated confidence down
  ([§11](#11-degradation-partials-and-stated-confidence)).

---

## 2. The shape of the input, and why it is injected

```python
Change(path, before, after, change_type, old_path)
Episode(id, files, title, repo_env_refs, repo_flag_reads)
Run(episode, policy, classifier)          # one episode's worth of context, built once
measure(episode, policy) -> NonCodeDelta  # findings + checklist + counts + confidence
```

Detectors take a `Run` and return findings. They never open a file, never read `os.environ`,
never look at a clock — ADR-002 §4.5's line, enforced by an AST test
(`test_analysis_modules_touch_no_io`) that also refuses `time.` and `pathlib.` calls in the
analysis half. Everything a detector needs from outside the episode arrives on the `Episode`:

- `repo_env_refs` / `repo_flag_reads` are the repository's full reference set, supplied by a
  caller that *has* an index. This is the only honest way to answer "does anything still read
  `SEARCH_INDEX_HOST`?", and when `non_code.env.reference_scope = repo` was asked for and the
  caller supplied nothing, the finding says so instead of quietly guessing.
- The file's taxonomy class comes from `e2.1`'s classifier, not from a local guess
  (`test_surface_attribution_uses_the_classifier_for_what_is_code`).

## 3. Which files are on a surface — three reasons, always printed

A file is on a surface when **the classifier says so** (a `.rb` under `db/migrate/` is class
`migration`), **a path table claims it** (`.github/workflows/**` is CI; `non_code.iac.globs`
claims whatever layout you actually use), or **the content says so** (a YAML file with
`kind:` is a manifest whatever its directory; a `.json` with `"name": { "default": … }` is a
flag file). All three reasons are recorded per file and printed by
`fathom noncode explain <path>` beside the classifier's own reason and confidence, because a
reviewer who cannot see *why* a file was pulled into a surface will not trust what was said
about it.

A file can be on several surfaces at once (`docker-compose.yml` is env, IaC and container),
which is why the census counts files per surface and the checklist de-duplicates by
subject rather than by file.

---

## 4. The cross-check, not the list

The env surface is not a list of variables that appeared. It is a **join** between
declarations and references, and the join is where every interesting finding lives:

| Question | Answered by | Confidence |
| --- | --- | --- |
| "did this episode introduce a variable?" | added declaration, or added reference | 0.9 / 0.8 |
| "does anything read the variable this line declares?" | reference set of the episode (or the supplied repository set) | 0.6 alone, 0.9 with the repository half |
| "did a variable disappear while something still reads it?" | removed declaration × surviving reference | 0.95 |
| "is this pair one rename?" | removed × added with token similarity ≥ `non_code.env.rename_similarity` | 0.85 |
| "is this value a credential in the tree?" | committed file × secret-shaped name × non-placeholder value | 0.9 |

Reference parsing is per-language, because `os.Getenv`, `os.environ`, `ENV["X"]`,
`env::var`, `System.getenv`, `$_ENV` and `@Value("${x}")` are seven different regexes and a
generic one finds nothing (Python, Go, Rust, Node, Ruby, Java, PHP, plus shell
`${X:-default}`, compose `${X}`, Kubernetes `$(X)`, Terraform `var.x` and Spring
`${X:default}` substitution). Declarations are per-dialect for the same reason: a dotenv
line, a Dockerfile `ENV`/`ARG`, a compose `environment:` item, a Kubernetes
`- name: / value:` pair and a Terraform `environment { … }` block are five shapes.

The two directions the ticket names are both asserted as separate tests, because each one
fails silently without the other:

- **added in `.env.example`, never read** → `env-declared-unread`, *confirm the reader exists
  before the operator sets it*. The failure mode is an operator setting a variable that does
  nothing, and then trusting it.
- **removed while code still references it** → `env-stale-reference`, located at **the
  reference**, not at the deleted line (the line is gone; `src/search.py:3` is where the
  KeyError happens at boot).

A rename is one row that consumes both halves, so a half-done rename does not silently become
two harmless-looking rows. `AUTH_TOKEN → OIDC_TOKEN` shares one token of two, which is below
the built-in 0.6 and stays two rows; a repository that renames aggressively lowers
`non_code.env.rename_similarity` and gets one row instead. The knob is tested in both
directions.

Values are never reprinted: `env-value-committed` says a live-looking credential is in the
tree and masks it (`sk-live… (48 chars)`), because the card ends up in a terminal, an export
and possibly an issue comment.

---

## 5. Flags: a default is a rollout decision

`flag-default-changed` exists because a diff that flips a flag's default **is** the rollout:
`off` → `on` in `flags.yaml` reaches every process at deploy time, with no canary and no
click. It is `high`, owned by `release`, and its verb is *"confirm this is the rollout, not a
typo"*. A flag that ships default-`on` is called the same thing (`this is on for everybody
the moment the code ships`).

The other three are the bookkeeping that keeps the provider honest: a new flag nobody created
(`flag-read-no-default` — "somebody has to create the flag; the code review is not that
somebody"), a flag whose declaration vanished while a read remains, and a flag the code no
longer reads (`flag-removed` — "an orphan flag row in a panel is a lie about the code"). A
removed flag prints its location as the **file** with `(was at checkout.py:5)`: pointing a
reader at a line that no longer exists is worse than pointing at the file.

---

## 6. Migrations: reversible, destructive, and whose turn it is

Three separate questions, because a reviewer needs to know which one they are answering:

1. **Can this be undone?** `migration-forward-only` names the dialect's own evidence — no
   `` `def down` ``, a `` `def downgrade()` `` that only says `pass`, no `-- +goose Down`
   section, no down marker at all. "Not reversible" is not a shrug; its verb is *plan the
   rollback by hand or write the down*.
2. **Does the forward path destroy data?** `migration-destructive` scans **the forward path
   only** — a `DROP INDEX` under `-- +goose Down`, or inside `def downgrade()`, is the
   rollback, and reporting it as the risk would be calling the thing that makes a migration
   safe the thing that makes it dangerous. Statements are matched against
   `non_code.migration.destructive_ops`, which includes the Rails/Django spellings
   (`remove_column`, `change_column_null`) as well as SQL.
3. **Whose turn is it?** `deploy-coordination` fires when the episode contains **migrations
   and nothing else** — no source, no test, by `e2.1`'s class. The warning is explicit, in
   the ticket's own words:

   > **needs deploy coordination** — this episode changes durable state and nothing else, so
   > it needs deploy coordination: the database will be ahead of the running code until
   > somebody ships the other half

   with the evidence that proves the claim (`episode files:`, `no source or test change in
   this episode`, `counts: destructive=3, forward_only=1, migrations=1, reversible=0`) and an
   owner of `release`. `migration-with-model` is the mirror case — the migration and the ORM
   definitions moved together, which is fine and needs *sequencing* (expand, migrate,
   contract) rather than coordination.

---

## 7. CI, infrastructure and containers

These four findings in the CI surface are the ones an infra team asks for by name:

- **`ci-secret-new`** — a `${{ secrets.X }}` reference appearing for the first time, with the
  **scope** the workflow expects (`jobs bound to environments: production`) because
  environment-scoped and repository-scoped are different tickets.
- **`ci-permission-escalation`** — a `permissions:` scope that gained `write`, named as the
  scope (`id-token`, `contents`), at job or top level, owned by `security`. `write-all` is in
  the escalating list because it is the escalation that does not name what it escalates.
- **`ci-oidc-changed`** — a new external identity: a `role-to-assume`, a cloud login action,
  or a job moving to a different `environment`. One row per workflow, with every new marker in
  its evidence: the role ARN and the environment binding are one review, not four rows.
  `id-token: write` deliberately lives in the *permissions* finding instead: what the token
  may do and whose identity it takes are different questions with different owners.
- **`ci-runner-changed`** (`ubuntu-latest` → `self-hosted` is "your job now runs on a machine
  somebody else maintains") and **`ci-trigger-main-push`** (a new `push` trigger on the
  default branch, i.e. every merge now runs this job).

The IaC surface reports resources by **address** (`aws_s3_bucket.audit_logs`,
`Deployment/worker`) rather than by line, because the plan is the artifact an operator reads;
a removed resource is `high` ("confirm the destroy, or move it out of state first"), and
`iac-policy-widened` reads only **added** lines for the patterns that widen an exposure
(`0.0.0.0/0`, `::/0`, `"Action": "*"`, `associate_public_ip_address = true`). An existing
`0.0.0.0/0` on an untouched line is not this episode's news, and a corpus trap keeps that
true.

The container surface treats a base image as a dependency: `container-base-changed` says
**which part moved** (vendor vs major, `python:3.11` → `python:3.13` is a major move even
though the first token is the same), `container-base-unpinned` is a `note` about
reproducibility (a tag can be re-pointed; a digest cannot), `container-supply-chain` catches
`curl … | sh` in a build, and `container-privilege-changed` fires on `USER root` and on
compose `privileged: true`.

---

## 8. The checklist (AC 4)

Every finding carries `action` and `owner`; `checklist.build()` rolls the episode's findings
into the artifact an operator actually works from:

```
requires operator action: yes (5 actions)
needs deploy coordination: yes — this episode changes durable state and needs an ordering
  decision before it ships
  [ ] high     release   needs deploy coordination — decide the order, the window and who
  [ ] high     database  run this migration before the code that needs it; say who runs it
  [ ] high     database  check the lock this takes and the rows it touches before running it
  …
  owners: database 5 · release 1 · confidence: 0.89 (severity-weighted mean)
```

Four rules, each with a test:

- **One row per (verb, subject).** Two workflows both referencing `secrets.SLACK_WEBHOOK` for
  the first time is *one* row with `from_findings = 2`, not two rows saying the same thing.
  Nothing is lost: `totals["findings"]` still counts the findings.
- **Ordering is by severity, then surface, then path** — what burns first, not which file was
  edited first.
- **A cap that names its remainder.** Rows past `non_code.card.max_actions` print as
  `… 30 more: fathom noncode checklist ep-demo --all`, and the cap is a config key.
- **Suppressed findings are counted, never silently missing.** A finding whose switch is off
  is still *computed* (the detectors run with everything enabled and `measure` filters), so
  "2 `ci-secret-new` finding(s) not reported: `ci-secret-new` is off (repo fathom.toml)" is a
  count of real work.

`render.audit` and `checklist.audit` run on the produced text — from the CLI itself, and
again from the contract suite — and fail a render that dropped a checkbox, dropped a
location, lost an owner, reprinted a secret, or failed to explain a suppressed count.

---

## 9. The knob table: seventeen numbers and twenty-seven switches

Every threshold is an `e0.8` key, so a repository can disagree with the built-in answer in
`fathom.toml`, have it layer, and have the card name the layer that won:

| Key | Default | What it decides |
| --- | --- | --- |
| `non_code.env.declaration_globs` | `.env.example` and friends | where a *declaration* lives |
| `non_code.env.committed_globs` | `.env`, `**/secrets.yaml` | where a value is a real value, not documentation |
| `non_code.env.placeholder_values` | `changeme`, `<secret>`, … | values that read as documentation |
| `non_code.env.secret_name_pattern` | `(?i)…(_KEY|TOKEN|DSN)…` | which names mean "secret store" |
| `non_code.env.reference_scope` | `episode` | what "nothing reads this" may be claimed about |
| `non_code.env.rename_similarity` | `0.6` | when a removed+added pair is one rename |
| `non_code.flags.globs` / `.require_default` | flag files / `true` | where defaults live; whether a fallback is mandatory |
| `non_code.migration.globs` / `.model_globs` | the eight layouts / ORM + `db/schema.rb` | what is a migration; what is the model half |
| `non_code.migration.destructive_ops` | SQL + Rails/Django statements | what counts as destroying data |
| `non_code.ci.default_branches` | `main`, `master` | which triggers mean "every merge" |
| `non_code.ci.escalating_permissions` | `contents:write`, … | which `permissions:` moves are escalations |
| `non_code.iac.globs` / `.widened_patterns` | Terraform/CFN/K8s/Helm | exposure text that matters on an added line |
| `non_code.container.require_digest_pin` | `true` | whether a tag without a digest is a finding |
| `non_code.card.max_actions` | `12` | checklist rows per card before the remainder is named |

Plus `non_code.findings.<kind>` for each of the 27 kinds, and the pre-existing
`non_code.patterns` (which declares a path a non-code surface regardless of its class).
`fathom noncode policy` prints every key with its winning layer;
`fathom noncode definition` prints the thresholds as sentences — so the document, the schema
and the running numbers are one artifact, and the schema is diffed against
`schemas/fathom.schema.json` in CI like every other key.

---

## 10. The labelled corpus and its gates

`fathom noncode score` gates a hand-labelled corpus against the **built-in** policy:

| Gate | Bar | Labelled |
| --- | --- | --- |
| rule accuracy | **1.000** | 37 fixtures, one per finding kind (several kinds have two, for a second dialect) |
| precision | **1.000** | 12 traps that must stay silent (a commented-out `DROP`, `changeme`, a `0.0.0.0/0` on an untouched line, a whitespace-only flag change, a pinned base nobody touched, a whole-docs episode) |
| episode accuracy | **1.000** | 5 whole episodes: migration-only, migration+code, six-surface, docs-only, compose+Kubernetes |
| surfaces covered | **6** | every surface has fixtures that fire |

A bar of 1.0 on a hand-labelled corpus is deliberate: the labels are the argument, and a gate
below 1.0 on your own labels is a gate with a hole in it. `test_every_finding_kind_has_its_own_fixture`
derives the expectation from the rule table, so adding a finding kind without a fixture fails
CI. `test_turning_off_any_finding_kind_removes_it_and_leaves_a_trace` runs every one of the 27
switches against its own fixture, so a switch that does nothing cannot ship.

---

## 11. Degradation, partials and stated confidence

A card that says "nothing" when it means "I could not read this" is the failure mode this
surface exists to avoid — a migration in an unread dialect, a Jenkinsfile, a flag file in an
undocumented shape. So:

- every file a reader could not fully read produces **one** partial line (a file is one
  limitation, not eight), and the card prints it;
- stated confidence for a *clean* episode is `1.0`, and `1 − 0.1 × partials`, floored at
  `0.5`, when something could not be read — so the "nothing found" case is a weaker claim
  that says it is weaker;
- when findings *do* exist, confidence is the severity-weighted mean of the findings' own
  confidences, and every finding's confidence is a number in the rule table or a documented
  step in the detector, never a shrug.

An episode with nothing on it says `nothing to do: no non-code surface this tool can see was
changed` and still prints its confidence, which is the difference between a clean result and
an empty one.

---

## 12. The card

Abridged from `fathom noncode demo` (which lifts the row cap on purpose, and says so on the
card):

```
──────── non-code surfaces · ep-demo
  this episode changes env, flag, migration, ci, iac, container and needs 33 operator action(s)
  ep-demo · non-code surfaces · env 7 · flag 6 · migration 5 · ci 6 · iac 5 · container 4 · 33
  findings · 33 need an operator · 15 blocking
  files: 9 changed · 9 on a non-code surface (env 4 · flag 1 · migration 1 · ci 1 · iac 1 · container 1)
  findings by severity: high 15 · notable 17 · note 1
  findings (33)
    env
      !! Environment variable renamed: DATABASE_URL → PG_DATABASE_URL · `DATABASE_URL` became
      `PG_DATABASE_URL` and 1 reference(s) still use the old name · at .env.example:2 · [dotenv]
        · removed at .env.example:2 (dotenv)
        · added at .env.example:2 (dotenv)
        · name similarity 1.00 ≥ non_code.env.rename_similarity = 0.6
        · still read at src/db.py:3 via `os.environ["DATABASE_URL"`
    migration
      !! Database migration added: 20261017120000: forward-only · new ruby migration
      (20261017120000), with 3 destructive statement(s) in the forward path · at
      db/migrate/20261017120000_add_orders_to_invoices.rb:1 · [ruby]
  requires operator action: yes (33 actions)
  [ ] high release repoint the remaining reads of DATABASE_URL to PG_DATABASE_URL and set
      PG_DATABASE_URL in the environment · — DATABASE_URL → PG_DATABASE_URL · at .env.example:2 ·
      (confidence 0.85)
  … 21 more: fathom noncode checklist ep-demo --all
  owners: database 5 · platform 10 · release 9 · security 7 · sre 2
  confidence: 0.89 (severity-weighted mean of the 33 findings' own confidence)
```

Verbs: `card`, `checklist`, `findings`, `explain`, `surfaces`, `policy`, `definition`, `score`,
`bench`, `demo`. A pair of revisions is supplied three ways — `--before/--after` files,
`--repo` plus `--old-repo` trees, or a `--manifest` JSON (inline or base64) which is the shape
`e2.11` hands to consumers, and it carries `repo_env_refs` / `repo_flag_reads` for the
repository-wide half of the cross-check.

Cost: ~5 ms per episode, ~6 ms per card (`bench`), so this runs per episode per repaint.

---

## 13. Ownership, and what this document refuses

`e2.8` reads **config-shaped** changes. It refuses:

- **executing anything**. Nothing here runs `terraform plan`, `alembic`, a Docker build or a
  workflow. Statements are read, not evaluated; `expect aws_s3_bucket.audit_logs in the plan`
  is a checklist row, not a plan invocation.
- **connecting to a provider**. Flag defaults are read from files. Whether `beta_search`
  exists in LaunchUnleashed is a fact about a vendor's database, and the finding says
  *create it in the provider* rather than pretending to have checked.
- **secrets retrieval or masking theater beyond reprinting**. The value is masked in the card;
  rotating it is a human action with a rotation runbook, which is why the verb is `rotate`.
- **claiming repository-wide truth from an episode**. That is what `repo_env_refs` is for, and
  when it was asked for and not given, the finding says so at 0.6 instead of hiding it.
- **a parser per dialect where a shape is enough**. The migration dialects are recognised by
  their down-marker spellings and statement shapes, not by full ASTs. A `goose` file is not
  parsed into Go, and the card does not claim it was.

## 14. The Rust seam

`fathom-analyze::noncode` (team 5) owns the detectors; `fathom-cli::verbs::noncode` owns the
card and the checklist. The seams that matter are the ones this package already treats as
inputs rather than globals: the classifier (`e2.1`), the taxonomy (`e0.9`), the glob matcher
and layer loader (`e0.8`), and the episode's reference set supplied by the index
(`e1.x`). `survey_version` and `delta_version` are in the JSON so a cached card invalidates
when a rule moves, and `config_digest` is ADR-004 §5.5's digest over the **45 keys this
package reads** — deliberately not the store manifest's shorter list (which declares only
`non_code.patterns`), because a cache key that ignores 44 of the keys an answer used is a
stale card waiting to happen.
