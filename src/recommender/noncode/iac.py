"""Infrastructure-as-code deltas: the plan is the finding (`e2.8`).

IaC is the surface where a diff is a *command waiting to run*: `terraform apply` reads the
same two revisions this module reads and calls `destroy` on whatever disappeared. So the
findings here are plan-shaped, not style-shaped, and there are exactly three of them —
something new, something gone, something whose exposure got wider — because those are the
three a plan review has to answer and the only three that can be stated from a text diff
without pretending to be a planner.

**What counts as a resource** differs per dialect, and each is read in its own shape rather
than with one generic "keys that look like resources" heuristic:

| dialect | resource identity |
| --- | --- |
| Terraform | `resource "aws_s3_bucket" "logs"` → `aws_s3_bucket.logs` (`data` and `provider` blocks are not resources; a `module` is) |
| CloudFormation | `Resources.<LogicalId>` with its `Type:` as the value |
| Kubernetes | `<kind>/<metadata.name>` — detected by `kind:` at column 0, because nothing about a filename says a YAML file is a Deployment |
| Compose | `services.<name>` — a service is infrastructure with an image tag on it |
| Helm values | a top-level key that gained/lost a whole block, reported as a values delta rather than a resource |

**Removed is louder than added.** Adding a bucket costs money; removing one deletes data.
A removed resource is high severity with the dialect's own word for it (`destroy`, `Delete`,
`kubectl delete`) so the reviewer sees the verb the tool will use, and a *deleted file* is
reported per resource rather than once, because a 40-resource Terraform file deleted
contains 40 destroys.

**Widening is matched on added lines only.** `0.0.0.0/0` that has been in the security group
since 2019 is not this episode's finding, and flagging it is how a security review becomes
background noise. The patterns themselves — wildcard principal or action, a default route
from anywhere, a public address, `--privileged` — are
`non_code.iac.widened_patterns`, extendable per cloud, and every finding quotes the line and
names the pattern that matched.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from . import yamltext as Y
from .policy import SURFACE_IAC, Finding
from .surfaces import Run

TF_RESOURCE = re.compile(r'^\s*(?P<kind>resource|module)\s+"(?P<type>[^"]+)"\s+"(?P<name>[^"]+)"')
KIND_NODE = "kind"
WIDEN_REGEX = (
    (re.compile(r'"action"\s*[:,]?\s*\[?\s*"\*"|\baction\s*=\s*\[\s*"\*"\s*\]', re.I),
     "a wildcard Action in an IAM policy"),
    (re.compile(r'"principal"\s*:\s*"\*"|\bprincipal\s*=\s*\[\s*"\*"\s*\]', re.I),
     "a wildcard Principal: the resource is now readable or writable by anybody"),
    (re.compile(r"0\.0\.0\.0/0|::/0"), "an ingress reachable from the whole internet"),
    (re.compile(r"associate_public_ip_address\s*=\s*true"),
     "an instance with a public address"),
    (re.compile(r"(?i)privileged\s*:?\s*(true|yes)|--privileged|--cap-add=sys_admin|"
                r"runasuser:\s*0|hostnetwork:\s*true"),
     "a privileged container: an escape is a root escape"),
    (re.compile(r"(?i)administratoraccess"), "an administrator policy attached"),
    (re.compile(r"(?i)public\s*=\s*true|acl\s*=\s*\"public-read\""),
     "an object or bucket made public"),
)

#: How each dialect says "this will be removed", printed so the reviewer sees the tool's verb.
DESTROY_VERBS = {"terraform": "destroy", "cloudformation": "Delete (stack update)",
                 "kubernetes": "kubectl delete / apply prune", "compose": "down and remove",
                 "helm": "removed from the values"}


@dataclass
class Resource:
    """One managed thing a dialect knows about."""

    key: str
    path: str
    line: int
    dialect: str
    kind: str = ""
    detail: str = ""

    def render(self) -> str:
        return "%s (%s)" % (self.key, self.dialect)


@dataclass
class Manifest:
    """One IaC file, as a set of resources plus the exposures its text grants."""

    path: str
    dialect: str = ""
    resources: dict[str, Resource] = field(default_factory=dict)
    exposures: list[tuple[int, str, str]] = field(default_factory=list)   # (line, text, why)
    notes: list[str] = field(default_factory=list)
    partial: bool = False

    def as_dict(self) -> dict:
        return {"path": self.path, "dialect": self.dialect,
                "resources": sorted(key for key in self.resources),
                "exposures": [{"line": line, "text": text[:80], "why": why}
                              for line, text, why in self.exposures],
                "partial": self.partial, "notes": list(self.notes)}


def detect_dialect(path: str, text: str | None) -> str:
    """Which IaC dialect this file is, `""` when none of them."""
    if text is None:
        return ""
    base = path.rsplit("/", 1)[-1].lower()
    if base.endswith((".tf", ".tfvars", ".tf.json", ".hcl")):
        return "terraform"
    if base.startswith("docker-compose") or base in ("compose.yml", "compose.yaml"):
        return "compose"
    if base == "chart.yaml" or base.startswith("cloudformation") or "cfn" in base:
        return "cloudformation"
    head = "\n".join(text.splitlines()[:60])
    if re.search(r"(?m)^kind\s*:", head) and re.search(r"(?m)^apiVersion\s*:", head):
        return "kubernetes"
    if "AWSTemplateFormatVersion" in head:
        return "cloudformation"
    if base.endswith((".yml", ".yaml")) and re.search(r"(?m)^Resources\s*:", head):
        return "cloudformation"
    return ""


def parse(path: str, text: str | None) -> Manifest | None:
    """Read one IaC file's resources and exposures."""
    if text is None:
        return None
    dialect = detect_dialect(path, text)
    if not dialect:
        return None
    manifest = Manifest(path=path, dialect=dialect)
    if dialect == "terraform":
        _terraform(manifest, text)
    else:
        _yaml_manifest(manifest, text)
    if dialect == "terraform":
        _exposures(manifest, text.splitlines())
    return manifest


