"""The labelled corpus: a fixture per surface, and what this ticket must say about each.

Every acceptance criterion is here with a hand-written expectation beside it:

| table | what it labels | AC |
| --- | --- | --- |
| `RULE_CASES` | one file pair (or a small episode) → the findings that must fire, with the name each one must carry | AC 1 — a fixture per surface (env, flag, migration, CI workflow, IaC, Dockerfile) produces the expected finding |
| `CROSS_CASES` | declarations plus references across files → the env cross-check verdict | AC 2 — declared-but-unread, and removed-but-referenced |
| `TRAP_CASES` | a change that must *not* fire a named finding | the same ACs, from the other side: precision |
| `EPISODE_CASES` | a whole episode → counts, the coordination warning, the checklist | AC 3, AC 4 |
| `DEMO_FILES` | one episode touching all six surfaces | the `demo` verb and the concept document's card |

Two rules keep this honest rather than agreeable.

**Every rule has a negative.** A corpus that only contains the cases a detector catches
proves the detector fires, never that it fires *because of* what it is looking at. The
pre-existing secret in a workflow, the `0.0.0.0/0` on an untouched line, the `-- drop table`
inside a comment, the `STRIPE_SECRET_KEY=changeme` in a README-driven `.env`, the migration
with its down file and the pinned base image that only gained a `COPY` are all here on
purpose: "it did not fire" is half of every acceptance criterion, and silence is only tested
by a case that expects it.

**Labels are facts about the change, not about the implementation.** Where a reader is
deliberately approximate, the label says so in `note`, so making the reader better forces the
label to change and makes the author notice what is now being claimed.

Fixtures stay small enough to read in one screen. A fixture nobody can re-derive when it
fails is a fixture that gets re-blessed into agreement with the regression.
"""

from __future__ import annotations

from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# environment variables
# ---------------------------------------------------------------------------
ENV_EXAMPLE_OLD = """\
# Service configuration. Copy to .env and fill in.
DATABASE_URL=postgres://localhost/app
REDIS_URL=redis://localhost:6379
SEARCH_INDEX_HOST=solr://search.internal:8983
"""

ENV_EXAMPLE_NEW = """\
# Service configuration. Copy to .env and fill in.
PG_DATABASE_URL=postgres://localhost/app
REDIS_URL=redis://localhost:6379
FEATURE_CHECKOUT_V2=false
PAYMENT_WEBHOOK_SECRET=
"""

SETTINGS_OLD = """\
import os

DATABASE_URL = os.environ["DATABASE_URL"]
SEARCH_INDEX_HOST = os.environ["SEARCH_INDEX_HOST"]


def database_url() -> str:
    return os.environ.get("DATABASE_URL", "sqlite:///:memory:")
"""

SETTINGS_NEW = """\
import os

PG_DATABASE_URL = os.environ["PG_DATABASE_URL"]
SEARCH_INDEX_HOST = os.environ["SEARCH_INDEX_HOST"]
FEATURE_CHECKOUT_V2 = os.getenv("FEATURE_CHECKOUT_V2", "false")


def database_url() -> str:
    return os.environ.get("PG_DATABASE_URL", "sqlite:///:memory:")
"""

DB_PY = """\
import os

ENGINE = os.environ["DATABASE_URL"]


def connect():
    return ENGINE
"""

COMMITTED_ENV = """\
DATABASE_URL=postgres://localhost/app
STRIPE_SECRET_KEY=sk_live_9f2b7c41ae83d0e5a1b6c2d4
OPENAI_API_KEY=changeme
"""

SH_ENTRYPOINT = """\
#!/usr/bin/env bash
set -euo pipefail

exec gunicorn "app:create_app(${APP_CONFIG:-prod})" --bind 0.0.0.0:8000
"""

# ---------------------------------------------------------------------------
# feature flags
# ---------------------------------------------------------------------------
FLAGS_OLD = """\
flags:
  - name: legacy_checkout
    default: false
    description: the pre-2026 checkout path
  - name: new_pricing
    default: false
"""

