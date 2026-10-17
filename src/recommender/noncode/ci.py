"""CI workflow and permission changes: the diff that runs with somebody else's credentials.

A CI file is a program that executes with tokens, on a machine, triggered by an event. Four
things about it are operator knowledge rather than code review, and the ticket names all
four — new secrets, new runners, changed OIDC, push-to-main triggers — with permission
changes as the umbrella they all sit under.

**Secrets.** `${{ secrets.FOO }}` is a request, not a definition: nothing in the repository
makes `FOO` exist. The finding fires on a name that appears for the first time in the
episode, and it says which scope to create it in, because repository, organisation and
*environment* secrets are three different screens and the job's `environment:` key decides
which one. Other CI tools get the same treatment with their own spelling — a CircleCI
`context:`, a GitLab `secrets:`/`variables:` entry — each named as the dialect it was read
in.

**Permissions.** `permissions:` is a token-capability list. What matters is the *change*: a
job that used to read now writes, or `contents: write` appeared, or someone reached for
`write-all`. Only the delta is reported, with the scope that moved, because a workflow that
has had `id-token: write` for two years is not this episode's news.

**Identity.** `id-token: write`, a role ARN, an `environment:` binding, a vault login: the
workflow can now become an identity a cloud provider trusts. This is an access review and it
is filed as one — with the marker that triggered it quoted, since "changed OIDC" is a
category and the line is the evidence.

**Triggers.** A `push` trigger whose branch list includes the default branch (or filters
nothing at all) means *on merge*. Added or widened in this episode, it is reported with the
workflow's other triggers in view, because `release: published` was already a side effect
and `push: [main]` on top of it is the one that starts firing today.

Every one of these is computed from *both* revisions and reported only on the difference,
which is the whole reason this module reads old and new rather than scanning a file for
worst-prone strings: a scanner that finds `secrets.AWS_ACCESS_KEY_ID` in a workflow that has
had it since 2021 produces a finding nobody can act on and one more reason to distrust the
next one.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from . import yamltext as Y
from .policy import Finding
from .surfaces import Run

SECRET_REF = re.compile(r"\$\{\{\s*secrets\.(?P<name>[A-Za-z0-9_]+)\s*\}\}")
CONTEXT_REF = re.compile(r"^\s*context\s*:\s*(?P<value>\S.*)$")
ROLE_ARN = re.compile(r"arn:[a-z0-9\-]+:iam::\d{12}:role/[^\s\"']+")
ENV_KEY_NAMES = ("environment",)

#: Triggers that mean "this runs without a human asking".
AUTOMATIC_TRIGGERS = ("push", "pull_request", "pull_request_target", "workflow_run",
                      "schedule", "release", "create")


@dataclass
class Workflow:
    """One CI file, in the shapes this module can read."""

    path: str
    dialect: str = "github-actions"
    secrets: dict[str, int] = field(default_factory=dict)        # name → first line
    contexts: dict[str, int] = field(default_factory=dict)       # CircleCI contexts
    permissions: dict[str, str] = field(default_factory=dict)    # `contents` → `write`
    job_permissions: dict[str, dict[str, str]] = field(default_factory=dict)
    runners: dict[str, list[str]] = field(default_factory=dict)  # job → labels
    environments: dict[str, str] = field(default_factory=dict)   # job → environment name
    identity: list[tuple[int, str]] = field(default_factory=list)  # (line, marker)
    triggers: dict[str, list[str]] = field(default_factory=dict)  # trigger → branches
    jobs: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    partial: bool = False

    def permission_pairs(self) -> dict[str, str]:
        """Every `scope: access` claim in the file, top-level and per job, keyed for diffing."""
        out = {"top." + key: value for key, value in self.permissions.items()}
        for job, scopes in self.job_permissions.items():
            for key, value in scopes.items():
                out["%s.%s" % (job, key)] = value
        return out

    def pushes_default(self, branches: tuple[str, ...]) -> tuple[bool, str]:
        """Does this file trigger on a push that includes a default branch?"""
        push = self.triggers.get("push")
        if push is None:
            return False, ""
        if not push:
            return True, "`on.push` with no branch filter, so every branch including %s" \
                         % "/".join(sorted(branches))
        hits = [name for name in push if name in branches]
        if hits:
            return True, "`on.push.branches` includes %s" % ", ".join(sorted(hits))
        if any(name.endswith("*") for name in push):
            return True, "`on.push.branches` has a wildcard (%s)" % ", ".join(push)
        return False, ""

    def deduped_identity(self) -> list[tuple[int, str]]:
        """One marker per line: a `uses:` line and its `role-to-assume` are one identity."""
        out: list[tuple[int, str]] = []
        seen_lines: set[int] = set()
        for line, marker in self.identity:
            if line in seen_lines:
                continue
            seen_lines.add(line)
            out.append((line, marker))
        return out

    def as_dict(self) -> dict:
        return {"path": self.path, "dialect": self.dialect, "secrets": dict(self.secrets),
                "contexts": dict(self.contexts), "permissions": dict(self.permissions),
                "job_permissions": {key: dict(value) for key, value
                                    in self.job_permissions.items()},
                "runners": {key: list(value) for key, value in self.runners.items()},
                "environments": dict(self.environments),
                "identity": [[line, marker] for line, marker in self.identity],
                "triggers": {key: list(value) for key, value in self.triggers.items()},
                "jobs": list(self.jobs), "partial": self.partial,
                "notes": list(self.notes)}


def parse(path: str, text: str | None) -> Workflow | None:
    """Read one CI file. `None` when there is nothing of this shape to read."""
    if text is None:
        return None
    workflow = Workflow(path=path, dialect=_dialect(path))
    for lineno, raw in enumerate(text.splitlines(), 1):
        for match in SECRET_REF.finditer(raw):
            workflow.secrets.setdefault(match.group("name"), lineno)
        if "configure-aws-credentials" in raw or "azure/login" in raw or \
                "vault-action" in raw or "role-to-assume" in raw:
            workflow.identity.append((lineno, raw.strip()[:90]))
        if ROLE_ARN.search(raw):
            workflow.identity.append((lineno, ROLE_ARN.search(raw).group(0)))
        if workflow.dialect == "circleci":
            match = CONTEXT_REF.match(raw)
            if match:
                workflow.contexts.setdefault(_scalar(match.group("value")), lineno)
    scan = Y.scan(text)
    workflow.partial = bool(scan.notes)
    workflow.notes = list(scan.notes)
    if workflow.dialect != "github-actions":
        _other_ci(workflow, scan)
        return workflow
    for node in scan.nodes:
        if node.key == "permissions":
            if node.value in ("write-all", "read-all", "none"):
                workflow.permissions[node.value] = "write" if node.value == "write-all" \
                    else node.value
            continue
        if node.path and node.path[-1] == "permissions" and node.key and node.value:
            job = _job_of(node)
            target = workflow.permissions if job == "top" \
                else workflow.job_permissions.setdefault(job, {})
            target[node.key] = node.value
        if node.key == "runs-on":
            job = _job_of(node)
            if job not in workflow.runners:
                workflow.runners[job] = node.list_value()
        if node.key == "environment" and node.value and node.path[:1] == ("jobs",):
            workflow.environments[_job_of(node)] = node.value
        if node.key == "id-token" and node.value == "write":
            workflow.identity.append((node.lineno, "id-token: write (scoped, not external)"))
        if node.path == ("jobs",) and node.key:
            workflow.jobs.append(node.key)
        if node.path == ("on",) and node.key:
            workflow.triggers.setdefault(node.key, [])
        if node.key == "branches" and node.path[:2] == ("on", "push"):
            workflow.triggers["push"] = node.list_value()
        if node.item and node.path[:2] == ("on", "push"):
            workflow.triggers.setdefault("push", []).append(node.item)
    return workflow


def detect(run: Run) -> list[Finding]:
    """Every CI finding for one episode."""
    findings: list[Finding] = []
    escalate = {item.strip() for item in run.policy.words("escalating_permissions")}
    defaults = tuple(run.policy.words("default_branches"))
    for facts in run.files:
        if not facts.on("ci"):
            continue
        old = parse(facts.path, facts.before)
        new = parse(facts.path, facts.after)
        if new is None and old is None:
            continue
        new = new or Workflow(path=facts.path)
        old = old or Workflow(path=facts.path)
        for note in new.notes:
            run.note(facts, note)
        anchor = facts.path
        added_lines = {line.text for line in facts.diff.added}

        if run.enabled("ci-secret-new"):
            for name, line in sorted(new.secrets.items()):
                if name in old.secrets:
                    continue
                scope = _secret_scope(new, name)
                findings.append(Finding(
                    kind="ci-secret-new", name=name, path=anchor, line=line,
                    detail="the workflow reads `${{ secrets.%s }}` for the first time" % name,
                    action="create %s in %s before this workflow runs" % (name, scope),
                    evidence=["first read at %s:%d" % (anchor, line), scope_evidence(new)]
                             + _partial_note(new),
                    dialect=new.dialect, confidence=0.95))
            for name, line in sorted(new.contexts.items()):
                if name in old.contexts:
                    continue
                findings.append(Finding(
                    kind="ci-secret-new", name=name, path=anchor, line=line,
                    detail="the pipeline pulls a new secrets context",
                    action="make sure context `%s` exists and grants only this project" % name,
                    evidence=["referenced at %s:%d" % (anchor, line)],
                    dialect=new.dialect, confidence=0.85))

        if run.enabled("ci-permission-escalation"):
            before = old.permission_pairs()
            after = new.permission_pairs()
            for key, access in sorted(after.items()):
                scope = key.rsplit(".", 1)[-1]
                moved = before.get(key)
                label = "%s: %s" % (scope, access)
                check = "%s:%s" % (scope, access)
                escalates = access == "write" and (check in escalate or scope == "id-token")
                if scope == "write-all":
                    escalates = True
                if not escalates or moved == access:
                    continue
                job = "top level" if key.startswith("top.") else key.split(".", 1)[0]
                findings.append(Finding(
                    kind="ci-permission-escalation", name=scope,
                    path=anchor, line=_line_of(facts, "%s:" % scope, access),
                    old_value=moved or "absent", value=access,
                    detail="%s now grants `%s`; the token this job holds can do more than it "
                           "did" % (job, access),
                    action="confirm the job needs `%s`, or narrow it back to %s"
                           % (label, moved or "read"),
                    evidence=["`%s: %s` at %s" % (scope, access, anchor),
                              "was %s" % (moved or "not granted")] + _partial_note(new),
                    dialect=new.dialect, confidence=0.9))

        if run.enabled("ci-oidc-changed"):
            old_identity = {marker for _, marker in old.identity}
            fresh = [(line, marker) for line, marker in new.deduped_identity()
                     if marker not in old_identity and "not external" not in marker]
            fresh_env = [(job, name) for job, name in sorted(new.environments.items())
                         if old.environments.get(job) != name]
            if fresh or fresh_env:
                line = fresh[0][0] if fresh else _line_of(facts, "environment:", None)
                evidence = ["added at %s:%d `%s`" % (anchor, marker_line, marker)
                            for marker_line, marker in fresh]
                evidence += ["`environment: %s` on job %s (was %s)"
                             % (name, job, old.environments.get(job) or "unbound")
                             for job, name in fresh_env]
                findings.append(Finding(
                    kind="ci-oidc-changed",
                    name=fresh[0][1][:60] if fresh else "environment: %s"
                         % fresh_env[0][1],
                    path=anchor, line=line,
                    detail="%s: the workflow can now become an identity something else trusts"
                           % ("a role or login appeared" if fresh else
                              "a job moved to a different environment"),
                    action="review which cloud role this lets the workflow assume, and who "
                           "can push to it",
                    evidence=evidence + _partial_note(new),
                    dialect=new.dialect, confidence=0.9))

        if run.enabled("ci-runner-changed"):
            for job, labels in sorted(new.runners.items()):
                was = old.runners.get(job)
                if was == labels:
                    continue
                if not labels:
                    continue
                host = [label for label in labels if label not in ("ubuntu-latest",
                                                                   "ubuntu-22.04",
                                                                   "ubuntu-24.04",
                                                                   "windows-latest",
                                                                   "macos-latest",
                                                                   "self-hosted")]
                if "self-hosted" not in labels and not (was and set(labels) != set(was)):
                    continue
                findings.append(Finding(
                    kind="ci-runner-changed", name=", ".join(labels), path=anchor,
                    line=_line_of(facts, "runs-on:", labels[0]),
                    detail="job `%s` now runs on %s%s"
                           % (job, ", ".join("`%s`" % label for label in labels),
                              " (was %s)" % ", ".join(was) if was else ""),
                    action="confirm what that runner can reach: network, cache, credentials "
                           "on the host",
                    evidence=["labels: %s" % ", ".join(labels),
                              "runner-owned: %s" % (", ".join(host) or "none named besides "
                                                    "`self-hosted`")] + _partial_note(new),
                    dialect=new.dialect, confidence=0.85))

        if run.enabled("ci-trigger-main-push"):
            now, why = new.pushes_default(defaults)
            before_was, _ = old.pushes_default(defaults)
            if now and not before_was:
                line = _line_of(facts, "on:", None) or 1
                findings.append(Finding(
                    kind="ci-trigger-main-push",
                    name="%s → %s" % ("push", ", ".join(new.triggers.get("push", []))
                                       or "every branch"),
                    path=anchor, line=line,
                    detail="this workflow now runs on a push to the default branch: %s" % why,
                    action="confirm what this workflow does on merge — publish, deploy, sign, "
                           "or hand a token to a script",
                    evidence=["triggers: %s" % (", ".join(sorted(new.triggers)) or "none"),
                              "jobs: %s" % (", ".join(new.jobs[:6]) or "unparsed")]
                             + _partial_note(new),
                    dialect=new.dialect, confidence=0.9))
    return findings


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _other_ci(workflow: Workflow, scan: Y.Scan) -> None:
    """Best-effort trigger/permission reading for the non-GitHub CI dialects."""
    if workflow.dialect == "gitlab":
        for node in scan.nodes:
            if node.key == "workflow:rules" or node.path and "rules" in node.path:
                for value in node.list_value():
                    if "$CI_COMMIT_BRANCH" in value:
                        workflow.triggers.setdefault("push", []).append("rules")
            if node.key in ("secrets", "variables") and node.value:
                workflow.secrets.setdefault(node.value, node.lineno)
    if workflow.dialect == "circleci":
        for node in scan.nodes:
            if node.key == "branches" or node.key == "filters":
                workflow.triggers.setdefault("push", []).extend(node.list_value())
    if workflow.dialect == "jenkins":
        workflow.notes.append("Jenkinsfile read as text: stages and credentials() calls are "
                              "recognised, the pipeline library is not")
    if workflow.dialect == "gitlab":
        workflow.notes.append("`.gitlab-ci.yml` is read for rules, secrets and variables "
                              "only: matrix, includes and reusable templates are not followed")
    if workflow.dialect == "circleci":
        workflow.notes.append("CircleCI config is read for branch filters and secrets only: "
                              "orbs and executors are not followed")
    workflow.partial = True


def _dialect(path: str) -> str:
    base = path.rsplit("/", 1)[-1].lower()
    if "workflows" in path or base.startswith(("ci.yml", "ci.yaml", "release.yml")) and \
            ".github" in path:
        return "github-actions"
    if "circleci" in path:
        return "circleci"
    if base.startswith(".gitlab-ci") or "gitlab" in path:
        return "gitlab"
    if base == "jenkinsfile":
        return "jenkins"
    if "buildkite" in path:
        return "buildkite"
    if base.startswith("azure-pipelines"):
        return "azure-pipelines"
    return "github-actions"


def _job_of(node: Y.Node) -> str:
    for index, key in enumerate(node.path):
        if key == "jobs" and index + 1 < len(node.path):
            return node.path[index + 1]
    return "top"


def _line_of(facts, key: str, value) -> int:
    for line in facts.diff.added:
        if key in line.text and (value is None or str(value) in line.text):
            return line.number
    for lineno, raw in enumerate((facts.after or "").splitlines(), 1):
        if key in raw and (value is None or str(value) in raw):
            return lineno
    return 1


def _scalar(value: str) -> str:
    return value.strip().strip("'\"[]")


def _secret_scope(workflow: Workflow, name: str) -> str:
    environments = sorted(set(workflow.environments.values()))
    if environments:
        return "the `%s` environment secret store (not repository secrets)" \
               % "', `".join(environments)
    return "the repository or organisation secret store"


def scope_evidence(workflow: Workflow) -> str:
    environments = sorted(set(workflow.environments.values()))
    if environments:
        return "jobs bound to environments: %s" % ", ".join(environments)
    return "no `environment:` key in this workflow, so it reads repository-level secrets"


def _partial_note(workflow: Workflow) -> list[str]:
    if not workflow.partial:
        return []
    return ["this file was read line-wise; %s" % workflow.notes[0]]