def _terraform(manifest: Manifest, text: str) -> None:
    for lineno, raw in enumerate(text.splitlines(), 1):
        match = TF_RESOURCE.match(raw)
        if not match:
            continue
        if match.group("kind") == "data":
            continue
        key = ("%s.%s" % (match.group("type"), match.group("name"))
               if match.group("kind") == "resource" else "module.%s" % match.group("type"))
        manifest.resources[key] = Resource(key=key, path=manifest.path, line=lineno,
                                          dialect="terraform", kind=match.group("type"),
                                          detail=raw.strip())
    if not manifest.resources and "resource" not in text:
        manifest.notes.append("no `resource` blocks: a variables or outputs file")


def _yaml_manifest(manifest: Manifest, text: str) -> None:
    scan = Y.scan(text)
    manifest.partial = bool(scan.notes)
    manifest.notes.extend(scan.notes)
    kind = ""
    names: list[str] = []
    for node in scan.nodes:
        if node.key == KIND_NODE and node.value:
            kind = node.value
        if node.key == "name" and node.path and node.path[-1] == "metadata":
            names.append(node.value)
        if manifest.dialect == "cloudformation" and node.path == ("Resources",) and node.key:
            manifest.resources[node.key] = Resource(key=node.key, path=manifest.path,
                                                   line=node.lineno,
                                                   dialect="cloudformation",
                                                   kind=_resource_type(scan, node.key))
        if manifest.dialect == "compose" and node.path == ("services",) and node.key:
            manifest.resources["service/%s" % node.key] = Resource(
                key="service/%s" % node.key, path=manifest.path, line=node.lineno,
                dialect="compose", kind="service",
                detail=_service_image(scan, node.key))
        if manifest.dialect == "helm" and node.path == () and node.key:
            manifest.resources["values/%s" % node.key] = Resource(
                key="values/%s" % node.key, path=manifest.path, line=node.lineno,
                dialect="helm", kind="value")
    if manifest.dialect == "kubernetes":
        names = names or ["(unnamed)"]
        for index, name in enumerate(names):
            key = "%s/%s" % (kind or "object", name)
            manifest.resources[key] = Resource(key=key, path=manifest.path,
                                               line=scan.nodes[0].lineno if scan.nodes else 1,
                                               dialect="kubernetes", kind=kind)
    if manifest.dialect in ("kubernetes", "compose"):
        _exposures(manifest, text.splitlines())


def _resource_type(scan: Y.Scan, logical_id: str) -> str:
    for node in scan.nodes:
        if node.path and node.path[-2:] == ("Resources", logical_id) and node.key == "Type":
            return node.value
    return ""


def _service_image(scan: Y.Scan, service: str) -> str:
    for node in scan.nodes:
        if node.path and node.path[-2:] == ("services", service) and node.key == "image":
            return node.value
    return ""


