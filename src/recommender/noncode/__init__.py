"""The non-code surface delta (`e2.8` / `reco-bjd`): config, env, flags, migrations and CI —
with the operator actions they require.

One call answers the ticket:

    from recommender.noncode import Change, Episode, measure

    view = measure(Episode(id="ep-7c1", files=[Change(path=…, before=…, after=…)]))
    view.summary_line()
    # ep-7c1 · env 3 · migration 2 · ci 2 · 9 findings · 7 need an operator · 2 blocking
    view.findings                     # named, located, each with an action and an owner
    view.checklist.lines()            # AC 4: the per-episode operator-action checklist
    view.counts(); view.confidence; view.requires_operator_action

The concept document is
[docs/concepts/noncode-surfaces.md](../../../docs/concepts/noncode-surfaces.md), and every
acceptance criterion in the ticket is a test in `tests/test_noncode_contract.py`.

The ticket's premise is the design: **the changes that break production are rarely in source
files.** A renamed environment variable, a migration with no down path, a workflow that now
reads a secret nobody created, a base image that moved a major version, an IAM statement that
opened a port to the internet — every one of those is a two-line diff in a file most reviews
skim, and every one of them needs somebody *outside the review* to do something. So this
package produces two artefacts, not one: the findings (`what changed, where`) and the
checklist (`who has to do what, in which order`), and AC 4's roll-up is the second one.

What the ticket demanded, and where each part lives:

| demand | module |
| --- | --- |
| env vars added/removed, cross-checked against code references, stale references flagged | `env.py` (declarations + references + the rename rule) |
| feature flags added/removed **with default value** | `flags.py` (declarations, call-site reads, the default delta) |
| migrations: forward-only vs reversible, missing down, migration with model changes | `migrations.py` (nine tools' down markers, destructive statements, companion files) |
| CI workflow and permission changes: secrets, runners, OIDC, push-to-main | `ci.py` (both revisions, the delta only) |
| infrastructure-as-code deltas | `iac.py` (Terraform, CloudFormation, Kubernetes, Compose) |
| Dockerfile / base-image changes | `container.py` (stages, pins, privileges, `curl … | sh`) |
| the per-episode operator-action checklist | `checklist.py` (merge, order, audit) |
| a fixture per surface, and the gates | `corpus.py`, `eval.py` |

Modules, and the Rust home ADR-002 §9 gives them (team 5, `fathom-analyze`):

| module | Rust home | may do IO? |
| --- | --- | --- |
| `surfaces.py`    | `fathom-analyze::noncode::surfaces` | no — the change/episode shapes, line diff, surface attribution |
| `yamltext.py`    | `fathom-analyze::noncode::yamltext` | no — the tolerant line-oriented YAML reader |
| `env.py`         | `fathom-analyze::noncode::env` | no — declarations, references, the cross-check |
| `flags.py`       | `fathom-analyze::noncode::flags` | no — flag declarations, reads, defaults |
| `migrations.py`  | `fathom-analyze::noncode::migrations` | no — reversibility, destructive statements |
| `ci.py`          | `fathom-analyze::noncode::ci` | no — workflow deltas |
| `iac.py`         | `fathom-analyze::noncode::iac` | no — resource and exposure deltas |
| `container.py`   | `fathom-analyze::noncode::container` | no — image references, privileges, fetches |
| `policy.py`      | `fathom-analyze::noncode::policy` | no — finding table, knobs, layer resolution, `Finding` |
| `episode.py`     | `fathom-analyze::noncode` | no — the run and the roll-up |
| `checklist.py`   | `fathom-analyze::noncode::checklist` | no — actions, ordering, the checklist auditor |
| `render.py`      | `fathom-cli::verbs::noncode::render` | no — the card and its auditor |
| `explain.py`     | `fathom-cli::verbs::noncode::explain` | no — prose |
| `corpus.py`      | `fathom-analyze::noncode::corpus` (tests) | no — labelled fixtures |
| `eval.py`        | `fathom-cli::verbs::noncode::score` | no — the gates |
| `cli.py`         | `fathom-cli::verbs::noncode` | **yes** — reads blobs and `fathom.toml` |

`tests/test_noncode_contract.py` greps the analysis half for `open(`, `os.environ` and
`subprocess` the way the taxonomy, change, noise and test-surface contract suites grep their
own, and the Rust port must reproduce the blessed card and checklist text byte for byte.
"""

from __future__ import annotations

from .checklist import Action, Audit, Checklist, audit as audit_checklist
from .checklist import build as build_checklist
from .checklist import render_owner_summary
from .ci import Workflow, parse as parse_workflow
from .container import Base, ImageFile
from .episode import DELTA_VERSION, NonCodeDelta, aggregate, measure
from .env import Declaration, Reference, name_similarity
from .flags import Flag
from .iac import Manifest
from .migrations import Migration
from .policy import (FINDING_IDS, KNOBS, KNOB_INDEX, NON_CODE_PATTERNS, RULES, RULE_INDEX,
                     SEVERITY_HIGH, SEVERITY_NOTABLE, SEVERITY_ORDER, SURFACES, Finding,
                     Policy, UnanchoredFinding)
from .render import audit as audit_card
from .render import render as render_card
from .surfaces import SURVEY_VERSION, Change, Episode, line_diff

__all__ = ["Action", "Audit", "Base", "Change", "Checklist", "DELTA_VERSION", "Declaration",
           "FINDING_IDS", "Episode", "Flag", "ImageFile", "KNOBS", "KNOB_INDEX",
           "Manifest", "Migration", "NON_CODE_PATTERNS", "NonCodeDelta", "Policy",
           "RULES", "RULE_INDEX", "Reference", "SEVERITY_HIGH", "SEVERITY_NOTABLE",
           "SEVERITY_ORDER", "SURFACES", "SURVEY_VERSION", "UnanchoredFinding",
           "Workflow", "aggregate", "audit_card", "audit_checklist", "build_checklist",
           "line_diff", "measure", "name_similarity", "render_card", "render_owner_summary"]
