"""What a changed file *is*, as far as this ticket is concerned — and the shared plumbing.

`e2.1` already answers "what is this path" for the whole product (`config`, `migration`,
`schema`, `source`, …), and this package reads that answer rather than re-deriving it; the
test that pins it is `test_the_class_of_a_file_comes_from_the_classifier_not_from_a_guess`.
What is *this* ticket's question is different: a file can be on several non-code surfaces at
once, and the interesting cases are exactly the overlaps.

| file | `e2.1` class | surfaces here |
| --- | --- | --- |
| `.github/workflows/deploy.yml` | `config` | `ci` **and** `env` (its `env:` block declares variables) |
| `k8s/manifests/api.yaml` | `config` | `iac` (it is a Deployment), `env` (its `env:`/`configMapKeyRef` block), `container` (its `image:` line) |
| `Dockerfile` | `config` | `container` **and** `env` (`ENV` is how an image gets a variable) |
| `db/migrate/0002_x.py` | `migration` | `migration`, plus `env` if it reads `ENV.fetch` |
| `src/settings.py` | `source` | `env` — a `source` file is on the env surface because that is where the *reads* live |

So attribution here produces a **set per file**, each entry carrying the reason it was
added, and it is deliberately not a partition. Surfaces come from three places:

1. the `e0.9`/`e2.1` taxonomy class (`migration` maps straight to the migration surface);
2. a path table per surface, part built-in (`.github/workflows/**`, `Dockerfile*`, `*.tf`)
   and part `fathom.toml` (`non_code.env.declaration_globs`, `non_code.iac.globs`, …, plus
   the `non_code.patterns` list `e0.8` already ships);
3. **content**, where a path cannot tell you: a YAML file with `kind:` at column 0 is a
   Kubernetes manifest whatever it is called, and a `flags:` block in an odd filename is a
   flag file. Content attribution always says so in its reason.

The rest of this module is the two shapes every detector takes as input — :class:`Change`
(one file, both revisions) and :class:`Episode` (a change set) — plus :func:`line_diff`,
which is what lets a detector say "this appeared on an added line" instead of "this string
is somewhere in the file". That distinction is the difference between a finding and a nag:
half the false positives this package could produce come from noticing something that was
already true before the episode.
"""

from __future__ import annotations

import difflib
from dataclasses import dataclass, field

from recommender.change.engine import Change as ClassifierChange
from recommender.change.engine import ChangeClassifier
from recommender.taxonomy import classes as C

from .policy import (SURFACE_CI, SURFACE_CONTAINER, SURFACE_ENV, SURFACE_FLAG, SURFACE_IAC,
                     SURFACE_MIGRATION, SURFACES, Policy)

#: Version of the attribution rules and the delta shape. Bumped when a finding means
#: something different than it did, so a cached survey recomputes.
SURVEY_VERSION = 1

#: Path shapes that are CI no matter what the config says. Kept here rather than in a knob:
#: these are the file names the tools themselves define, and a repository that renames its
#`.github/workflows` directory has a problem a glob will not fix.
CI_PATH_GLOBS: tuple[str, ...] = (
    ".github/workflows/**", ".github/actions/**", "**/.github/workflows/**",
    ".circleci/**", ".gitlab-ci.yml", ".gitlab-ci*.y*ml", "Jenkinsfile", "**/Jenkinsfile",
    ".buildkite/**", ".drone.yml", ".woodpecker.y*ml", "azure-pipelines.yml",
    ".teamcity/**", ".github/action.y*ml",
)

#: Files that are a container definition. Compose files are on two surfaces at once: a
#: service is an infrastructure resource *and* an image reference.
CONTAINER_PATH_GLOBS: tuple[str, ...] = (
    "Dockerfile", "**/Dockerfile", "**/Dockerfile.*", "Dockerfile.*", "*.dockerfile",
    "Containerfile", "**/Containerfile", "**/Containerfile.*", "docker-compose.yml",
    "docker-compose.y*ml", "**/docker-compose.y*ml", "compose.yml", "compose.y*ml",
    "**/compose.y*ml", "docker-compose.override.yml",
)