FLAGS_NEW = """\
flags:
  - name: checkout_v2
    default: false
    description: the rewrite, dark-launched
  - name: instant_search
    default: true
    description: on for everybody at deploy
  - name: new_pricing
    default: true
"""

CHECKOUT_PY_OLD = """\
from . import flags


def render(cart):
    if flags.is_enabled("legacy_checkout"):
        return render_old(cart)
    return render_new(cart)
"""

CHECKOUT_PY_NEW = """\
from . import flags


def render(cart):
    if flags.is_enabled("checkout_v2"):
        return render_v2(cart)
    if flags.is_enabled("beta_search", default=False):
        return render_beta(cart)
    return render_new(cart)
"""

# ---------------------------------------------------------------------------
# migrations
# ---------------------------------------------------------------------------
RAILS_FORWARD_ONLY = """\
class AddOrdersToInvoices < ActiveRecord::Migration[7.1]
  def up
    add_column :invoices, :order_id, :bigint
    execute "UPDATE invoices SET order_id = 0 WHERE order_id IS NULL"
    change_column_null :invoices, :order_id, false
    remove_column :invoices, :legacy_ref
  end
end
"""

RAILS_REVERSIBLE = """\
class AddOrdersToInvoices < ActiveRecord::Migration[7.1]
  def change
    add_column :invoices, :order_id, :bigint
    add_index :invoices, :order_id
  end
end
"""

ALEMBIC_REVERSIBLE = """\
""" + '"""add orders to invoices"""\n' + """\
revision = "b7c1f0a9"
down_revision = "51e3f2c1"


def upgrade():
    op.add_column("invoices", sa.Column("order_id", sa.BigInteger()))


def downgrade():
    op.drop_column("invoices", "order_id")
"""

ALEMBIC_NO_DOWN = """\
""" + '"""add orders to invoices"""\n' + """\
revision = "c8d2e1b0"
down_revision = "51e3f2c1"


def upgrade():
    op.execute("ALTER TABLE invoices ALTER COLUMN total SET NOT NULL")


def downgrade():
    pass
"""

GOOSE_PAIR_UP = """\
-- +goose Up
ALTER TABLE invoices ADD COLUMN order_id BIGINT;

-- +goose Down
ALTER TABLE invoices DROP COLUMN order_id;
"""

GOOSE_WITHOUT_DOWN = """\
-- +goose Up
ALTER TABLE invoices DROP COLUMN legacy_ref;
"""

ORDER_MODEL = """\
class Order < ApplicationRecord
  belongs_to :invoice
  validates :order_id, presence: true
end
"""

# ---------------------------------------------------------------------------
# CI
# ---------------------------------------------------------------------------
WORKFLOW_OLD = """\
name: release
on:
  release:
    types: [published]
permissions:
  contents: read
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo publish
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
"""

WORKFLOW_NEW = """\
name: release
on:
  release:
    types: [published]
  push:
    branches: [main]
permissions:
  contents: read
jobs:
  publish:
    runs-on: [self-hosted, linux]
    environment: production
    permissions:
      id-token: write
      contents: write
    steps:
      - uses: actions/checkout@v4
      - uses: aws-actions/configure-aws-credentials@v4
        with:
          role-to-assume: arn:aws:iam::123456789012:role/fathom-release
      - run: cargo publish
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
          SIGNING_KEY: ${{ secrets.SIGNING_KEY }}
"""

WORKFLOW_ONLY_A_COMMENT = WORKFLOW_NEW.replace("name: release", "name: release   # touched to\n")

# ---------------------------------------------------------------------------
# infrastructure as code
# ---------------------------------------------------------------------------
TF_OLD = """\
resource "aws_dynamodb_table" "sessions" {
  name = "fathom-sessions"
  hash_key = "id"
}

resource "aws_security_group" "edge" {
  ingress {
    from_port   = 443
    to_port     = 443
    cidr_blocks = ["10.0.0.0/8"]
  }
}
"""

