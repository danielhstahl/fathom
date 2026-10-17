"""The findings this ticket can raise, the operator action each one demands, and every
number that decides whether one fires (`e2.8` / `reco-bjd`).

Two tables live here, and the difference between them and `e2.9`'s is the point of this
package. A test-surface finding is addressed to the author — *you deleted a test*. A
non-code finding is addressed to somebody who is not in the code review: the person who
sets the environment, the person who runs the migration, the person who owns the runner
and the person who holds the OIDC trust. So every finding here carries an **action** and an
**owner** as well as a name and a location, and :class:`Finding` refuses to exist without
them (:class:`UnanchoredFinding`). "A new environment variable was added" is a fact; "set
`PAYMENT_WEBHOOK_SECRET` in every environment before this ships, owner: platform" is an
action, and only the second one survives the trip from a review to an operator.

**The finding table.** Twenty-seven kinds across six surfaces — env (6), flag (4), migration
(5), CI (5), IaC (3), container (3). Each has a config key, a default and a severity:

| surface | ids |
| --- | --- |
| env | `env-added`, `env-removed`, `env-declared-unread`, `env-stale-reference`, `env-renamed`, `env-value-committed` |
| flag | `flag-added`, `flag-removed`, `flag-default-changed`, `flag-read-no-default` |
| migration | `migration-added`, `migration-forward-only`, `migration-destructive`, `migration-with-model`, `deploy-coordination` |
| ci | `ci-secret-new`, `ci-permission-escalation`, `ci-oidc-changed`, `ci-runner-changed`, `ci-trigger-main-push` |
| iac | `iac-resource-added`, `iac-resource-removed`, `iac-policy-widened` |
| container | `container-base-changed`, `container-base-unpinned`, `container-supply-chain`, `container-privilege-changed` |

**The knob table.** Seventeen numbers, globs and lists, declared in `e0.8`'s schema so they
layer, validate and `config explain` like every other key. The ones that decide whether a
finding fires at all:

```
non_code.env.reference_scope        episode  how widely "nothing reads this" may be claimed
non_code.env.rename_similarity      0.6     token similarity that calls a −/+ pair a rename
non_code.env.secret_name_pattern    (?i)(secret|token|…)   names that mean "secret store"
non_code.env.placeholder_values     changeme, your-value-here, …   values that are not a leak
non_code.env.declaration_globs      .env.example, *.sample, …      files that *declare* vars
non_code.flags.globs                flags.yaml, features.json, …   files that *declare* flags
non_code.flags.require_default      true   a read with no fallback is worth a row
non_code.migration.globs            migrations/**, db/migrate/**, …
non_code.migration.model_globs      models.py, db/schema.rb, *.prisma, …
non_code.migration.destructive_ops  DROP TABLE, SET NOT NULL, …
non_code.ci.default_branches        main, master
non_code.ci.escalating_permissions  contents:write, id-token:write, …
non_code.iac.globs                  *.tf, *.tfvars, cloudformation, k8s manifests
non_code.iac.widened_patterns       *, 0.0.0.0/0, AdministratorAccess
non_code.container.require_digest_pin  true
non_code.card.max_actions           12    rows before the checklist names its remainder
```

Precedence is `e0.8`'s, unchanged: flags > env > repo `fathom.toml` > user config >
built-in default. Booleans merge `replace`, so a repository can actually say no — including
to a whole finding kind, which is a trace on the checklist rather than a silence.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field, replace

from recommender.config.globs import matching as _glob_matching

#: The `non_code.patterns` built-in default, restated here from `e0.8`'s key spec so this
#: package can read the key without importing the layer half. A contract test refuses drift
#: between this tuple and the schema default.
NON_CODE_PATTERNS: tuple[str, ...] = ("**/migrations/**", ".github/workflows/**")

SEVERITY_HIGH = "high"
SEVERITY_NOTABLE = "notable"
SEVERITY_NOTE = "note"
SEVERITY_ORDER: tuple[str, ...] = (SEVERITY_HIGH, SEVERITY_NOTABLE, SEVERITY_NOTE)

#: The six surfaces the ticket names, in the order a checklist prints them.
SURFACE_ENV = "env"
SURFACE_FLAG = "flag"
SURFACE_MIGRATION = "migration"
SURFACE_CI = "ci"
SURFACE_IAC = "iac"
SURFACE_CONTAINER = "container"
SURFACES: tuple[str, ...] = (SURFACE_ENV, SURFACE_FLAG, SURFACE_MIGRATION, SURFACE_CI,
                             SURFACE_IAC, SURFACE_CONTAINER)

#: Who has to act. Printed on every action row, because "somebody should" is not an owner.
OWNER_RELEASE = "release"
OWNER_PLATFORM = "platform"
OWNER_DATABASE = "database"
OWNER_SECURITY = "security"
OWNER_SRE = "sre"
OWNERS: tuple[str, ...] = (OWNER_RELEASE, OWNER_PLATFORM, OWNER_DATABASE, OWNER_SECURITY,
                           OWNER_SRE)


@dataclass(frozen=True)
class Rule:
    """One row of the finding table."""

    id: str
    title: str
    surface: str
    config_key: str
    default: bool
    severity: str
    doc: str
    detector: str
    action: str
    owner: str

    @property
    def phrase(self) -> str:
        return self.title.lower()


def _rule(id_: str, title: str, surface: str, severity: str, doc: str, detector: str,
          action: str, owner: str) -> Rule:
    return Rule(id=id_, title=title, surface=surface, config_key="non_code.findings.%s"
                % id_.replace("-", "_"), default=True, severity=severity, doc=doc,
                detector=detector, action=action, owner=owner)


RULES: tuple[Rule, ...] = (
    # --- environment variables ------------------------------------------------
    _rule("env-added", "Environment variable added", SURFACE_ENV, SEVERITY_NOTABLE,
          "A variable this episode introduces, either declared in an env file or read by new "
          "code. The process environment is not in the repository, so nothing about the diff "
          "makes it exist in production.",
          "env.declarations", "set it before deploy", OWNER_PLATFORM),
    _rule("env-removed", "Environment variable removed", SURFACE_ENV, SEVERITY_NOTE,
          "A variable that is no longer declared or read. Retiring it is the operator's job: "
          "a value left in a secret store with nothing reading it is a stale credential with "
          "an expiry nobody watches.",
          "env.declarations", "retire the value", OWNER_PLATFORM),
    _rule("env-declared-unread", "Environment variable declared but never read",
          SURFACE_ENV, SEVERITY_NOTABLE,
          "Declared in `.env.example` (or a ConfigMap, compose block or workflow `env:`) with "
          "no read found for it. Either the code that reads it has not landed, or the "
          "declaration is theatre and the operator will set a value that changes nothing.",
          "env.crosscheck", "confirm the reader exists", OWNER_PLATFORM),
    _rule("env-stale-reference", "Code still reads a removed environment variable",
          SURFACE_ENV, SEVERITY_HIGH,
          "The declaration is gone and a reference is not. This is the rename that was only "
          "half done: the process starts, reads nothing, and fails at the first request that "
          "needed it rather than at boot.",
          "env.crosscheck", "fix the reference or restore the variable", OWNER_RELEASE),
    _rule("env-renamed", "Environment variable renamed", SURFACE_ENV, SEVERITY_HIGH,
          "One variable removed and a similar one added in the same episode, with the old name "
          "still referenced. Renames are the failure mode of a diff-shaped tool: both halves "
          "look harmless alone.",
          "env.crosscheck", "repoint the references and the deployment config",
          OWNER_RELEASE),
    _rule("env-value-committed", "Real value committed to the tree", SURFACE_ENV,
          SEVERITY_HIGH,
          "A non-placeholder value for a secret-shaped name landed in a committed file. The "
          "value is now in every clone and every object the history holds.",
          "env.declarations", "rotate the credential and move it to the secret store",
          OWNER_SECURITY),
    # --- feature flags --------------------------------------------------------
    _rule("flag-added", "Feature flag added", SURFACE_FLAG, SEVERITY_NOTABLE,
          "A new flag with the default value the code will see if the provider never answers. "
          "The default is the rollout plan whether the author meant it as one or not.",
          "flags.parses", "create the flag in the provider before deploy", OWNER_RELEASE),
    _rule("flag-removed", "Feature flag removed", SURFACE_FLAG, SEVERITY_NOTABLE,
          "A flag the code no longer reads, or a declaration that went while reads remained. "
          "Flag rows in a provider outlive the code that read them and confuse whoever "
          "inherits the panel.",
          "flags.parses", "delete the flag and its rollouts", OWNER_RELEASE),
    _rule("flag-default-changed", "Feature flag default changed", SURFACE_FLAG, SEVERITY_HIGH,
          "The default flipped. When the default is what ships, this diff *is* the rollout, "
          "and it happens to 100 % of traffic at the moment the deploy finishes.",
          "flags.parses", "confirm this is the rollout, not a typo", OWNER_RELEASE),
    _rule("flag-read-no-default", "Feature flag read with no default", SURFACE_FLAG,
          SEVERITY_NOTE,
          "The call site supplies no fallback, so the provider is on the request path: a "
          "missing flag or a provider outage becomes the caller's problem.",
          "flags.parses", "make sure the flag exists before the code ships", OWNER_RELEASE),
    # --- database migrations --------------------------------------------------
    _rule("migration-added", "Database migration added", SURFACE_MIGRATION, SEVERITY_HIGH,
          "An ordered, execute-once change to durable state. It runs against a database that "
          "is still serving traffic, in an order the deploy has to get right.",
          "migrations.parses", "schedule the migration before the code rolls",
          OWNER_DATABASE),
    _rule("migration-forward-only", "Migration is not reversible", SURFACE_MIGRATION,
          SEVERITY_NOTABLE,
          "A forward path with no down path: no `-- +goose Down`, no `def down`, no "
          "`.down.sql`, no undo script. A rollback of the release is now a manual repair with "
          "a different author.",
          "migrations.reversibility", "plan the rollback by hand or write the down",
          OWNER_DATABASE),
    _rule("migration-destructive", "Migration destroys or rewrites data", SURFACE_MIGRATION,
          SEVERITY_HIGH,
          "A `DROP`, `TRUNCATE`, `SET NOT NULL`, type change or rename in the forward path. "
          "These take locks, and some of them cannot be undone by anything.",
          "migrations.destructive", "check locks, size and backup before running it",
          OWNER_DATABASE),
    _rule("migration-with-model", "Migration and model change in one episode",
          SURFACE_MIGRATION, SEVERITY_NOTABLE,
          "The schema file and the code that reads it move together, which is correct and "
          "also an ordering problem: one of the two will be true before the other.",
          "migrations.companion", "sequence the deploy (expand, migrate, contract)",
          OWNER_RELEASE),
    _rule("deploy-coordination", "Episode needs deploy coordination", SURFACE_MIGRATION,
          SEVERITY_HIGH,
          "The episode changes only durable-state surface: nothing here is safe to ship on the "
          "normal path, because there is no code for the migration to be compatible with.",
          "migrations.companion", "needs deploy coordination", OWNER_RELEASE),
    # --- CI -------------------------------------------------------------------
    _rule("ci-secret-new", "CI secret referenced for the first time", SURFACE_CI,
          SEVERITY_HIGH,
          "A workflow now reads `${{ secrets.X }}`. The build fails until somebody types that "
          "name into the repository, organisation or environment secrets — a fact nobody "
          "learns until a red tick.",
          "ci.workflows", "create the secret in the same scope", OWNER_PLATFORM),
    _rule("ci-permission-escalation", "CI permissions widened", SURFACE_CI, SEVERITY_HIGH,
          "A `permissions:` block gained a write scope, or `write-all`. The token the job "
          "holds is now worth more than it was, and it is handed to every step in that job.",
          "ci.workflows", "confirm the scope, or narrow it back", OWNER_SECURITY),
    _rule("ci-oidc-changed", "CI identity or trust boundary changed", SURFACE_CI,
          SEVERITY_HIGH,
          "`id-token: write`, a role ARN, an `environment:` binding or a vault login changed: "
          "the workflow can now mint an identity a cloud provider trusts. This is an access "
          "review, not a code review.",
          "ci.workflows", "review who the workflow can now become", OWNER_SECURITY),
    _rule("ci-runner-changed", "CI runner changed", SURFACE_CI, SEVERITY_NOTABLE,
          "A job moved onto a runner this project owns, or onto a new label. Everything the "
          "job can see — network, cache, credentials on the host — changed with it.",
          "ci.workflows", "confirm the runner image and its network", OWNER_PLATFORM),
    _rule("ci-trigger-main-push", "Pipeline now runs on the default branch", SURFACE_CI,
          SEVERITY_NOTABLE,
          "A `push` trigger that includes the default branch appeared, or a job gained one. "
          "Whatever that workflow does — publish, deploy, sign — now happens on merge.",
          "ci.workflows", "confirm publish/deploy side effects", OWNER_RELEASE),
    # --- infrastructure as code ----------------------------------------------
    _rule("iac-resource-added", "Infrastructure resource added", SURFACE_IAC,
          SEVERITY_NOTABLE,
          "A managed resource appeared in a plan. It costs money, has a name that must be "
          "globally unique, and will outlive the branch.",
          "iac.deltas", "expect it in the plan; check name and cost", OWNER_PLATFORM),
    _rule("iac-resource-removed", "Infrastructure resource removed", SURFACE_IAC,
          SEVERITY_HIGH,
          "A managed resource disappeared from the configuration, which in every IaC tool "
          "means *destroy it*. For a bucket, a table or a database that is the data, not the "
          "infrastructure.",
          "iac.deltas", "confirm the destroy, or archive first", OWNER_PLATFORM),
    _rule("iac-policy-widened", "Infrastructure policy widened", SURFACE_IAC, SEVERITY_HIGH,
          "A new line widens an IAM policy, opens an ingress to the internet, or makes an "
          "instance public. The blast radius of this diff is not the repository.",
          "iac.deltas", "review the exposure this grants", OWNER_SECURITY),
    # --- containers -----------------------------------------------------------
    _rule("container-base-changed", "Base image changed", SURFACE_CONTAINER, SEVERITY_HIGH,
          "The `FROM` moved: a different vendor, a different major, or a different digest. "
          "Every binary in the image was built against the old one.",
          "container.parses", "rebuild and re-test the image", OWNER_SRE),
    _rule("container-base-unpinned", "Base image not pinned", SURFACE_CONTAINER,
          SEVERITY_NOTE,
          "No digest, or a tag that floats (`latest`, or no tag at all). The image you build "
          "next week is not the image you reviewed today.",
          "container.parses", "pin a digest for reproducibility", OWNER_SRE),
    _rule("container-supply-chain", "Remote code executed during the build",
          SURFACE_CONTAINER, SEVERITY_NOTABLE,
          "A `curl … | sh` in a Dockerfile: the build's reproducibility and the reviewer's "
          "threat model both now include whatever that server returns at build time.",
          "container.parses", "vendor and pin the thing you are installing", OWNER_SECURITY),
    _rule("container-privilege-changed", "Container privilege changed", SURFACE_CONTAINER,
          SEVERITY_NOTABLE,
          "`USER root` added, the last non-root `USER` removed, `--privileged` or a capability "
          "granted. A container escape is now a root escape.",
          "container.parses", "confirm the process needs the privilege", OWNER_SECURITY),
)

RULE_INDEX: dict[str, Rule] = {rule.id: rule for rule in RULES}
_BY_KEY_TAIL: dict[str, Rule] = {rule.config_key.rsplit(".", 1)[-1]: rule for rule in RULES}

#: Every finding id, in table order — the print order and the `policy` verb's order.
FINDING_IDS: tuple[str, ...] = tuple(rule.id for rule in RULES)


# ---------------------------------------------------------------------------
# knobs
# ---------------------------------------------------------------------------
#: Built-in globs. A repository adds to these with `extend`, it does not replace them: the
#: thing a detector cannot see is a finding that never fires, so the built-in half is the
#: union of the layouts the corpus proves and never a single team's convention.
DECLARATION_GLOBS: tuple[str, ...] = (
    ".env.example", ".env.sample", ".env.template", ".env.default", ".env.dist",
    "**/.env.example", "**/.env.sample", "**/.env.template", "**/*.env.example",
    "**/*.env.sample", "config/env.example", "docs/env.example",
)

#: Files that *declare* a `.env` key with real-ish values (so a value in them is a leak,
#: while a value in a `.example` is documentation).
COMMITTED_ENV_GLOBS: tuple[str, ...] = (
    ".env", ".env.local", ".env.production", "**/.env", "**/.env.production",
    "**/.env.staging", "config/env.ini", "**/secrets.yaml", "**/secrets.yml",
)

FLAG_GLOBS: tuple[str, ...] = (
    "flags.yaml", "flags.yml", "flags.json", "feature-flags.yaml", "feature-flags.json",
    "**/flags.yaml", "**/flags.yml", "**/flags.json", "**/features.yaml",
    "**/features.json", "**/featureflags.json", "**/flip-flop.json",
    "**/unleash-toggles.json", "config/flags/**", "**/config/flags/**",
    "**/feature_flags.py", "**/flags.php",
)

MIGRATION_GLOBS: tuple[str, ...] = (
    "**/migrations/**", "**/migration/**", "db/migrate/**", "db/migrations/**",
    "**/alembic/versions/**", "**/migrations/versions/**", "prisma/migrations/**",
    "**/flyway/sql/**", "src/main/resources/db/migration/**", "**/migrations/**.sql",
    "**/migrations/**.up.sql", "resource/migrations/**",
)

#: The model half of "the schema and the code that reads it moved together". Narrow on
#: purpose: `**/model/**` is a Java package a reviewer does not want pinged for a typo.
MODEL_GLOBS: tuple[str, ...] = (
    "**/models.py", "**/models/**.py", "**/model.py", "db/schema.rb", "db/schema.sql",
    "app/models/**", "**/entities/**.java", "**/Entity/**.php", "**/*.prisma",
    "**/schema.sql", "**/models/**.go", "internal/model/**.go", "**/Models/**.cs",
    "**/migrations/**/schema.rs",
)

IAC_GLOBS: tuple[str, ...] = (
    "**/*.tf", "**/*.tfvars", "**/*.hcl", "cloudformation*.y*ml", "**/cloudformation*.y*ml",
    "**/cfn-*.y*ml", "k8s/**.y*ml", "**/k8s/**.y*ml", "**/kubernetes/**.y*ml",
    "**/manifests/**.y*ml", "**/helm/**/values.y*ml", "**/Chart.yaml",
    "ansible/**.y*ml", "**/*.stack.yaml", "Pulumi.*.yaml", "**/*.cue",
)

#: Values that are documentation rather than a credential. Compared case-insensitively and
#: after stripping quotes and whitespace, so `SECRET=changeme` is a comment and
#: `SECRET=sk-live-9f2…` is a leak.
PLACEHOLDER_VALUES: tuple[str, ...] = (
    "", "changeme", "change-me", "change_me", "your-value-here", "your_value_here",
    "your-secret-here", "xxx", "xxxx", "xxxxx", "<value>", "<secret>", "placeholder",
    "sample", "example", "examples", "test", "dummy", "todo", "none", "null", "true",
    "false", "0", "1", "secret", "password", "admin", "foo", "bar", "undefined",
    "REPLACE_ME", "insert-here", "redacted",
)

#: Names that mean "this belongs in a secret store", so a *missing* declaration is an
#: operator action and a committed value is a rotation.
SECRET_NAME_PATTERN = (
    r"(?i)(^|_)(secret|token|password|passwd|pwd|api[_-]?key|access[_-]?key|private[_-]?key"
    r"|credential|auth[_-]?key|dsn|_key|_cert|_certificate)($|_)|(^|_)(aws|gcp|azure|oidc)"
    r"(_|-)?(key|secret|token|credential)"
)

#: IaC text that widens an exposure. Matched against *added lines only* — a policy that was
#: already there is not this episode's news, and matching context would turn every touched
#: Terraform file into a security review.
WIDENED_PATTERNS: tuple[str, ...] = (
    '"*"', '"arn:aws:iam::aws:policy/AdministratorAccess"', '"0.0.0.0/0"',
    "::/0", '"Principal": "*"', "associate_public_ip_address = true",
    "cidr_blocks = [\"0.0.0.0/0\"]", "Action = [\"*\"]", "--privileged",
)

#: Destructive statements in a forward migration path, matched case-insensitively against
#: the statement text (not the whole file), and extendable because every shop has its own.
DESTRUCTIVE_OPS: tuple[str, ...] = (
    "drop table", "drop column", "drop index", "drop constraint", "drop database",
    "truncate table", "truncate ", "set not null", "alter column", "modify column",
    "rename to", "delete from", "update ", "alter type", "drop materialized view",
    "drop_table", "remove_column", "remove_index", "change_column_null", "delete_all",
    "destroy_all", "execute ",
)

#: `permissions:` scopes whose move to `write` is an escalation, in `owner:scope` spelling.
ESCALATING_PERMISSIONS: tuple[str, ...] = (
    "contents:write", "actions:write", "security-events:write", "id-token:write",
    "pull-requests:write", "deployments:write", "packages:write", "statuses:write",
    "checks:write", "issues:write", "discussions:write", "pages:write",
    "organization-secrets:write", "organization-administration:write", "write-all",
)

#: Actions/CI steps that mint or assume an external identity — the OIDC surface.
OIDC_MARKERS: tuple[str, ...] = (
    "id-token: write", "id-token: write ", "configure-aws-credentials", "role-to-assume",
    "azure/login", "hashicorp/vault-action", "aws-role-to-assume", "audience:",
    "subject-algorithm", "id_tokens", "identity-provider",
)


@dataclass(frozen=True)
class Knob:
    """One tunable number, glob list or pattern, with its built-in default."""

    name: str
    key: str
    default: object
    kind: str
    units: str = ""
    doc: str = ""


KNOBS: tuple[Knob, ...] = (
    Knob("declaration_globs", "non_code.env.declaration_globs", list(DECLARATION_GLOBS),
         "glob_list", "paths",
         "Files whose whole job is to declare environment variables (`.env.example` and "
         "friends). A key added here is a key an operator must set; a key added only in code "
         "is the same finding with a different location."),
    Knob("committed_globs", "non_code.env.committed_globs", list(COMMITTED_ENV_GLOBS),
         "glob_list", "paths",
         "Files where a *value* is a real value rather than documentation. A `.env` in a "
         "repository is usually an accident; a secret-shaped name in one is a rotation."),
    Knob("placeholder_values", "non_code.env.placeholder_values", list(PLACEHOLDER_VALUES),
         "string_list", "values",
         "Values that read as documentation (`changeme`, `<secret>`, empty). Compared "
         "case-insensitively: this list is what keeps the committed-value finding from "
         "firing on the `.env` in every README."),
    Knob("secret_name_pattern", "non_code.env.secret_name_pattern", SECRET_NAME_PATTERN,
         "string", "regex",
         "Which variable names mean 'belongs in a secret store'. Deliberately a pattern, not "
         "a list: the useful part is the shape (`_KEY`, `TOKEN`, `DSN`), and every company "
         "has names the shape catches that a list does not."),
    Knob("reference_scope", "non_code.env.reference_scope", "episode", "string", "scope",
         "`episode`: 'nothing reads this' is claimed about the files in this episode only, "
         "and the finding says so at a lower confidence. `repo`: the caller supplied the "
         "repository's full reference set, which is the only basis for the confident version "
         "of the claim — and the card says out loud when `repo` was asked for and not given."),
    Knob("rename_similarity", "non_code.env.rename_similarity", 0.6, "number", "similarity",
         "Name-token similarity at which a removed variable and an added one are called one "
         "rename (`DATABASE_URL` → `PG_DATABASE_URL` shares 2 of 3 tokens). Low values merge "
         "unrelated removals into a rename; high values leave a half-done rename as two "
         "harmless-looking rows."),
    Knob("flag_globs", "non_code.flags.globs", list(FLAG_GLOBS), "glob_list", "paths",
         "Files that declare flags with their defaults. Code that *reads* a flag is scanned "
         "wherever it lives, so this list only decides where defaults come from."),
    Knob("require_default", "non_code.flags.require_default", True, "boolean", "",
         "Report a flag read that supplies no fallback. Off for a repository that runs its "
         "provider on the same node and treats a missing flag as a bug in the deploy."),
    Knob("migration_globs", "non_code.migration.globs", list(MIGRATION_GLOBS), "glob_list",
         "paths",
         "Migration layouts. The `e0.9`/`e2.1` taxonomy class `migration` is consulted first "
         "and these only add to it, so a repository with an unusual directory says it once."),
    Knob("model_globs", "non_code.migration.model_globs", list(MODEL_GLOBS), "glob_list",
         "paths",
         "The model half of 'the migration and the model moved together': ORM definitions, "
         "`db/schema.rb`, `*.prisma`. Narrow on purpose — a Java package called `model` that "
         "moves without a migration is not a schema change."),
    Knob("destructive_ops", "non_code.migration.destructive_ops", list(DESTRUCTIVE_OPS),
         "string_list", "statements",
         "Statements treated as destroying or rewriting data in a forward path. Matched "
         "against statement text, case-insensitively, and extendable per repository."),
    Knob("default_branches", "non_code.ci.default_branches", ["main", "master"],
         "string_list", "branches",
         "Which branch names count as 'the default branch' when deciding whether a trigger "
         "means every merge runs this job."),
    Knob("escalating_permissions", "non_code.ci.escalating_permissions",
         list(ESCALATING_PERMISSIONS), "string_list", "scopes",
         "`permissions:` moves that are escalations, in `scope:access` spelling. `write-all` "
         "is in the list because it is the escalation that does not name what it escalates."),
    Knob("iac_globs", "non_code.iac.globs", list(IAC_GLOBS), "glob_list", "paths",
         "Infrastructure-as-code files: Terraform, CloudFormation, Kubernetes manifests, "
         "Helm values, Pulumi, Ansible. A manifest with `kind:` is also detected by content, "
         "because nothing about a path says a YAML file is a Deployment."),
    Knob("widened_patterns", "non_code.iac.widened_patterns", list(WIDENED_PATTERNS),
         "string_list", "patterns",
         "Text that widens an infrastructure exposure when it appears on an added line: a "
         "wildcard principal or action, a default route from anywhere, a public address."),
    Knob("require_digest_pin", "non_code.container.require_digest_pin", True, "boolean", "",
         "Insist that a new or changed base image is pinned by digest. Off in a repository "
         "that rebuilds nightly and considers a floating tag the honest answer."),
    Knob("max_actions", "non_code.card.max_actions", 12, "integer", "rows",
         "Action rows a card prints before it says how many more there are and which verb "
         "lists them. A cap that names its remainder, never a truncation without a trace."),
)

KNOB_INDEX: dict[str, Knob] = {knob.name: knob for knob in KNOBS}
KEY_TO_KNOB: dict[str, Knob] = {knob.key: knob for knob in KNOBS}

#: Config keys this package reads, for the analyzer manifest (ADR-004 §5.1).
CONFIG_KEYS: tuple[str, ...] = (
    "non_code.patterns",
) + tuple(rule.config_key for rule in RULES) + tuple(knob.key for knob in KNOBS)


class UnanchoredFinding(Exception):
    """A finding that cannot name its thing, its place, or its action. Raised, not rendered."""


@dataclass
class Finding:
    """One finding: what changed, where, who acts, and what they have to do.

    The constructor is the contract. A finding without a name, a path or an **action** is
    raised rather than returned, because the whole point of this package is the checklist at
    the end: a row a reviewer cannot turn into a verb is a row that gets skimmed and
    forgotten, and a checklist with one skimmable row in it is not a checklist.
    """

    kind: str
    name: str
    path: str
    surface: str = ""
    line: int = 0
    action: str = ""
    detail: str = ""
    value: str = ""
    old_value: str = ""
    owner: str = ""
    confidence: float = 1.0
    evidence: list[str] = field(default_factory=list)
    old_path: str = ""
    old_line: int = 0
    dialect: str = ""

    def __post_init__(self) -> None:
        if self.kind not in RULE_INDEX:
            raise UnanchoredFinding("unknown finding kind %r" % self.kind)
        rule = RULE_INDEX[self.kind]
        if self.surface and self.surface != rule.surface:
            raise UnanchoredFinding("%s belongs to surface %s, not %s"
                                    % (self.kind, rule.surface, self.surface))
        self.surface = rule.surface
        if not self.name or not self.name.strip():
            raise UnanchoredFinding("%s: a finding must name the thing it is about" % self.kind)
        if not self.path:
            raise UnanchoredFinding("%s %s: a finding must name a path" % (self.kind, self.name))
        if not self.action.strip():
            self.action = self.rule.action
        if not self.action.strip():
            raise UnanchoredFinding("%s %s: a finding must carry an operator action"
                                    % (self.kind, self.name))
        if not 0.0 < self.confidence <= 1.0:
            raise UnanchoredFinding("%s %s: confidence must be in (0, 1]" % (self.kind, self.name))
        if not self.owner:
            self.owner = self.rule.owner
        if not self.evidence:
            self.evidence = [self.detail] if self.detail else ["no further evidence recorded"]

    @property
    def rule(self) -> Rule:
        return RULE_INDEX[self.kind]

    @property
    def severity(self) -> str:
        return self.rule.severity

    @property
    def location(self) -> str:
        return "%s:%d" % (self.path, self.line) if self.line else self.path

    @property
    def old_location(self) -> str:
        if not self.old_path:
            return ""
        return "%s:%d" % (self.old_path, self.old_line) if self.old_line else self.old_path

    @property
    def headline(self) -> str:
        """`what · value`, the part that is not the action."""
        if self.old_value and self.value:
            return "%s: %s → %s" % (self.name, self.old_value, self.value)
        if self.value:
            return "%s: %s" % (self.name, self.value)
        return self.name

    def render(self) -> str:
        parts = ["%s: %s" % (self.rule.title, self.headline)]
        if self.detail:
            parts.append(self.detail)
        parts.append("at %s" % self.location)
        if self.old_location and self.old_location != self.location:
            parts.append("(was at %s)" % self.old_location)
        if self.dialect:
            parts.append("[%s]" % self.dialect)
        return " · ".join(parts)

    def action_line(self) -> str:
        """The checklist row. The checkbox and the owner come first: they are the clickable part."""
        bits = ["[ ] %-8s %-8s %s" % (self.severity, self.owner, self.action)]
        bits.append("— %s" % self.headline)
        bits.append("at %s" % self.location)
        if self.confidence < 0.999:
            bits.append("(confidence %.2f)" % self.confidence)
        return " · ".join(bits)

    def as_dict(self) -> dict:
        return {"kind": self.kind, "title": self.rule.title, "severity": self.severity,
                "surface": self.surface, "name": self.name, "path": self.path,
                "line": self.line, "location": self.location, "action": self.action,
                "owner": self.owner, "detail": self.detail, "value": self.value,
                "old_value": self.old_value, "confidence": round(self.confidence, 3),
                "dialect": self.dialect, "old_path": self.old_path,
                "old_line": self.old_line, "config_key": self.rule.config_key,
                "evidence": list(self.evidence)}


def sort_key(finding: Finding) -> tuple:
    return (SEVERITY_ORDER.index(finding.severity), SURFACES.index(finding.surface),
            finding.path, finding.line, finding.kind, finding.name)


sort_findings = sort_key          # exported name for "order them the same way everywhere"


@dataclass
class Policy:
    """The effective finding policy and knob set for one repository.

    Built the same way `e2.9`'s is: every switch and every number resolved through
    `e0.8`'s five layers, each remembering which one won, so a card can quote the file that
    disagreed with the default.
    """

    enabled: dict[str, bool] = field(default_factory=dict)
    source: dict[str, str] = field(default_factory=dict)
    knobs: dict[str, object] = field(default_factory=dict)
    knob_source: dict[str, str] = field(default_factory=dict)
    patterns: list[str] = field(default_factory=list)
    patterns_source: str = "built-in default"
    config_digest: str = ""
    _regexes: dict[str, re.Pattern] = field(default_factory=dict, repr=False)

    # -- construction -------------------------------------------------------
    @classmethod
    def built_in(cls) -> "Policy":
        return cls(enabled={rule.id: rule.default for rule in RULES},
                   knobs={knob.name: _coerce(knob, knob.default) for knob in KNOBS},
                   patterns=list(NON_CODE_PATTERNS))

    @classmethod
    def from_config(cls, config=None) -> "Policy":
        policy = cls()
        for rule in RULES:
            value, source = _lookup(config, rule.config_key, rule.default)
            policy.enabled[rule.id] = bool(value) if value is not None else rule.default
            policy.source[rule.id] = source
        for knob in KNOBS:
            value, source = _lookup(config, knob.key, knob.default)
            policy.knobs[knob.name] = knob.default if value is None else _coerce(knob, value)
            policy.knob_source[knob.name] = source
        value, source = _lookup(config, "non_code.patterns", list(NON_CODE_PATTERNS))
        policy.patterns = [str(item) for item in (value or [])] or list(NON_CODE_PATTERNS)
        policy.patterns_source = source
        if config is not None:
            # ADR-004 §5.5: the digest names *everything the payload reads*. The tempting
            # call here is `config.digest_for("non_code.surface")`, which is the store
            # manifest's own key list — and that manifest declares only `non_code.patterns`,
            # because it was written before this analyzer had opinions of its own. A card
            # cached behind a digest that ignores 44 of the 45 keys it read is a stale card
            # waiting to happen, so the digest is taken over `CONFIG_KEYS` instead.
            policy.config_digest = config.digest(list(CONFIG_KEYS))
        return policy

    # -- caller overrides ---------------------------------------------------
    def with_knobs(self, source: str = "caller override", **names) -> "Policy":
        """A copy with these knobs overridden, each labelled with where the value came from.

        The corpus uses this to run one fixture under two definitions (say, `reference_scope
        = repo` with and without a supplied reference set) and prove the knob is real.
        """
        clone = self._clone()
        for name, value in names.items():
            knob = KNOB_INDEX[name]
            clone.knobs[name] = _coerce(knob, value)
            clone.knob_source[name] = "%s (%s=%s)" % (source, name, _format(value))
        clone._regexes = {}
        return clone

    def with_flag(self, rule_id: str, enabled: bool,
                  source: str = "caller override") -> "Policy":
        """A copy with one finding kind switched on or off, accepting the id or the key tail."""
        rule = RULE_INDEX.get(rule_id) or _BY_KEY_TAIL.get(rule_id)
        if rule is None:
            raise KeyError("no finding kind %r" % rule_id)
        clone = self._clone()
        clone.enabled[rule.id] = bool(enabled)
        clone.source[rule.id] = source
        return clone

    def _clone(self) -> "Policy":
        return replace(self, enabled=dict(self.enabled), source=dict(self.source),
                       knobs=dict(self.knobs), knob_source=dict(self.knob_source),
                       _regexes={})

    def all_enabled(self) -> "Policy":
        """A copy with every finding kind switched on — the survey-time policy.

        The detectors observe and `measure` filters, so that "3 findings suppressed because
        `non_code.findings.env_removed` is off" is a count of real work rather than a guess
        about work that was skipped. Knobs, sources and digests are carried over untouched:
        this changes which findings are *observed*, never what the numbers say.
        """
        clone = self._clone()
        clone.enabled = {rule.id: True for rule in RULES}
        return clone

    # -- the questions ------------------------------------------------------
    def enabled_for(self, rule_id: str) -> bool:
        return bool(self.enabled.get(rule_id, RULE_INDEX[rule_id].default))

    def effective(self, rule_id: str) -> tuple[bool, str]:
        return self.enabled_for(rule_id), self.source.get(rule_id, "built-in default")

    def knob(self, name: str) -> object:
        return self.knobs.get(name, KNOB_INDEX[name].default)

    def number(self, name: str) -> int | float:
        value = self.knob(name)
        return value if isinstance(value, (int, float)) else KNOB_INDEX[name].default

    def flag(self, name: str) -> bool:
        return bool(self.knob(name))

    def text(self, name: str) -> str:
        return str(self.knob(name) or "")

    def words(self, name: str) -> list[str]:
        value = self.knob(name)
        if isinstance(value, str):
            return [item for item in value.split(",") if item.strip()]
        return [str(item) for item in (value or [])]

    def regex(self, name: str) -> re.Pattern:
        """The named pattern knob, compiled once. A broken pattern falls back to the default.

        A repository that typos `non_code.env.secret_name_pattern` gets the built-in list
        and a note on the card, not a crash in the middle of an episode: a config error is
        reported (the caller collects :func:`problems`), it is never a reason to show nothing.
        """
        if name in self._regexes:
            return self._regexes[name]
        text = self.text(name)
        try:
            compiled = re.compile(text)
        except re.error:
            compiled = re.compile(str(KNOB_INDEX[name].default))
        self._regexes[name] = compiled
        return compiled

    def problems(self) -> list[str]:
        """Config values this package could not use, in the reviewer's words."""
        out: list[str] = []
        for name in ("secret_name_pattern",):
            try:
                re.compile(self.text(name))
            except re.error as error:
                out.append("%s (%s) is not a usable regular expression; the built-in pattern "
                           "was used instead (%s)" % (KNOB_INDEX[name].key, self.text(name),
                                                      error))
        return out

    def matches(self, knob_name: str, path: str) -> str:
        """The first glob from a glob-list knob that claims `path`, `""` if none does."""
        hits = _glob_matching(list(self.knob(knob_name) or []), path) or []
        return hits[0] if hits else ""

    def pattern_that_claims(self, path: str) -> str:
        """The `non_code.patterns` entry that claims `path`, `""` if none does.

        A user assertion, not a detector: it says "treat this path as a non-code surface",
        which is a census fact and a licence to report what the surface detectors find there
        — it is never a substitute for a detector deciding something.
        """
        hits = _glob_matching(list(self.patterns or []), path) or []
        return hits[0] if hits else ""

    def describes_path(self, knob_name: str, path: str) -> bool:
        return bool(self.matches(knob_name, path))

    def is_secret_name(self, name: str) -> bool:
        return bool(self.regex("secret_name_pattern").search(name))

    # -- the text -----------------------------------------------------------
    def describe(self) -> list[str]:
        """`fathom noncode policy`: every switch, every number, every winning layer."""
        out: list[str] = ["non-code surface policy (highest precedence first)"]
        out.append("  findings")
        for rule in RULES:
            enabled, source = self.effective(rule.id)
            out.append("  %-28s %-5s %-44s %s"
                       % (rule.id, "on" if enabled else "off", rule.config_key, source))
        out.append("  definition")
        for knob in KNOBS:
            out.append("  %-24s %-30s %-44s %s"
                       % (knob.name, _clip(_format(self.knob(knob.name)), 30), knob.key,
                          self.knob_source.get(knob.name, "built-in default")))
        out.append("  %-24s %-30s %-44s %s"
                   % ("patterns", _clip(", ".join(self.patterns), 30), "non_code.patterns",
                      self.patterns_source))
        return out

    def definition_lines(self) -> list[str]:
        """Every knob spelled out — printed by `explain --definition`."""
        return ["%s = %s  (%s)" % (knob.name, _format(self.knob(knob.name)), knob.doc)
                for knob in KNOBS]

    def as_dict(self) -> dict:
        return {"findings": {rule.id: {"enabled": self.enabled_for(rule.id),
                                       "source": self.source.get(rule.id, "built-in default"),
                                       "config_key": rule.config_key,
                                       "severity": rule.severity, "surface": rule.surface,
                                       "owner": rule.owner, "action": rule.action}
                             for rule in RULES},
                "knobs": {knob.name: {"value": _jsonable(self.knob(knob.name)), "key": knob.key,
                                      "default": _jsonable(knob.default), "units": knob.units,
                                      "source": self.knob_source.get(knob.name,
                                                                     "built-in default")}
                          for knob in KNOBS},
                "patterns": {"value": list(self.patterns), "key": "non_code.patterns",
                             "default": list(NON_CODE_PATTERNS),
                             "source": self.patterns_source},
                "config_digest": self.config_digest}