#: IaC by filename alone; the content rule for manifests lives in `iac.py`.
IAC_PATH_GLOBS: tuple[str, ...] = (
    "**/*.tf", "**/*.tfvars", "**/*.tf.json", "**/*.hcl", "Pulumi.*.yaml",
    "**/*.stack.yaml", "Chart.yaml", "**/Chart.yaml",
)

#: A migration filename that says which way it goes, per tool.
DIRECTION_SUFFIXES: tuple[tuple[str, str], ...] = (
    (".up.sql", "up"), (".up", "up"), (".down.sql", "down"), (".down", "down"),
    (".rollback.sql", "down"),
)


# ---------------------------------------------------------------------------
# the input shapes
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Change:
    """One file in an episode: the path and whichever revisions the caller has."""

    path: str
    before: str | None = None
    after: str | None = None
    change_type: str = "modified"
    old_path: str = ""

    @property
    def subject(self) -> str:
        return self.path or self.old_path

    @property
    def content(self) -> str | None:
        return self.after if self.after is not None else self.before

    @property
    def is_added(self) -> bool:
        return self.change_type == "added" or (self.before is None and self.after is not None)

    @property
    def is_deleted(self) -> bool:
        return self.change_type == "deleted" or (self.after is None and self.before is not None)

    @property
    def is_renamed(self) -> bool:
        return self.change_type == "renamed" or bool(self.old_path and self.before is not None
                                                    and self.after is not None)

    @staticmethod
    def from_pair(path: str, before: str | None, after: str | None, **kwargs) -> "Change":
        change_type = "modified"
        if before is None and after is not None:
            change_type = "added"
        if after is None and before is not None:
            change_type = "deleted"
        return Change(path=path, before=before, after=after, change_type=change_type, **kwargs)


@dataclass(frozen=True)
class Episode:
    """A change set, plus the repository-wide context a diff cannot supply.

    `repo_env_refs` and `repo_flag_reads` are the honest answer to "does anything still read
    this?" — a question that needs the whole tree. They are injected rather than read, per
    ADR-002 §4.5, and when `non_code.env.reference_scope = repo` was asked for and the
    caller supplied nothing, the finding says so instead of quietly guessing.
    """

    id: str
    files: tuple[Change, ...] = ()
    title: str = ""
    repo_env_refs: tuple[str, ...] = ()
    repo_flag_reads: tuple[str, ...] = ()

    @classmethod
    def from_pairs(cls, episode_id: str, pairs: dict[str, tuple], **kwargs) -> "Episode":
        return cls(id=episode_id,
                   files=tuple(Change.from_pair(path, before, after)
                               for path, (before, after) in sorted(pairs.items())),
                   **kwargs)

    @property
    def paths(self) -> list[str]:
        return [change.subject for change in self.files]


# ---------------------------------------------------------------------------
# line-level diff
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Line:
    number: int                            # 1-based, in the revision it belongs to
    text: str


@dataclass(frozen=True)
class LineDiff:
    """Added and removed lines with the line numbers a finding has to point at."""

    added: tuple[Line, ...] = ()
    removed: tuple[Line, ...] = ()

    @property
    def empty(self) -> bool:
        return not self.added and not self.removed

    def added_matching(self, predicate) -> list[Line]:
        return [line for line in self.added if predicate(line.text)]

    def removed_matching(self, predicate) -> list[Line]:
        return [line for line in self.removed if predicate(line.text)]


def line_diff(before: str | None, after: str | None) -> LineDiff:
    """Compare two revisions line by line.

    An added file is every line added; a deleted file is every line removed. That matters:
    a brand-new `Dockerfile` has to be able to raise the finding about its unpinned base
    image even though nothing was "changed" in the diff-hunk sense.
    """
    old_lines = (before or "").splitlines()
    new_lines = (after or "").splitlines()
    if before is None:
        return LineDiff(added=tuple(Line(i, text) for i, text in enumerate(new_lines, 1)))
    if after is None:
        return LineDiff(removed=tuple(Line(i, text) for i, text in enumerate(old_lines, 1)))
    added: list[Line] = []
    removed: list[Line] = []
    matcher = difflib.SequenceMatcher(a=old_lines, b=new_lines, autojunk=False)
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag in ("replace", "delete"):
            removed.extend(Line(i, old_lines[i - 1]) for i in range(i1 + 1, i2 + 1))
        if tag in ("replace", "insert"):
            added.extend(Line(j, new_lines[j - 1]) for j in range(j1 + 1, j2 + 1))
    return LineDiff(added=tuple(added), removed=tuple(removed))