TF_NEW = """\
resource "aws_s3_bucket" "audit_logs" {
  bucket = "fathom-audit-logs"
}

resource "aws_security_group" "edge" {
  ingress {
    from_port   = 443
    to_port     = 443
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_iam_role_policy" "lambda" {
  policy = jsonencode({
    Statement = [{ Effect = "Allow", Action = ["*"], Resource = "*" }]
  })
}
"""

K8S_OLD = """\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api
spec:
  template:
    spec:
      containers:
        - name: api
          image: registry.example.com/api:1.24.3
          env:
            - name: LOG_LEVEL
              value: info
"""

K8S_NEW = """\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api
  labels:
    team: payments
spec:
  template:
    spec:
      containers:
        - name: api
          image: registry.example.com/api:1.25.0
          securityContext:
            privileged: true
          env:
            - name: LOG_LEVEL
              value: info
            - name: FEATURE_CHECKOUT_V2
              value: "true"
"""

# ---------------------------------------------------------------------------
# containers
# ---------------------------------------------------------------------------
DOCKERFILE_OLD = """\
FROM python:3.11-slim@sha256:1f2a8c90b7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1f0
WORKDIR /app
COPY requirements.txt /app/
RUN pip install -r requirements.txt==2.1.0
COPY . /app
USER appuser
CMD ["gunicorn", "app:app"]
"""

DOCKERFILE_NEW = """\
FROM python:3.13-slim
ARG STAGE=prod
FROM python:3.13-slim@sha256:1f2a8c90b7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1f0
WORKDIR /app
RUN curl -fsSL https://install.example-tools.sh | sh
COPY requirements.txt /app/
RUN pip install -r requirements.txt==2.1.0
COPY . /app
USER root
CMD ["gunicorn", "app:app"]
"""

COMPOSE_OLD = """\
version: "3.9"
services:
  api:
    image: registry.example.com/api:1.24.3
    environment:
      - DATABASE_URL=postgres://db/app
  worker:
    image: registry.example.com/worker:1.24.3
"""

COMPOSE_NEW = """\
version: "3.9"
services:
  api:
    image: registry.example.com/api:1.25.0
    environment:
      - DATABASE_URL=postgres://db/app
      - FEATURE_CHECKOUT_V2=true
  worker:
    image: registry.example.com/worker:1.24.3
  search:
    image: docker.elastic.co/elasticsearch/elasticsearch
    privileged: true
"""

K8S_WORKER = """\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: worker
spec:
  template:
    spec:
      containers:
        - name: worker
          image: registry.example.com/worker:1.25.0
          env:
            - name: QUEUE_NAME
              value: invoices
"""

README_ONLY = """\
# Install

    pip install fathom
"""

README_ONLY_NEW = """\
# Install

    pip install fathom==0.8

See the docs for the config file.
"""


# ---------------------------------------------------------------------------
# the labelled cases
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class RuleCase:
    """One fixture that must produce one specific finding, with the name it must carry."""

    name: str
    rule: str
    files: dict                      # path -> (before, after)
    expect_name: str = ""            # substring the finding's name must contain
    expect_says: tuple[str, ...] = ()
    note: str = ""
    knobs: dict = field(default_factory=dict)
    repo_env_refs: tuple[str, ...] = ()
    repo_flag_reads: tuple[str, ...] = ()


@dataclass(frozen=True)
class TrapCase:
    """A change that must leave a named finding silent."""

    name: str
    must_not_fire: tuple[str, ...]
    files: dict
    note: str = ""
    knobs: dict = field(default_factory=dict)
    repo_env_refs: tuple[str, ...] = ()


@dataclass(frozen=True)
class EpisodeCase:
    """A whole episode with hand-written expectations about its roll-up."""

    name: str
    files: dict
    counts: dict = field(default_factory=dict)
    must_say: tuple[str, ...] = ()
    must_not_say: tuple[str, ...] = ()
    requires_operator_action: bool | None = None
    coordination: bool | None = None
    note: str = ""
    knobs: dict = field(default_factory=dict)