def describe_rules() -> list[str]:
    """The finding table on its own, independent of any repository's configuration."""
    out: list[str] = []
    for rule in RULES:
        out.append("%-28s %-8s %-10s %-44s %s"
                   % (rule.id, rule.severity, rule.surface, rule.config_key, rule.doc))
    return out


def _jsonable(value: object) -> object:
    return list(value) if isinstance(value, tuple) else value


def _clip(text: str, width: int) -> str:
    if len(text) <= width:
        return text
    return text[: max(1, width - 1)] + "…"


def _format(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        return ("%.2f" % value).rstrip("0").rstrip(".")
    if isinstance(value, (list, tuple)):
        return ", ".join(str(item) for item in value) or "—"
    return str(value)


def _coerce(knob: Knob, value: object) -> object:
    if knob.kind == "integer":
        try:
            return int(value)
        except (TypeError, ValueError):
            return knob.default
    if knob.kind == "number":
        try:
            return float(value)
        except (TypeError, ValueError):
            return knob.default
    if knob.kind == "boolean":
        if isinstance(value, bool):
            return value
        return str(value).strip().lower() in ("1", "true", "yes", "on")
    if knob.kind in ("glob_list", "string_list"):
        if isinstance(value, str):
            return [item.strip() for item in value.split(",") if item.strip()]
        return [str(item) for item in (value or [])]
    return value


def _lookup(config, key: str, default):
    """(value, source) for one key, from a resolved `Config` or a bare mapping."""
    if config is None:
        return default, "built-in default"
    decision = None
    getter = getattr(config, "decision", None)
    if callable(getter):
        decision = getter(key)
    if decision is not None:
        value = getattr(decision, "value", None)
        if value is None:
            value = default
        return value, _layer_label(getattr(decision, "winner", "defaults"),
                                   getattr(decision, "source", ""),
                                   getattr(decision, "from_defaults", False))
    values = getattr(config, "values", config)
    cursor: object = values
    for segment in key.split("."):
        if not isinstance(cursor, dict) or segment not in cursor:
            return default, "built-in default"
        cursor = cursor[segment]
    return cursor, "config values (unresolved)"


def _layer_label(layer: str, source: str, from_defaults: bool) -> str:
    if from_defaults or layer in ("defaults", "default", ""):
        return "built-in default"
    return "%s layer%s" % (layer, " (%s)" % source if source else "")