# ---------------------------------------------------------------------------
# attribution
# ---------------------------------------------------------------------------
@dataclass
class FileFacts:
    """One file: what it is, which surfaces it is on, and how we found out."""

    path: str
    change: Change
    cls: str = C.PathClass.OTHER
    class_confidence: float = 0.0
    class_reason: str = ""
    surfaces: list[str] = field(default_factory=list)
    reasons: list[str] = field(default_factory=list)
    diff: LineDiff = field(default_factory=LineDiff)
    notes: list[str] = field(default_factory=list)
    declared_non_code: str = ""

    def on(self, *surface: str) -> bool:
        return any(name in self.surfaces for name in surface)

    @property
    def change_type(self) -> str:
        return self.change.change_type

    @property
    def is_added(self) -> bool:
        return self.change.is_added

    @property
    def is_deleted(self) -> bool:
        return self.change.is_deleted

    @property
    def after(self) -> str | None:
        return self.change.after

    @property
    def before(self) -> str | None:
        return self.change.before

    def as_dict(self) -> dict:
        return {"path": self.path, "class": self.cls, "class_confidence":
                round(self.class_confidence, 3), "surfaces": list(self.surfaces),
                "reasons": list(self.reasons), "change_type": self.change_type,
                "declared_by": self.declared_non_code, "notes": list(self.notes),
                "added_lines": len(self.diff.added), "removed_lines": len(self.diff.removed)}