# -- env (AC 1 + AC 2) ------------------------------------------------------
RULE_CASES: tuple[RuleCase, ...] = (
    RuleCase(name="env-added-read", rule="env-added",
             files={".env.example": (ENV_EXAMPLE_OLD, ENV_EXAMPLE_NEW),
                    "src/settings.py": (SETTINGS_OLD, SETTINGS_NEW)},
             expect_name="FEATURE_CHECKOUT_V2",
             note="a new declared-and-read variable is one finding, named, with both sites"),
    RuleCase(name="env-declared-unread", rule="env-declared-unread",
             files={".env.example": (ENV_EXAMPLE_OLD, ENV_EXAMPLE_NEW),
                    "src/settings.py": (SETTINGS_OLD, SETTINGS_NEW)},
             expect_name="PAYMENT_WEBHOOK_SECRET",
             expect_says=("episode",),
             note="declared in .env.example, read nowhere in the episode; the claim is scoped "
                  "to the episode and says so"),
    RuleCase(name="env-stale-reference", rule="env-stale-reference",
             files={".env.example": (ENV_EXAMPLE_OLD,
                                     ENV_EXAMPLE_OLD.replace("SEARCH_INDEX_HOST="
                                                             "solr://search.internal:8983\n",
                                                             "")),
                    "src/search.py": ('import os\n\nHOST = os.environ["SEARCH_INDEX_HOST"]\n',
                                      'import os\n\nHOST = os.environ["SEARCH_INDEX_HOST"]'
                                      "  # still reads it\n")},
             expect_name="SEARCH_INDEX_HOST",
             note="the declaration went; a file nobody touched still reads it. AC 2's vice versa"),
    RuleCase(name="env-renamed", rule="env-renamed",
             files={".env.example": (ENV_EXAMPLE_OLD, ENV_EXAMPLE_NEW),
                    "src/settings.py": (SETTINGS_OLD, SETTINGS_NEW),
                    "src/db.py": (DB_PY, DB_PY)},
             expect_name="DATABASE_URL",
             expect_says=("still",),
             note="half-done rename: settings.py moved to PG_DATABASE_URL, db.py still reads "
                  "DATABASE_URL — the production incident the ticket names"),
    RuleCase(name="env-removed", rule="env-removed",
             files={".env.example": (ENV_EXAMPLE_OLD,
                                     ENV_EXAMPLE_OLD.replace(
                                         "REDIS_URL=redis://localhost:6379\n", ""))},
             expect_name="REDIS_URL", note="retiring a variable is still an operator action"),
    RuleCase(name="env-value-committed", rule="env-value-committed",
             files={".env": (None, COMMITTED_ENV)},
             expect_name="STRIPE_SECRET_KEY",
             note="a live-looking secret in a committed .env; the placeholder key must not "
                  "raise anything of its own"),
    RuleCase(name="env-dockerfile-declares", rule="env-added",
             files={"Dockerfile": (DOCKERFILE_OLD, DOCKERFILE_NEW)},
             expect_name="STAGE", note="a Dockerfile ARG/ENV is a variable somebody must set"),
    RuleCase(name="env-compose-declares", rule="env-added",
             files={"docker-compose.yml": (COMPOSE_OLD, COMPOSE_NEW)},
             expect_name="FEATURE_CHECKOUT_V2",
             note="compose `environment:` items are declarations too"),
    RuleCase(name="env-kubernetes-declares", rule="env-added",
             files={"k8s/api.yaml": (K8S_OLD, K8S_NEW)},
             expect_name="FEATURE_CHECKOUT_V2",
             note="a Deployment's env block is where an operator sets it"),
    # -- flags --------------------------------------------------------------
    RuleCase(name="flag-added-off", rule="flag-added",
             files={"flags.yaml": (FLAGS_OLD, FLAGS_NEW),
                    "checkout.py": (CHECKOUT_PY_OLD, CHECKOUT_PY_NEW)},
             expect_name="checkout_v2", expect_says=("default off",)),
    RuleCase(name="flag-added-on", rule="flag-added",
             files={"flags.yaml": (FLAGS_OLD, FLAGS_NEW)},
             expect_name="instant_search", expect_says=("ON by default",),
             note="a flag that ships switched on is the rollout, not the dark launch"),
    RuleCase(name="flag-default-changed", rule="flag-default-changed",
             files={"flags.yaml": (FLAGS_OLD, FLAGS_NEW)},
             expect_name="new_pricing", expect_says=("off", "on")),
    RuleCase(name="flag-removed", rule="flag-removed",
             files={"flags.yaml": (FLAGS_OLD, FLAGS_NEW),
                    "checkout.py": (CHECKOUT_PY_OLD, CHECKOUT_PY_NEW)},
             expect_name="legacy_checkout"),
    RuleCase(name="flag-read-no-default", rule="flag-read-no-default",
             files={"app/payment.py": (None, 'def pay(cart):\n'
                                             '    if flags.is_enabled("instant_refunds"):\n'
                                             '        return refund(cart)\n')},
             expect_name="instant_refunds",
             note="no fallback at the call site and no declaration: the provider is now on the "
                  "request path"),
    # -- migrations ---------------------------------------------------------
    RuleCase(name="migration-added-rails", rule="migration-added",
             files={"db/migrate/20261017120000_add_orders_to_invoices.rb": (None,
                                                                            RAILS_FORWARD_ONLY)},
             expect_name="20261017120000", expect_says=("forward-only",)),
    RuleCase(name="migration-forward-only-rails", rule="migration-forward-only",
             files={"db/migrate/20261017120000_add_orders_to_invoices.rb": (None,
                                                                            RAILS_FORWARD_ONLY)},
             expect_name="20261017120000", expect_says=("no `def down`",)),
    RuleCase(name="migration-destructive", rule="migration-destructive",
             files={"db/migrate/20261017120000_add_orders_to_invoices.rb": (None,
                                                                            RAILS_FORWARD_ONLY)},
             expect_name="REMOVE_COLUMN"),
    RuleCase(name="migration-forward-only-alembic", rule="migration-forward-only",
             files={"migrations/versions/c8d2e1b0_add_order.py": (None, ALEMBIC_NO_DOWN)},
             expect_name="c8d2e1b0", expect_says=("pass",),
             note="a `downgrade()` that only says `pass` is a comment, not a rollback"),
    RuleCase(name="migration-goose-no-down", rule="migration-forward-only",
             files={"migrations/0002_drop_legacy_ref.up.sql": (None, GOOSE_WITHOUT_DOWN)},
             expect_name="0002_drop_legacy_ref", expect_says=("no down marker",)),
    RuleCase(name="migration-with-model", rule="migration-with-model",
             files={"db/migrate/20261017120000_add_orders_to_invoices.rb": (None,
                                                                            RAILS_REVERSIBLE),
                    "app/models/order.rb": (None, ORDER_MODEL)},
             expect_name="order.rb"),
    RuleCase(name="deploy-coordination", rule="deploy-coordination",
             files={"db/migrate/20261017120000_add_orders_to_invoices.rb": (None,
                                                                            RAILS_FORWARD_ONLY)},
             expect_name="migration-only", expect_says=("needs deploy coordination",)),
    # -- ci -----------------------------------------------------------------
    RuleCase(name="ci-secret-new", rule="ci-secret-new",
             files={".github/workflows/release.yml": (WORKFLOW_OLD, WORKFLOW_NEW)},
             expect_name="SIGNING_KEY", expect_says=("environment",)),
    RuleCase(name="ci-permission-escalation", rule="ci-permission-escalation",
             files={".github/workflows/release.yml": (WORKFLOW_OLD, WORKFLOW_NEW)},
             expect_name="id-token", expect_says=("publish",)),
    RuleCase(name="ci-permission-escalation-top-level", rule="ci-permission-escalation",
             files={".github/workflows/release.yml": (WORKFLOW_OLD,
                                                      WORKFLOW_NEW.replace(
                                                          "permissions:\n  contents: read",
                                                          "permissions: write-all"))},
             expect_name="write-all", expect_says=("top level",),
             note="`write-all` is the escalation that does not name what it escalates"),
    RuleCase(name="ci-oidc-changed", rule="ci-oidc-changed",
             files={".github/workflows/release.yml": (WORKFLOW_OLD, WORKFLOW_NEW)},
             expect_name="aws-actions/configure-aws-credentials",
             expect_says=("role-to-assume", "environment: production"),
             note="one identity row per workflow, with every new marker in its evidence: the "
                  "role ARN and the environment binding are one review, not four rows"),
    RuleCase(name="ci-runner-changed", rule="ci-runner-changed",
             files={".github/workflows/release.yml": (WORKFLOW_OLD, WORKFLOW_NEW)},
             expect_name="self-hosted"),
    RuleCase(name="ci-trigger-main-push", rule="ci-trigger-main-push",
             files={".github/workflows/release.yml": (WORKFLOW_OLD, WORKFLOW_NEW)},
             expect_name="push"),
    # -- iac ----------------------------------------------------------------
    RuleCase(name="iac-resource-added", rule="iac-resource-added",
             files={"infra/main.tf": (TF_OLD, TF_NEW)},
             expect_name="aws_s3_bucket.audit_logs"),
    RuleCase(name="iac-resource-removed", rule="iac-resource-removed",
             files={"infra/main.tf": (TF_OLD, TF_NEW)},
             expect_name="aws_dynamodb_table.sessions", expect_says=("destroy",)),
    RuleCase(name="iac-policy-widened", rule="iac-policy-widened",
             files={"infra/main.tf": (TF_OLD, TF_NEW)},
             expect_name="ingress reachable"),
    RuleCase(name="iac-kubernetes-resource", rule="iac-resource-added",
             files={"k8s/worker.yaml": (None, K8S_WORKER)},
             expect_name="Deployment/worker",
             note="a manifest added whole: its resources are all new, and the reader names "
                  "them by kind and metadata name, not by file"),
    # -- container ----------------------------------------------------------
    RuleCase(name="container-base-changed", rule="container-base-changed",
             files={"Dockerfile": (DOCKERFILE_OLD, DOCKERFILE_NEW)},
             expect_name="python", expect_says=("the major version moved",),
             note="a 3.11 → 3.13 move: same vendor, different Python under the app"),
    RuleCase(name="container-base-unpinned", rule="container-base-unpinned",
             files={"Dockerfile": (DOCKERFILE_OLD, DOCKERFILE_NEW)},
             expect_name="python:3.13-slim"),
    RuleCase(name="container-supply-chain", rule="container-supply-chain",
             files={"Dockerfile": (DOCKERFILE_OLD, DOCKERFILE_NEW)},
             expect_name="curl"),
    RuleCase(name="container-privilege-changed", rule="container-privilege-changed",
             files={"Dockerfile": (DOCKERFILE_OLD, DOCKERFILE_NEW)},
             expect_name="USER root"),
    RuleCase(name="container-compose-privilege", rule="container-privilege-changed",
             files={"docker-compose.yml": (COMPOSE_OLD, COMPOSE_NEW)},
             expect_name="privileged"),
    RuleCase(name="container-k8s-image", rule="container-base-changed",
             files={"k8s/api.yaml": (K8S_OLD, K8S_NEW)}, expect_name="api:1.25.0"),
)

