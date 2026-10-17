"""Dockerfiles, compose files and image references: the base image is a dependency you run.

A base image change is the one dependency bump that can change behaviour without changing a
line of your code, and it arrives through a diff in a file most reviewers skim. So this
module reads image *references* with the same seriousness as a lockfile, and reports three
things:

**Changed** — `FROM python:3.11-slim` → `FROM python:3.13-slim`, or a different vendor
entirely. Compared per stage, keyed by `AS builder` name first and stage index second,
because a multi-stage Dockerfile that reorders its stages is not a base change. A
registry/repo change or a major-tag jump is high severity; a patch or digest refresh is
notable, and the finding says which of the two it is, since "rebuild and re-test" costs
different amounts of sleep depending on the answer.

**Unpinned** — `FROM nginx` (no tag), `FROM node:latest`, or a `@sha256:` digest that was
removed. A floating tag means the image you build next week is not the image reviewed today,
and every rebuild is an unreviewed change. `non_code.container.require_digest_pin` is a
knob because some teams rebuild nightly and consider the floating tag the honest answer
rather than a lie; the finding only fires on added or changed `FROM` lines, so it never
nags about a Dockerfile this episode only touched a `COPY` in.

**Privileged / supply chain** — `USER root` added or the non-root `USER` removed (a
container that runs as root is one escape away from a host that does), `--privileged` or a
capability granted, and `curl … | sh` in a `RUN`, which makes the build's reproducibility
and the reviewer's threat model depend on a server nobody reviewed. Compose and Kubernetes
are read for the same shapes (`privileged: true`, `runAsUser: 0`, an `image:` tag that
moved), because a Deployment is a Dockerfile with a scheduler in front of it.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from . import yamltext as Y
from .policy import SURFACE_CONTAINER, Finding
from .surfaces import Run

INSTRUCTION = re.compile(r"^\s*(?P<name>[A-Za-z]+)\s+(?P<args>\S.*)$")
BASE_IMAGE = re.compile(r"^FROM\s+(?P<flags>--\S+\s+)*(?P<image>[^\s]+)"
                        r"(?:\s+AS\s+(?P<stage>\w+))?", re.I)
SUPPLY_CHAIN = re.compile(r"(curl|wget)\s[^|]*\|\s*(sudo\s+)?(ba|z|da)?sh\b|"
                          r"iwr\s[^|]*\|\s*iex|"
                          r"(pip|pip3|npm)\s+install\s+(?!-)(?!.*==)(?!.*@)\S+", re.I)
ROOT_USER = re.compile(r"^\s*USER\s+(root|0)\b", re.I)
PRIVILEGED_TEXT = re.compile(r"(?i)--privileged|--cap-add=sys_admin|privileged\s*[:=]\s*true|"
                             r"runasuser\s*:\s*0|hostpid\s*:\s*true")


@dataclass(frozen=True)
class Base:
    """One `FROM` (or `image:`) line, decomposed."""

    stage: str
    image: str
    registry: str = ""
    repo: str = ""
    tag: str = ""
    digest: str = ""
    path: str = ""
    line: int = 0
    dialect: str = "dockerfile"

    @property
    def reference(self) -> str:
        text = self.repo or self.image
        if self.tag:
            text += ":" + self.tag
        if self.digest:
            text += "@" + self.digest[:19] + "…"
        return text

    @property
    def pinned(self) -> bool:
        return bool(self.digest) or bool(self.tag and self.tag != "latest")

    @property
    def major(self) -> str:
        """The numeric prefix of the tag: `3.11-slim` → `3.11`, `20-alpine` → `20`.

        Splitting on the first dot would call `python:3.11` and `python:3.13` the same major,
        which is exactly the change a reviewer most wants to hear about.
        """
        match = re.match(r"\d+(?:\.\d+)?", self.tag or "")
        return match.group(0) if match else (self.tag or "")

    def location(self) -> str:
        return "%s:%d" % (self.path, self.line)


@dataclass
class ImageFile:
    """One container definition: its stages, users, privileges and build-time fetches."""

    path: str
    dialect: str = "dockerfile"
    bases: dict[str, Base] = field(default_factory=dict)
    users: list[tuple[int, str]] = field(default_factory=list)
    privileged: list[tuple[int, str]] = field(default_factory=list)
    fetches: list[tuple[int, str]] = field(default_factory=list)
    images: list[tuple[int, str]] = field(default_factory=list)   # compose/k8s `image:`
    notes: list[str] = field(default_factory=list)

    def as_dict(self) -> dict:
        return {"path": self.path, "dialect": self.dialect,
                "bases": {key: value.reference for key, value in sorted(self.bases.items())},
                "users": [[line, name] for line, name in self.users],
                "privileged": [[line, text] for line, text in self.privileged],
                "fetches": [[line, text] for line, text in self.fetches],
                "images": [[line, text] for line, text in self.images],
                "notes": list(self.notes)}


def split_reference(reference: str) -> tuple[str, str, str, str]:
    """`(registry, repo, tag, digest)` from `ghcr.io/org/app:1.2@sha256:…`."""
    digest = ""
    if "@" in reference:
        reference, digest = reference.split("@", 1)
    tag = ""
    head, _, tail = reference.rpartition(":")
    if head and "/" not in tail:
        reference, tag = head, tail
    registry = ""
    repo = reference
    bits = reference.split("/")
    if len(bits) > 1 and ("." in bits[0] or ":" in bits[0] or bits[0] == "localhost"):
        registry, repo = bits[0], "/".join(bits[1:])
    return registry, repo, tag, digest


def parse(path: str, text: str | None) -> ImageFile | None:
    """Read a Dockerfile, compose file or manifest for its container surface."""
    if text is None:
        return None
    base = path.rsplit("/", 1)[-1].lower()
    dialect = _dialect(path, text)
    if not dialect:
        return None
    image = ImageFile(path=path, dialect=dialect)
    if dialect == "dockerfile":
        stage = "0"
        pending_continuation = False
        for lineno, raw in enumerate(text.splitlines(), 1):
            line = raw
            if pending_continuation:
                match = INSTRUCTION.match(line.strip())
                if match:
                    line = match.group("args")
                    lineno = pending_continuation
            pending_continuation = line.rstrip().endswith("\\") and not line.lstrip(
            ).startswith("#")
            if line.lstrip().startswith("#"):
                continue
            match = BASE_IMAGE.match(line)
            if match:
                registry, repo, tag, digest = split_reference(match.group("image"))
                stage = match.group("stage") or str(len(image.bases))
                image.bases[stage] = Base(stage=stage, image=match.group("image"),
                                         registry=registry, repo=repo, tag=tag,
                                         digest=digest, path=path, line=lineno)
                continue
            instruction = INSTRUCTION.match(line)
            if not instruction:
                continue
            name = instruction.group("name").upper()
            args = instruction.group("args")
            if name == "USER":
                image.users.append((lineno, args.strip()))
            if ROOT_USER.match(line):
                image.privileged.append((lineno, line.strip()))
            if PRIVILEGED_TEXT.search(line):
                image.privileged.append((lineno, line.strip()))
            if name == "RUN" and SUPPLY_CHAIN.search(args):
                image.fetches.append((lineno, args.strip()))
        if not image.bases:
            image.notes.append("no FROM instruction found; read as a container file anyway "
                               "because the path says so")
        return image
    scan = Y.scan(text)
    image.notes.extend(scan.notes)
    for node in scan.nodes:
        if node.key == "image" and node.value:
            image.images.append((node.lineno, node.value))
        if node.key == "runsOn" or node.key == "name":
            continue
        if PRIVILEGED_TEXT.search(node.raw):
            image.privileged.append((node.lineno, node.raw.strip()))
        if node.key == "user" and node.value.strip() in ("root", "0"):
            image.privileged.append((node.lineno, node.raw.strip()))
    return image


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
        if not facts.on("container"):
            run.claim(facts, SURFACE_CONTAINER, "content: a %s file"
                      % (after.dialect if after else before.dialect))
        before = before or ImageFile(path=facts.path, dialect=(after.dialect if after else ""))
        after = after or ImageFile(path=facts.path, dialect=(before.dialect if before else ""))
        findings.extend(_bases(facts, before, after, run))
        findings.extend(_unpinned(facts, after, run))
        findings.extend(_privilege(facts, before, after, run))
        findings.extend(_supply_chain(facts, after, run))
    return findings


def _bases(facts, before: ImageFile, after: ImageFile, run: Run) -> list[Finding]:
    out: list[Finding] = []
    if run.enabled("container-base-changed"):
        for stage in sorted(after.bases):
            new = after.bases[stage]
            old = before.bases.get(stage)
            if old is None:
                if facts.is_added:
                    continue                     # a new file: unpinned/base rows below
                continue                         # a new stage with no predecessor to compare
            if old.reference == new.reference:
                continue
            vendor_changed = (old.registry, old.repo) != (new.registry, new.repo)
            major_changed = not vendor_changed and old.major != new.major
            severity_words = ("the vendor changed" if vendor_changed else
                              "the major version moved" if major_changed else
                              "a tag/digest refresh")
            out.append(Finding(
                kind="container-base-changed", name=new.repo or new.image, path=facts.path,
                line=new.line, old_path=facts.path, old_line=old.line,
                old_value=old.reference, value=new.reference,
                detail="stage `%s`: %s (%s → %s)" % (stage, severity_words, old.reference,
                                                     new.reference),
                action="rebuild the image and re-run its smoke tests; the binaries in it were "
                       "built against %s" % old.reference,
                evidence=["was `%s` at %s" % (old.reference, old.location()),
                          "now `%s` at %s" % (new.reference, new.location()),
                          severity_words],
                dialect=new.dialect, confidence=0.95))
        for line, reference in after.images:
            if any(line == old_line and reference == old_ref for old_line, old_ref
                   in before.images):
                continue
            old_refs = [ref for _, ref in before.images]
            if reference in old_refs:
                continue
            out.append(Finding(
                kind="container-base-changed", name=reference, path=facts.path, line=line,
                detail="an image reference this file did not name before",
                action="rebuild and re-test: this tag is what the running pod will pull",
                evidence=["`image: %s` at %s:%d" % (reference, facts.path, line),
                          "was: %s" % (", ".join(old_refs[:3]) or "nothing named here")],
                dialect=after.dialect, confidence=0.85))
    return out


def _unpinned(facts, after: ImageFile, run: Run) -> list[Finding]:
    out: list[Finding] = []
    if not run.enabled("container-base-unpinned") or not run.policy.flag("require_digest_pin"):
        return out
    added = {line.number: line.text for line in facts.diff.added}
    for stage, base in sorted(after.bases.items()):
        if base.digest:
            continue
        floating = not base.tag or base.tag == "latest"
        if not floating and not run.policy.flag("require_digest_pin"):
            continue
        if facts.diff.added and base.line not in added:
            continue                             # pre-existing line, untouched by this episode
        out.append(Finding(
            kind="container-base-unpinned", name=base.reference or base.image,
            path=facts.path, line=base.line,
            detail="stage `%s` floats: %s" % (stage, _why_unpinned(base)),
            action="pin a digest (`FROM %s@sha256:…`) so the image you build matches the one "
                   "you reviewed" % (base.repo or base.image),
            value="unpinned",
            evidence=["`FROM %s` at %s" % (base.image, base.location()),
                      "non_code.container.require_digest_pin = true"],
            dialect=base.dialect, confidence=0.9))
    return out


def _privilege(facts, before: ImageFile, after: ImageFile, run: Run) -> list[Finding]:
    if not run.enabled("container-privilege-changed"):
        return []
    out: list[Finding] = []
    old_privileged = {text for _, text in before.privileged}
    new_privileged = {text for _, text in after.privileged}
    for line, text in after.privileged:
        if text in old_privileged:
            continue
        out.append(Finding(
            kind="container-privilege-changed", name=text[:60], path=facts.path, line=line,
            detail="the container gains privilege: `%s`" % text,
            action="confirm the process needs this; a container escape is now a root escape",
            evidence=["added at %s:%d" % (facts.path, line)],
            dialect=after.dialect, confidence=0.9))
    old_users = {name for _, name in before.users}
    new_users = {name for _, name in after.users}
    if new_users and new_users <= old_users:
        return out
    if old_users and not new_users and facts.change_type != "deleted":
        out.append(Finding(
            kind="container-privilege-changed", name="the last USER line was removed",
            path=facts.path, line=max(1, _last_line(facts, "USER")),
            detail="with no `USER` the process runs as root",
            action="put a non-root `USER` back, or say why the process needs uid 0",
            evidence=["was: %s" % ", ".join(sorted(old_users))],
            dialect=after.dialect, confidence=0.85))
    return out


def _supply_chain(facts, after: ImageFile, run: Run) -> list[Finding]:
    if not run.enabled("container-supply-chain"):
        return []
    out: list[Finding] = []
    known = {text for _, text in (parse(facts.path, facts.before) or after).fetches}
    added = {line.number: line.text for line in facts.diff.added}
    for line, text in after.fetches:
        if text in known:
            continue
        if facts.diff.added and line not in added and not facts.is_added:
            continue
        out.append(Finding(
            kind="container-supply-chain", name=text[:60], path=facts.path, line=line,
            detail="the build runs whatever a server returns at build time",
            action="vendor and pin what you are installing; a `| sh` in an image is a "
                   "dependency you cannot review",
            evidence=["`%s` at %s:%d" % (text, facts.path, line)],
            dialect=after.dialect, confidence=0.85))
    return out


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _dialect(path: str, text: str) -> str:
    base = path.rsplit("/", 1)[-1].lower()
    if base.startswith(("dockerfile", "containerfile")) or base.endswith(".dockerfile"):
        return "dockerfile"
    if base.startswith("docker-compose") or base in ("compose.yml", "compose.yaml"):
        return "compose"
    head = "\n".join(text.splitlines()[:60])
    if re.search(r"(?m)^(kind|apiVersion)\s*:", head):
        return "kubernetes"
    return ""


def _why_unpinned(base: Base) -> str:
    if not base.tag and not base.digest:
        return "no tag at all, which means `:latest`"
    if base.tag == "latest":
        return "the tag is `latest`"
    if not base.digest:
        return "tagged but not digest-pinned, so the tag can be re-pointed"
    return "not pinned"


def _last_line(facts, needle: str) -> int:
    numbers = [lineno for lineno, raw in enumerate((facts.before or "").splitlines(), 1)
               if needle in raw]
    return numbers[-1] if numbers else 1