class Run:
    """One episode's worth of context, shared by every detector.

    Detectors take a `Run` and return findings; they never open a file, never read
    `os.environ`, and never look at a clock (ADR-002 §4.5). Anything a detector needs from
    outside the episode arrives on the :class:`Episode`.
    """

    def __init__(self, episode: Episode, policy: Policy | None = None,
                 classifier: ChangeClassifier | None = None) -> None:
        self.episode = episode
        self.policy = policy or Policy.built_in()
        self.classifier = classifier or ChangeClassifier.default()
        self.files: list[FileFacts] = [self._facts(change) for change in episode.files]
        self.notes: list[str] = self.policy.problems()
        self._by_surface: dict[str, list[FileFacts]] = {name: [] for name in SURFACES}
        for facts in self.files:
            for name in facts.surfaces:
                self._by_surface[name].append(facts)

    # -- per-file facts -----------------------------------------------------
    def _facts(self, change: Change) -> FileFacts:
        path = change.subject
        decision = self.classifier.classify(ClassifierChange(
            path=path, content=(change.after if change.after is not None else change.before)))
        facts = FileFacts(path=path, change=change, cls=decision.cls,
                          class_confidence=decision.confidence, class_reason=decision.reason,
                          diff=line_diff(change.before, change.after))
        surfaces: list[str] = []
        reasons: list[str] = []

        def add(surface: str, reason: str) -> None:
            if surface not in surfaces:
                surfaces.append(surface)
            reasons.append("%s: %s" % (surface, reason))

        if facts.cls == C.PathClass.MIGRATION:
            add(SURFACE_MIGRATION, "taxonomy class `migration` (`e2.1` says so: %s)"
                % _first_clause(facts.class_reason))
        if facts.cls == C.PathClass.SOURCE or facts.cls == C.PathClass.TEST:
            add(SURFACE_ENV, "a `%s` file is where environment reads live" % facts.cls)
        for glob in CI_PATH_GLOBS:
            if _match(glob, path):
                add(SURFACE_CI, "path %s is a CI definition" % glob)
                break
        for glob in CONTAINER_PATH_GLOBS:
            if _match(glob, path):
                add(SURFACE_CONTAINER, "path %s is a container definition" % glob)
                break
        for glob in IAC_PATH_GLOBS:
            if _match(glob, path):
                add(SURFACE_IAC, "path %s is infrastructure code" % glob)
                break
        if self.policy.matches("migration_globs", path):
            add(SURFACE_MIGRATION, "`non_code.migration.globs` claims %s"
                % self.policy.matches("migration_globs", path))
        if self.policy.matches("iac_globs", path):
            add(SURFACE_IAC, "`non_code.iac.globs` claims %s"
                % self.policy.matches("iac_globs", path))
        if self.policy.matches("flag_globs", path):
            add(SURFACE_FLAG, "`non_code.flags.globs` claims %s"
                % self.policy.matches("flag_globs", path))
        if self.policy.matches("declaration_globs", path) or \
                self.policy.matches("committed_globs", path):
            add(SURFACE_ENV, "`non_code.env` globs claim %s" % path)
        declared = self.policy.pattern_that_claims(path)
        if declared:
            facts.declared_non_code = "`non_code.patterns` = %s" % declared
            add(
                _surface_for_path(path) or SURFACE_ENV,
                "`non_code.patterns` claims %s, so it is reviewed as a surface, not as lines"
                % declared)
        # Content decides where a path cannot: Kubernetes manifests, CloudFormation, and a
        # flags file whose name is not in any table. Done by the detectors, which already
        # have the parsed shape, and recorded here so the reason list is complete.
        facts.surfaces = surfaces
        facts.reasons = reasons
        return facts

    # -- the questions the detectors ask ------------------------------------
    def note(self, facts: "FileFacts", text: str) -> None:
        """A reader could not fully read this file: record it where the card will say it.

        A finding carries its own caveat in its evidence, but a file that produced *no*
        finding is the one where silence is easiest to mistake for a clean result. These
        notes become `NonCodeDelta.partials`, which is also what pulls the episode's
        stated confidence down.
        """
        if text and text not in facts.notes:
            facts.notes.append(text)

    def claim(self, facts: FileFacts, surface: str, reason: str) -> None:
        """A detector found a content-level reason for a surface; record it once."""
        if surface not in facts.surfaces:
            facts.surfaces.append(surface)
            self._by_surface.setdefault(surface, []).append(facts)
        facts.reasons.append("%s: %s" % (surface, reason))

    def files_on(self, surface: str) -> list[FileFacts]:
        return list(self._by_surface.get(surface, []))

    def enabled(self, kind: str) -> bool:
        """Does this finding kind report in this survey?

        `measure` builds the run from :meth:`Policy.all_enabled` and applies the repository's
        switches to the results, so that a disabled finding is counted and printed as
        suppressed instead of never computed. A detector that skipped its own work here would
        turn that count into a guess.
        """
        return self.policy.enabled_for(kind)

    def changed(self, facts: FileFacts) -> LineDiff:
        """The diff to look at: the file's own, or the empty one for a deleted file's new side."""
        return facts.diff

    def census(self) -> dict[str, int]:
        """How many files sit on each surface — the header line and the `surfaces` verb."""
        return {name: len(self._by_surface.get(name, [])) for name in SURFACES}

    def has_class(self, cls: str) -> bool:
        return any(facts.cls == cls for facts in self.files)

    def paths_of_class(self, cls: str) -> list[str]:
        return [facts.path for facts in self.files if facts.cls == cls]

    def text(self, facts: FileFacts, side: str) -> str:
        value = facts.after if side == "after" else facts.before
        return value if value is not None else ""

    def as_dict(self) -> dict:
        return {"episode_id": self.episode.id, "survey_version": SURVEY_VERSION,
                "files": [facts.as_dict() for facts in self.files],
                "surfaces": self.census(), "notes": list(self.notes)}


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _surface_for_path(path: str) -> str:
    """Which surface a `non_code.patterns` hit belongs to, `""` when nothing says."""
    from .policy import SURFACE_CI, SURFACE_CONTAINER, SURFACE_IAC, SURFACE_MIGRATION
    for globs, surface in ((CI_PATH_GLOBS, SURFACE_CI), (CONTAINER_PATH_GLOBS,
                                                         SURFACE_CONTAINER),
                           (IAC_PATH_GLOBS, SURFACE_IAC),
                           (("**/migrations/**", "db/migrate/**", "**/alembic/versions/**"),
                            SURFACE_MIGRATION)):
        if any(_match(glob, path) for glob in globs):
            return surface
    return ""


def _match(glob: str, path: str) -> bool:
    from recommender.config.globs import matches
    return bool(matches(glob, path))


def _first_clause(text: str, limit: int = 90) -> str:
    for separator in (";", ",", " · "):
        if separator in text:
            text = text.split(separator)[0]
            break
    text = text.strip()
    return text if len(text) <= limit else text[: limit - 1] + "…"