# -- every surface has at least one fixture (AC 1) ---------------------------
SURFACE_OF_RULE: dict[str, str] = {
    "env-added": "env", "env-declared-unread": "env", "env-stale-reference": "env",
    "env-renamed": "env", "env-removed": "env", "env-value-committed": "env",
    "flag-added": "flag", "flag-default-changed": "flag", "flag-removed": "flag",
    "flag-read-no-default": "flag",
    "migration-added": "migration", "migration-forward-only": "migration",
    "migration-destructive": "migration", "migration-with-model": "migration",
    "deploy-coordination": "migration",
    "ci-secret-new": "ci", "ci-permission-escalation": "ci", "ci-oidc-changed": "ci",
    "ci-runner-changed": "ci", "ci-trigger-main-push": "ci",
    "iac-resource-added": "iac", "iac-resource-removed": "iac", "iac-policy-widened": "iac",
    "container-base-changed": "container", "container-base-unpinned": "container",
    "container-supply-chain": "container", "container-privilege-changed": "container",
}

# ---------------------------------------------------------------------------
# the traps: findings that must stay silent
# ---------------------------------------------------------------------------
TRAP_CASES: tuple[TrapCase, ...] = (
    TrapCase(name="ci-secret-was-al-there",
             must_not_fire=("ci-secret-new", "ci-permission-escalation",
                            "ci-trigger-main-push", "ci-runner-changed"),
             files={".github/workflows/release.yml":
                    (WORKFLOW_NEW, WORKFLOW_NEW + "\n# bumped a step name\n")},
             note="the secret, the permission and the trigger all predate this episode; only a "
                  "comment moved"),
    TrapCase(name="iac-widening-was-already-there",
             must_not_fire=("iac-policy-widened",),
             files={"infra/main.tf": (TF_NEW, TF_NEW + "\n# reviewed 2026-10-01\n")},
             note="`0.0.0.0/0` on an untouched line is not this episode's news"),
    TrapCase(name="migration-destructive-inside-a-comment",
             must_not_fire=("migration-destructive", "migration-forward-only"),
             files={"migrations/0003_add_index.up.sql": (None,
                                                         "-- +goose Up\nCREATE INDEX ON invoices"
                                                         " (order_id);\n"
                                                         "-- +goose Down\nDROP INDEX invoices"
                                                         "_order_idx;\n"
                                                         "-- we could drop table invoices "
                                                         "later, do not forget\n")},
             note="a commented-out DROP is not a DROP, and the Down section is real"),
    TrapCase(name="dockerfile-untouched-base",
             must_not_fire=("container-base-changed", "container-base-unpinned"),
             files={"Dockerfile": (DOCKERFILE_OLD, DOCKERFILE_OLD.replace(
                 "COPY . /app", "COPY . /app\nCOPY VERSION /app/VERSION"))},
             note="a pinned base this episode did not touch must not be reported twice"),
    TrapCase(name="env-placeholder-is-not-a-leak",
             must_not_fire=("env-value-committed",),
             files={".env": (None, "DATABASE_URL=postgres://localhost/app\n"
                                   "STRIPE_SECRET_KEY=changeme\n")},
             note="`changeme` is documentation, and every README in the world writes one"),
    TrapCase(name="env-declared-and-read-in-episode",
             must_not_fire=("env-declared-unread",),
             files={".env.example": ("A=\n", "A=\nFEATURE_X=false\n"),
                    "src/feature.py": (None, 'import os\n\nX = os.getenv("FEATURE_X", "false")\n')},
             note="the reader is in the same episode; the cross-check is satisfied by the diff"),
    TrapCase(name="env-repo-scope-answers-it",
             must_not_fire=("env-declared-unread",),
             files={".env.example": ("A=\n", "A=\nFEATURE_X=false\n")},
             knobs={"reference_scope": "repo"}, repo_env_refs=("FEATURE_X",),
             note="`reference_scope = repo` plus a supplied reference set is the confident "
                  "version of the claim, and it silences the row"),
    TrapCase(name="flag-default-unchanged",
             must_not_fire=("flag-default-changed", "flag-added"),
             files={"flags.yaml": (FLAGS_OLD, FLAGS_OLD + "\n")},
             note="a whitespace-only change to a flag file is not a rollout"),
    TrapCase(name="alembic-reversible-is-not-forward-only",
             must_not_fire=("migration-forward-only",),
             files={"migrations/versions/b7c1f0a9_add_order.py": (None, ALEMBIC_REVERSIBLE)},
             note="a real `def downgrade()` is a rollback"),
    TrapCase(name="rails-change-is-reversible",
             must_not_fire=("migration-forward-only",),
             files={"db/migrate/20261017120000_add_orders_to_invoices.rb": (None,
                                                                            RAILS_REVERSIBLE)},
             note="`def change` with only reversible steps is reversible"),
    TrapCase(name="docs-only-episode-is-quiet",
             must_not_fire=("env-added", "flag-added", "migration-added", "ci-secret-new",
                            "iac-resource-added", "container-base-changed"),
             files={"README.md": (README_ONLY, README_ONLY_NEW)},
             note="the clean baseline: a corpus of only positives proves the detectors fire, "
                  "not that they fire because of what they are looking at"),
    TrapCase(name="source-only-episode-is-quiet",
             must_not_fire=("env-added", "migration-added", "container-base-changed"),
             files={"src/calc.py": ("def add(a, b):\n    return a + b\n",
                                    "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n"
                                    "    return a - b\n")}),
)