def _exposures(manifest: Manifest, lines: list[str]) -> None:
    for lineno, raw in enumerate(lines, 1):
        low = raw.lower()
        for pattern, why in WIDEN_REGEX:
            if pattern.search(low):
                manifest.exposures.append((lineno, raw.strip(), why))
                break


# ---------------------------------------------------------------------------
# the episode
# ---------------------------------------------------------------------------
def detect(run: Run) -> list[Finding]:
    findings: list[Finding] = []
    for facts in run.files:
        before = parse(facts.path, facts.before)
        after = parse(facts.path, facts.after)
        if before is None and after is None:
            continue
        if after is not None and before is None and facts.change_type != "modified":
            run.claim(facts, SURFACE_IAC, "`%s` content is a %s manifest"
                      % (facts.path, after.dialect))
        if after is not None and not facts.on("iac"):
            run.claim(facts, SURFACE_IAC, "content: a %s manifest whatever the path says"
                      % after.dialect)
        before = before or Manifest(path=facts.path, dialect=(after.dialect if after else ""))
        after = after or Manifest(path=facts.path, dialect=(before.dialect if before else ""))
        findings.extend(_resource_deltas(facts, before, after, run))
        findings.extend(_exposure_deltas(facts, after, run))
    return findings


def _resource_deltas(facts, before: Manifest, after: Manifest, run: Run) -> list[Finding]:
    out: list[Finding] = []
    added = [key for key in sorted(after.resources) if key not in before.resources]
    removed = [key for key in sorted(before.resources) if key not in after.resources]
    if run.enabled("iac-resource-added"):
        for key in added:
            resource = after.resources[key]
            out.append(Finding(
                kind="iac-resource-added", name=key, path=facts.path, line=resource.line,
                detail="a new %s resource (%s)" % (resource.dialect, resource.kind or "resource"),
                action="expect `%s` in the plan; check the name is unique and the cost is "
                       "budgeted" % key,
                value=resource.kind, dialect=resource.dialect,
                evidence=["declared at %s:%d `%s`" % (facts.path, resource.line,
                                                      (resource.detail or key)[:70])],
                confidence=0.95))
    if run.enabled("iac-resource-removed"):
        for key in removed:
            resource = before.resources[key]
            verb = DESTROY_VERBS.get(resource.dialect, "remove")
            out.append(Finding(
                kind="iac-resource-removed", name=key, path=facts.path,
                line=max(1, _line_in(facts, key.split(".")[-1])),
                old_path=facts.path, old_line=resource.line,
                detail="this resource is no longer declared, so the next %s will `%s` it"
                       % (resource.dialect, verb),
                action="confirm `%s` should be destroyed%s"
                       % (key, ", or move it out of state first"
                          if resource.dialect == "terraform" else ""),
                value=verb, dialect=resource.dialect,
                evidence=["was declared at %s:%d" % (facts.path, resource.line),
                          "file change: %s" % facts.change_type],
                confidence=0.9 if facts.change_type != "deleted" else 0.85))
    return out


def _exposure_deltas(facts, after: Manifest, run: Run) -> list[Finding]:
    """Widening is decided on *added* lines: see the module docstring for why."""
    out: list[Finding] = []
    if not run.enabled("iac-policy-widened"):
        return out
    added = {line.number: line.text for line in facts.diff.added}
    patterns = [item.strip() for item in run.policy.words("widened_patterns") if item.strip()]
    seen: set[tuple] = set()
    for lineno, text, why in after.exposures:
        if lineno not in added:
            continue
        matched = [item for item in patterns if item in text]
        if not matched and not any(pattern.search(text.lower()) for pattern, _ in WIDEN_REGEX):
            continue                             # not on the repository's list either
        key = (lineno, why)
        if key in seen:
            continue
        seen.add(key)
        out.append(Finding(
            kind="iac-policy-widened", name=why.split(":")[0], path=facts.path, line=lineno,
            detail=why,
            action="review what this lets reach what — the blast radius is not the repository",
            evidence=["added at %s:%d `%s`" % (facts.path, lineno, text[:80]),
                      "matched %s" % (", ".join(matched) if matched
                                       else "the built-in widening pattern")],
            dialect=after.dialect, confidence=0.9))
    return out


def _line_in(facts, needle: str) -> int:
    for lineno, raw in enumerate((facts.before or "").splitlines(), 1):
        if needle in raw:
            return lineno
    return 1