# ---------------------------------------------------------------------------
# whole episodes (AC 3 and AC 4)
# ---------------------------------------------------------------------------
EPISODE_CASES: tuple[EpisodeCase, ...] = (
    EpisodeCase(name="migration-only",
                files={"db/migrate/20261017120000_add_orders_to_invoices.rb":
                       (None, RAILS_FORWARD_ONLY)},
                counts={"blocking": 5, "findings": 6}, coordination=True,
                requires_operator_action=True,
                must_say=("needs deploy coordination", "migration"),
                note="AC 3: a migration-only episode must say the words"),
    EpisodeCase(name="migration-with-code",
                files={"db/migrate/20261017120000_add_orders_to_invoices.rb":
                       (None, RAILS_REVERSIBLE),
                       "app/models/order.rb": (None, ORDER_MODEL)},
                coordination=False, requires_operator_action=True,
                must_say=("sequence the deploy",),
                note="the ordering row appears, and the migration-only warning does not"),
    EpisodeCase(name="six-surfaces",
                files={".env.example": (ENV_EXAMPLE_OLD, ENV_EXAMPLE_NEW),
                       "src/settings.py": (SETTINGS_OLD, SETTINGS_NEW),
                       "src/db.py": (DB_PY, DB_PY),
                       "flags.yaml": (FLAGS_OLD, FLAGS_NEW),
                       "checkout.py": (CHECKOUT_PY_OLD, CHECKOUT_PY_NEW),
                       "db/migrate/20261017120000_add_orders_to_invoices.rb":
                       (None, RAILS_FORWARD_ONLY),
                       ".github/workflows/release.yml": (WORKFLOW_OLD, WORKFLOW_NEW),
                       "infra/main.tf": (TF_OLD, TF_NEW),
                       "Dockerfile": (DOCKERFILE_OLD, DOCKERFILE_NEW)},
                requires_operator_action=True, coordination=False,
                must_say=("requires operator action: yes",)),
    EpisodeCase(name="docs-only",
                files={"README.md": (README_ONLY, README_ONLY_NEW)},
                counts={"findings": 0}, requires_operator_action=False, coordination=False,
                must_say=("requires operator action: no", "nothing to do"),
                note="an episode with nothing on it says so, and states its confidence anyway"),
    EpisodeCase(name="compose-and-k8s",
                files={"docker-compose.yml": (COMPOSE_OLD, COMPOSE_NEW),
                       "k8s/api.yaml": (K8S_OLD, K8S_NEW)},
                requires_operator_action=True,
                must_say=("compose", "kubernetes")),
)

# ---------------------------------------------------------------------------
# the demo card (docs and the `demo` verb)
# ---------------------------------------------------------------------------
def demo_files() -> dict:
    """One episode, six surfaces, one finding of every severity."""
    case = next(item for item in EPISODE_CASES if item.name == "six-surfaces")
    return dict(case.files)


def demo_episode(*, policy=None, episode_id: str = "ep-demo",
                 title: str = "add orders, wire the release pipeline"):
    from .episode import Episode, measure
    from .surfaces import Change
    files = demo_files()
    return measure(Episode(
        id=episode_id, title=title,
        files=tuple(Change.from_pair(path, before, after)
                    for path, (before, after) in sorted(files.items()))), policy)
