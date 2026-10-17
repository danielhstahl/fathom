"""Environment variables: what an episode declares, what it reads, and the two claims that
need both to be safe (`e2.8` AC 1 and AC 2).

The ticket's example is the whole design: a variable added to `.env.example` but never read
in code is a finding, and one removed while code still reads it is a finding. Neither is
visible from a file. Both are visible from the *pair* — the declaration set and the
reference set — which is why this module produces two collections and then compares them,
rather than pattern-matching one file at a time.

**Declarations** (things an operator has to set) come from the places a variable is written
down rather than read: `.env.example` and friends (`non_code.env.declaration_globs`), a
committed `.env` (`committed_globs`), a compose `environment:` block, a Kubernetes `env:`
item or `configMapKeyRef`, a CI workflow's `env:` map, a Dockerfile `ENV`, a Terraform
`variable`. Each carries the value if it has one, and whether the file is documentation
(`.example`) or a real environment.

**References** (things that break when the declaration is not there) come from the code, in
the shapes each language actually uses — `os.environ["X"]`, `os.getenv`, `os.Getenv`,
`env::var`, `process.env.X`, `ENV.fetch`, `System.getenv`, `${X:-default}` in shell,
`$(X)` in Kubernetes, `var.x` in Terraform, `${X}` interpolation in compose and Spring
config. Each reference records whether it has a **fallback** at the call site, because "the
variable is missing" and "the variable is missing and the process will notice" are two
different pages at 3 a.m.

**The cross-check** compares the two sets before and after, and its honesty is bounded by
what the episode can see:

| claim | basis | stated confidence |
| --- | --- | --- |
| "nothing reads this new variable" | the episode's files, unless `reference_scope = repo` and the caller supplied the repository's reference set | 0.60 alone, 0.90 with the repository half |
| "code still reads the variable you removed" | a reference is positive evidence, and positive evidence does not need the rest of the tree | 0.95 |
| "this pair is a rename" | name-token similarity ≥ `non_code.env.rename_similarity`, plus the old name still referenced | 0.85 |

The rename rule is the one worth arguing about, so it is a knob and it prints its numbers.
`DATABASE_URL` → `PG_DATABASE_URL` shares both tokens of the shorter name; `DATABASE_URL`
and `REDIS_URL` share one of three and stay two separate findings. A half-done rename is the
production incident this ticket names — the new name is set in the deployment, the old one
is still read by a file nobody touched, and the process starts fine until it needs it.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from .policy import Finding
from .surfaces import FileFacts, Line, LineDiff, Run

#: How much of the truth the episode can see on its own, and with the repository supplied.
CONFIDENCE_EPISODE = 0.6
CONFIDENCE_REPO = 0.9
CONFIDENCE_REFERENCE = 0.95
CONFIDENCE_RENAME = 0.85
CONFIDENCE_VALUE = 0.85

DOTENV_LINE = re.compile(r"^\s*(?:export\s+)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*=(?P<value>.*)$")
DOCKER_ENV = re.compile(r"^\s*(?:ENV|ARG)\s+(?P<body>\S.*)$", re.I)
TERRAFORM_VAR = re.compile(r'^\s*variable\s+"(?P<name>[^"]+)"')
CREDENTIAL_SHAPED = re.compile(
    r"(?i)^(sk|pk)[-_][a-z0-9]{6,}$|^(akia|asia|abia)[a-z0-9]{10,}$"
    r"|^(ghp|gho|ghs|github_pat|xox[baprs])[-_][a-z0-9-]{6,}$|^-----BEGIN"
    r"|^[a-z0-9/+_-]{32,}={0,2}$")

#: (dialect, pattern) pairs for code. Specific spellings only: a generic `$NAME` sweep across
#: every file would turn every shell script into an environment review, so the loose dialects
#: are enabled per file type and require an UPPER_CASE name.
REFERENCE_PATTERNS: tuple[tuple[str, re.Pattern], ...] = (
    ("python", re.compile(r"os\.environ(?:\.get)?\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("python", re.compile(r"os\.environ\[\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("python", re.compile(r"os\.getenv\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("python", re.compile(r"\benviron\.get\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("python", re.compile(r"\bgetenv\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("python", re.compile(r"\benv_(?:str|int|bool|float|json|list)\(\s*[\"']([A-Za-z_]\w*)")),
    ("python", re.compile(r"[ ,\(]env\s*[=:]?\s*[\"']([A-Z][A-Z0-9_]{2,})[\"']")),
    ("go", re.compile(r"os\.(?:Getenv|LookupEnv)\(\s*\"([A-Za-z_][A-Za-z0-9_]*)\"")),
    ("go", re.compile(r"BindEnv\(\s*\"[^\"]*\",\s*\"([A-Za-z_][A-Za-z0-9_]*)\"")),
    ("rust", re.compile(r"env::var(?:_os|_string|_path)?\(\s*\"([A-Za-z_][A-Za-z0-9_]*)\"")),
    ("rust", re.compile(r"env!\(\s*\"([A-Za-z_][A-Za-z0-9_]*)\"")),
    ("node", re.compile(r"process\.env\.([A-Za-z_][A-Za-z0-9_]*)")),
    ("node", re.compile(r"process\.env\[\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("node", re.compile(r"import\.meta\.env\.([A-Za-z_][A-Za-z0-9_]*)")),
    ("ruby", re.compile(r"ENV\[\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("ruby", re.compile(r"ENV\.fetch\(\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
    ("java", re.compile(r"System\.getenv\(\s*\"([A-Za-z_][A-Za-z0-9_]*)\"")),
    ("php", re.compile(r"\$_(?:ENV|SERVER)\[\s*[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']")),
)

#: Looser dialects, only where the file type says the shape means an environment variable.
SHELLISH = (re.compile(r"\$\{(?P<name>[A-Z][A-Z0-9_]{2,})(?P<op>:?[-=?+]?)(?P<default>[^}]*)\}"),)
SHELL_PATHS = ("**/*.sh", "**/*.bash", "**/*.zsh", "**/entrypoint*", "**/*.envrc")
K8S_SUBST = re.compile(r"\$\((?P<name>[A-Za-z_][A-Za-z0-9_]*)\)")
COMPOSE_SUBST = re.compile(r"\$\{(?P<name>[A-Za-z_][A-Za-z0-9_]*)(?P<op>:?[-=?+]?)(?P<default>[^}]*)\}")
SPRING_SUBST = re.compile(r"\$\{(?P<name>[A-Z][A-Z0-9_]{2,})(?::(?P<default>[^}]*))?\}")
TERRAFORM_REF = re.compile(r"\bvar\.(?P<name>[a-z][a-z0-9_]*)")

#: A value that is an indirection to a secret manager or another variable, not a value.
SECRET_INTERPOLATION = re.compile(r"^\s*(\$\{\{[^}]*secrets\.[^}]*\}\}|\$\{[^}]*\}|\$\([^)]*\)"
                                  r"|\$\{\{[^}]*env\.[^}]*\}\}|%\{[^}]*\})\s*$")

FALLBACK_TOKENS = ("??", "||", "unwrap_or", "unwrap_or_else", "GetOrDefault", ".get(",
                   "getenv", "fetch(", "setdefault", "LookupEnv", ":-", ":=", ":?")


@dataclass(frozen=True)
class Declaration:
    """One variable an operator has to provide."""

    name: str
    path: str
    line: int
    value: str = ""
    dialect: str = ""
    documentation: bool = False           # `.example`-style: a value here is documentation
    committed: bool = False               # a file where a value is a *real* value
    notes: list[str] = field(default_factory=list)

    def location(self) -> str:
        return "%s:%d" % (self.path, self.line)


@dataclass(frozen=True)
class Reference:
    """One place the code asks for a variable."""

    name: str
    path: str
    line: int
    dialect: str = ""
    defaulted: bool = False
    spell: str = ""

    def location(self) -> str:
        return "%s:%d" % (self.path, self.line)


# ---------------------------------------------------------------------------
# parsing one file
# ---------------------------------------------------------------------------
#: YAML keys that look like a variable name inside an `env:` block but are not one.
ENV_RESERVED_KEYS: tuple[str, ...] = ("env", "environment", "env_from", "name", "value",
                                      "value_from", "valuefrom", "config_map_key_ref",
                                      "configmapkeyref", "secret_key_ref", "secretkeyref",
                                      "config_map", "secret", "prefix")


def _matches_any(globs, path: str) -> bool:
    from recommender.config.globs import matches
    return any(matches(glob, path) for glob in globs)


def parse_declarations(path: str, text: str, policy) -> list[Declaration]:
    """Every variable this file *declares*, in the dialect its name and shape imply."""
    from . import yamltext as Y
    out: list[Declaration] = []
    if not text.strip():
        return out
    documentation = bool(policy.matches("declaration_globs", path))
    committed = bool(policy.matches("committed_globs", path))
    base = path.rsplit("/", 1)[-1]
    if documentation or committed or base.startswith(".env") or base.endswith(".env"):
        for lineno, raw in enumerate(text.splitlines(), 1):
            if raw.lstrip().startswith("#"):
                continue
            match = DOTENV_LINE.match(raw)
            if match:
                out.append(Declaration(name=match.group("name"), path=path, line=lineno,
                                       value=_clean(match.group("value")),
                                       dialect="dotenv", documentation=documentation,
                                       committed=committed))
        return out
    if base.lower().startswith(("dockerfile", "containerfile")) or path.endswith(
            ".dockerfile"):
        for lineno, raw in enumerate(text.splitlines(), 1):
            match = DOCKER_ENV.match(raw)
            if not match:
                continue
            for name, value in _docker_pairs(match.group("body")):
                out.append(Declaration(name=name, path=path, line=lineno, value=value,
                                       dialect="dockerfile"))
        return out
    if path.endswith((".tf", ".tfvars", ".tf.json", ".hcl")):
        for lineno, raw in enumerate(text.splitlines(), 1):
            match = TERRAFORM_VAR.match(raw)
            if match:
                out.append(Declaration(name=match.group("name"), path=path, line=lineno,
                                       value=_terraform_default(text, lineno),
                                       dialect="terraform"))
        return out
    if text.lstrip().startswith("{"):                 # JSON: keys are not declarations
        return out
    scan = Y.scan(text)
    dialect = _yaml_dialect(path)
    for node in scan.nodes:
        if not node.under("env", "environment", "env_from"):
            continue
        if SECRET_INTERPOLATION.match(node.value or ""):
            # `${{ secrets.X }}` is not a value: the operator fact is the secret, and `ci.py`
            # already reports it. Reporting the same line as an environment variable too is
            # two rows for one thing to do.
            continue
        if node.item and "=" in node.item:                     # compose list form `- FOO=v`
            name, _, value = node.item.partition("=")
            if _env_name_ok(name.strip()):
                out.append(Declaration(name=name.strip(), path=path, line=node.lineno,
                                       value=_clean(value), dialect=dialect))
            continue
        if node.key == "name" and (node.value or node.item):    # k8s form `- name: FOO`
            name = node.value or node.item
            if _env_name_ok(name):
                out.append(Declaration(name=name, path=path, line=node.lineno,
                                       value=_env_value_of(scan, node), dialect=dialect))
            continue
        if node.key and node.key.lower() not in ENV_RESERVED_KEYS and node.value \
                and _env_name_ok(node.key):                      # map form `FOO: value`
            out.append(Declaration(name=node.key, path=path, line=node.lineno,
                                   value=_clean(node.value), dialect=dialect))
    return out


def find_references(path: str, text: str, policy) -> list[Reference]:
    """Every environment read in one file, with the language that spelled it that way."""
    out: list[Reference] = []
    if not text.strip():
        return out
    shellish = _matches_any(SHELL_PATHS, path)
    terraform = path.endswith((".tf", ".tfvars", ".hcl"))
    spring = _is_spring(path)
    for lineno, raw in enumerate(text.splitlines(), 1):
        if raw.lstrip().startswith(("#", "//", "/*", "*")):
            continue
        for dialect, pattern in REFERENCE_PATTERNS:
            for match in pattern.finditer(raw):
                name = match.group(1)
                out.append(Reference(name=name, path=path, line=lineno, dialect=dialect,
                                     defaulted=_has_fallback(raw, match.end()),
                                     spell=match.group(0).strip()))
        if shellish:
            for match in SHELLISH[0].finditer(raw):
                out.append(Reference(name=match.group("name"), path=path, line=lineno,
                                     dialect="shell",
                                     defaulted=match.group("op") in (":-", ":=", ":?")
                                     or bool(match.group("default")),
                                     spell=match.group(0)))
        if terraform:
            for match in TERRAFORM_REF.finditer(raw):
                out.append(Reference(name=match.group("name"), path=path, line=lineno,
                                     dialect="terraform", defaulted=True,
                                     spell="var." + match.group("name")))
        if spring:
            for match in SPRING_SUBST.finditer(raw):
                out.append(Reference(name=match.group("name"), path=path, line=lineno,
                                     dialect="spring",
                                     defaulted=bool(match.group("default")),
                                     spell=match.group(0)))
        if path.endswith((".yml", ".yaml")):
            for match in K8S_SUBST.finditer(raw):
                out.append(Reference(name=match.group("name"), path=path, line=lineno,
                                     dialect="kubernetes", defaulted=False,
                                     spell=match.group(0)))
            if "compose" in path.rsplit("/", 1)[-1]:
                for match in COMPOSE_SUBST.finditer(raw):
                    name = match.group("name")
                    if _env_name_ok(name):
                        out.append(Reference(name=name, path=path, line=lineno,
                                             dialect="compose",
                                             defaulted=bool(match.group("default")),
                                             spell=match.group(0)))
    return _dedupe(out)


# ---------------------------------------------------------------------------
# the episode
# ---------------------------------------------------------------------------
@dataclass
class Survey:
    """The two sets, and what their difference means."""

    declared_before: dict[str, Declaration] = field(default_factory=dict)
    declared_after: dict[str, Declaration] = field(default_factory=dict)
    refs_before: dict[str, list[Reference]] = field(default_factory=dict)
    refs_after: dict[str, list[Reference]] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)
    repo_supplied: bool = False
    repo_asked: bool = False

    def added_declarations(self) -> list[Declaration]:
        return [decl for name, decl in sorted(self.declared_after.items())
                if name not in self.declared_before]

    def removed_declarations(self) -> list[Declaration]:
        return [decl for name, decl in sorted(self.declared_before.items())
                if name not in self.declared_after]

    def added_references(self) -> list[Reference]:
        return [refs[0] for name, refs in sorted(self.refs_after.items())
                if name not in self.refs_before]

    def references_to(self, name: str) -> list[Reference]:
        return list(self.refs_after.get(name, ()))

    @property
    def repo_scope(self) -> bool:
        return self.repo_supplied or self.repo_asked


def survey(run: Run) -> Survey:
    """Build the before/after declaration and reference sets for one episode."""
    state = Survey()
    policy = run.policy
    for facts in run.files:
        before, after = facts.before, facts.after
        if before is not None:
            for decl in parse_declarations(facts.path, before, policy):
                state.declared_before.setdefault(decl.name, decl)
            for ref in find_references(facts.path, before, policy):
                state.refs_before.setdefault(ref.name, []).append(ref)
        if after is not None:
            for decl in parse_declarations(facts.path, after, policy):
                state.declared_after.setdefault(decl.name, decl)
            for ref in find_references(facts.path, after, policy):
                state.refs_after.setdefault(ref.name, []).append(ref)
    state.repo_supplied = bool(run.episode.repo_env_refs)
    state.repo_asked = policy.text("reference_scope") == "repo"
    if state.repo_supplied:
        state.notes.append("the caller supplied %d repository-wide environment reference(s)"
                           % len(set(run.episode.repo_env_refs)))
    return state


def detect(run: Run) -> tuple[list[Finding], Survey]:
    """Every env finding for one episode, plus the survey that produced them (for `explain`)."""
    state = survey(run)
    findings: list[Finding] = []
    consumed: set[str] = set()

    renames = _find_renames(state, run)
    for old, new, similarity, stale in renames:
        if run.enabled("env-renamed"):
            findings.append(_rename_finding(old, new, similarity, stale, state, run))
        consumed.add(old.name)
        consumed.add(new.name)

    if run.enabled("env-added"):
        findings.extend(_added_findings(state, run, consumed))
    if run.enabled("env-declared-unread"):
        findings.extend(_unread_findings(state, run, consumed))
    if run.enabled("env-stale-reference"):
        findings.extend(_stale_findings(state, run, consumed))
    if run.enabled("env-removed"):
        findings.extend(_removed_findings(state, run, consumed))
    if run.enabled("env-value-committed"):
        findings.extend(_committed_findings(state, run))
    return findings, state


# -- individual rules -------------------------------------------------------
def _added_findings(state: Survey, run: Run, consumed: set[str]) -> list[Finding]:
    out: list[Finding] = []
    declared = {decl.name: decl for decl in state.added_declarations()}
    referenced = {ref.name: ref for ref in state.added_references()}
    for name in sorted(set(declared) | set(referenced)):
        if name in consumed:
            continue
        decl = declared.get(name)
        ref = referenced.get(name)
        anchor = decl or ref
        secret = run.policy.is_secret_name(name)
        evidence: list[str] = []
        if decl:
            evidence.append("declared at %s (%s%s)"
                            % (decl.location(), decl.dialect,
                               ", documentation only" if decl.documentation else ""))
        if ref:
            evidence.append("first read at %s via `%s`%s"
                            % (ref.location(), ref.spell,
                               "" if ref.defaulted else ", with no fallback at the call site"))
        other = [item for item in state.references_to(name)[1:4]]
        evidence.extend("also read at %s (%s)" % (item.location(), item.dialect)
                        for item in other)
        if not decl:
            evidence.append("no declaration file in this episode mentions it")
        detail = "new variable %s" % name
        if decl and decl.value and not decl.documentation:
            detail += " with a value committed (%s)" % _mask(decl.value, secret)
        if ref and not ref.defaulted:
            detail += "; the read has no default, so an unset value fails at boot or on use"
        action = "set %s before deploy" % name
        if secret:
            action = "put %s in the secret store and set it before deploy" % name
        if not decl:
            action += ", or declare it in the env file"
        out.append(Finding(kind="env-added", name=name, path=anchor.path, line=anchor.line,
                           action=action, detail=detail,
                           value=_mask(decl.value, secret) if decl else "",
                           evidence=evidence, dialect=anchor.dialect,
                           confidence=0.9 if decl else 0.8))
    return out


def _unread_findings(state: Survey, run: Run, consumed: set[str]) -> list[Finding]:
    out: list[Finding] = []
    repo_refs = set(run.episode.repo_env_refs or ())
    for decl in state.added_declarations():
        if decl.name in consumed:
            continue
        if state.references_to(decl.name):
            continue                                   # read somewhere in the episode
        if decl.name in repo_refs:
            continue                                   # the repository half answers it
        scope = run.policy.text("reference_scope")
        if scope == "repo" and not repo_refs:
            confidence = CONFIDENCE_EPISODE
            note = ("non_code.env.reference_scope is `repo`, but the caller supplied no "
                    "repository reference set, so this claim is only about this episode")
        else:
            confidence = CONFIDENCE_REPO if repo_refs else CONFIDENCE_EPISODE
            note = ("basis: %s" % ("the repository reference set the caller supplied"
                                   if repo_refs else
                                   "the %d file(s) in this episode only" % len(state.refs_after)))
        out.append(Finding(
            kind="env-declared-unread", name=decl.name, path=decl.path, line=decl.line,
            detail="declared in %s (%s) with no read found" % (decl.path, decl.dialect),
            action="confirm the code that reads %s exists before the operator sets it"
                   % decl.name,
            evidence=[note] + (["declared value %s"
                               % _mask(decl.value, run.policy.is_secret_name(decl.name))]
                              if decl.value else []),
            dialect=decl.dialect, confidence=confidence))
    return out


def _stale_findings(state: Survey, run: Run, consumed: set[str]) -> list[Finding]:
    out: list[Finding] = []
    for decl in state.removed_declarations():
        if decl.name in consumed:
            continue
        refs = state.references_to(decl.name)
        if not refs:
            continue
        out.append(Finding(
            kind="env-stale-reference", name=decl.name, path=refs[0].path, line=refs[0].line,
            old_path=decl.path, old_line=decl.line,
            detail="declaration removed from %s while %d reference(s) remain"
                   % (decl.path, len(refs)),
            action="repoint the reference to %s or restore the variable" % decl.name,
            evidence=["read at %s via `%s`" % (ref.location(), ref.spell) for ref in refs[:4]],
            dialect=refs[0].dialect, confidence=CONFIDENCE_REFERENCE))
    return out


def _removed_findings(state: Survey, run: Run, consumed: set[str]) -> list[Finding]:
    out: list[Finding] = []
    for decl in state.removed_declarations():
        if decl.name in consumed or state.references_to(decl.name):
            continue
        out.append(Finding(
            kind="env-removed", name=decl.name, path=decl.path, line=decl.line,
            detail="no longer declared or read anywhere in this episode",
            action="retire %s from the environment and the secret store" % decl.name,
            evidence=["last declared at %s (%s)" % (decl.location(), decl.dialect)],
            dialect=decl.dialect, confidence=0.85))
    return out


def _committed_findings(state: Survey, run: Run) -> list[Finding]:
    out: list[Finding] = []
    placeholders = {str(item).strip().lower() for item in run.policy.words("placeholder_values")}
    for decl in sorted(state.declared_after.values(),
                       key=lambda item: (item.path, item.line)):
        if not decl.committed or decl.documentation or decl.value.strip() in ("",):
            continue
        value = decl.value.strip().strip("'\"")
        if not value or value.lower() in placeholders:
            continue
        shaped = bool(CREDENTIAL_SHAPED.match(value))
        if not (shaped or run.policy.is_secret_name(decl.name)):
            continue
        out.append(Finding(
            kind="env-value-committed", name=decl.name, path=decl.path, line=decl.line,
            detail="%s has a real-looking value in a committed file (%s)"
                   % (decl.name, _mask(value)),
            action="rotate %s and move it to the secret store; the value is in the history now"
                   % decl.name,
            evidence=["matched %s" % ("a credential shape" if shaped else
                                      "`non_code.env.secret_name_pattern`"),
                      "declared in %s (%s)" % (decl.path, decl.dialect)],
            value=_mask(value), dialect=decl.dialect, confidence=CONFIDENCE_VALUE))
    return out


def _find_renames(state: Survey, run: Run) -> list[tuple[Declaration, Declaration, float,
                                                         list[Reference]]]:
    """Pair removed and added variables that are one rename, best match first.

    Greedy on similarity: the closest pair is taken, both names are consumed, and the next
    pair is considered. Two removals and two additions that are two independent renames get
    two rows; an unrelated pair that merely rhymes falls under `rename_similarity` and stays
    two rows too, which is the honest answer.
    """
    removed = [decl for decl in state.removed_declarations()]
    added = [decl for decl in state.added_declarations()]
    threshold = run.policy.number("rename_similarity")
    candidates: list[tuple[float, Declaration, Declaration]] = []
    for old in removed:
        for new in added:
            similarity = name_similarity(old.name, new.name)
            if similarity >= threshold:
                candidates.append((similarity, old, new))
    candidates.sort(key=lambda item: (-item[0], item[1].name, item[2].name))
    taken_old: set[str] = set()
    taken_new: set[str] = set()
    out: list[tuple[Declaration, Declaration, float, list[Reference]]] = []
    for similarity, old, new in candidates:
        if old.name in taken_old or new.name in taken_new:
            continue
        taken_old.add(old.name)
        taken_new.add(new.name)
        out.append((old, new, similarity, state.references_to(old.name)))
    return out


def _rename_finding(old: Declaration, new: Declaration, similarity: float,
                    stale: list[Reference], state: Survey, run: Run) -> Finding:
    evidence = ["removed at %s (%s)" % (old.location(), old.dialect),
                "added at %s (%s)" % (new.location(), new.dialect),
                "name similarity %.2f ≥ non_code.env.rename_similarity = %s"
                % (similarity, run.policy.number("rename_similarity"))]
    if stale:
        evidence.extend("still read at %s via `%s`" % (ref.location(), ref.spell)
                        for ref in stale[:4])
        detail = ("`%s` became `%s` and %d reference(s) still use the old name"
                  % (old.name, new.name, len(stale)))
        action = "repoint the remaining reads of %s to %s and set %s in the environment" \
                 % (old.name, new.name, new.name)
    else:
        detail = "`%s` became `%s` with no reference to the old name left in this episode" \
                 % (old.name, new.name)
        action = "set %s in every environment and retire %s" % (new.name, old.name)
    return Finding(kind="env-renamed", name="%s → %s" % (old.name, new.name),
                   path=new.path, line=new.line, old_path=old.path, old_line=old.line,
                   detail=detail, action=action, evidence=evidence, dialect=new.dialect,
                   confidence=CONFIDENCE_RENAME)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def name_similarity(left: str, right: str) -> float:
    """Token similarity between two variable names, `0.0`–`1.0`.

    Tokens split on `_`, `-`, `.` and camelCase humps. The score is the token Jaccard,
    except that when one name's tokens are a subset of the other's the score is the
    containment — `PG_DATABASE_URL` contains `DATABASE_URL`, which is what a prefixed rename
    looks like, and a plain Jaccard would call it 0.67 and need a knob to notice.
    """
    if not left or not right:
        return 0.0
    if left == right:
        return 1.0
    left_tokens, right_tokens = _tokens(left), _tokens(right)
    if not left_tokens or not right_tokens:
        return 0.0
    shared = left_tokens & right_tokens
    jaccard = len(shared) / len(left_tokens | right_tokens)
    containment = len(shared) / min(len(left_tokens), len(right_tokens))
    return max(jaccard, containment if shared else 0.0)


def _tokens(name: str) -> set[str]:
    parts = re.split(r"[_\-.]+", name)
    words: list[str] = []
    for part in parts:
        words.extend(word for word in re.findall(r"[A-Z]+(?![a-z])|[A-Z]?[a-z0-9]+|\d+", part))
    return {word.lower() for word in words if word}


def _dedupe(refs: list[Reference]) -> list[Reference]:
    seen: set[tuple] = set()
    out: list[Reference] = []
    for ref in refs:
        key = (ref.name, ref.path, ref.line, ref.dialect)
        if key in seen:
            continue
        seen.add(key)
        out.append(ref)
    return out


def _has_fallback(line: str, after: int) -> bool:
    tail = line[after:]
    if tail.lstrip().startswith((")", "]")):
        return False
    return any(token in tail for token in FALLBACK_TOKENS)


def _clean(value: str) -> str:
    value = (value or "").strip()
    if len(value) >= 2 and value[0] in "\"'" and value[-1] == value[0]:
        return value[1:-1]
    if " #" in value:
        value = value.split(" #", 1)[0].strip()
    return value


def _mask(value: str, secret: bool = False) -> str:
    """Show a value without reproducing a credential in a card, an export or a log.

    Masking everything is as unhelpful as printing a token: `false` and `postgres://…` are
    the information the operator needs, and `sk-live-…` is the information nobody needs
    twice. So the shape of the *name* decides: a name that matches
    `non_code.env.secret_name_pattern` is masked whatever its length, and an ordinary short
    value is printed as it is.
    """
    value = (value or "").strip()
    if not value:
        return ""
    if value.startswith(("${", "$(", "{{", "%{")):
        return value                             # an interpolation is a reference, not a secret
    if not secret and len(value) <= 12:
        return value
    if len(value) <= 4:
        return "•" * len(value)
    if len(value) <= 10:
        return "%s…%s" % (value[:2], value[-2:])
    return "%s…%s (%d chars)" % (value[:2], value[-2:], len(value))


def _docker_pairs(body: str) -> list[tuple[str, str]]:
    body = body.strip()
    pairs: list[tuple[str, str]] = []
    if "=" in body.split()[0]:
        for token in body.split():
            name, _, value = token.partition("=")
            if re.match(r"^[A-Za-z_][A-Za-z0-9_]*$", name):
                pairs.append((name, _clean(value)))
        return pairs
    parts = body.split(None, 1)
    if parts and re.match(r"^[A-Za-z_][A-Za-z0-9_]*$", parts[0]):
        pairs.append((parts[0], _clean(parts[1]) if len(parts) > 1 else ""))
    return pairs


def _terraform_default(text: str, start: int) -> str:
    lines = text.splitlines()
    for offset in range(1, 24):
        if start + offset > len(lines):
            break
        line = lines[start + offset - 1]
        if re.match(r"^\s*default\s*=", line):
            return line.split("=", 1)[1].strip().strip('"')
        if re.match(r"^\s*\}", line):
            break
    return ""


def _env_name_ok(name: str) -> bool:
    """Does this look like a variable name rather than a container or object name?"""
    return bool(name) and bool(re.match(r"^[A-Za-z_][A-Za-z0-9_.]*$", name))


def _env_value_of(scan, node) -> str:
    """A Kubernetes/compose `name:` item's value: the sibling `value:`, if it has one."""
    nodes = scan.nodes
    try:
        index = nodes.index(node)
    except ValueError:
        return ""
    for other in nodes[index + 1:index + 4]:
        if other.key.lower() in ("value", "valuefrom", "value_from", "default"):
            return _clean(other.value)
        if other.key.lower() in ("name", ""):
            break
    return ""


def _yaml_dialect(path: str) -> str:
    if "compose" in path or path.endswith(("compose.yml", "compose.yaml")):
        return "compose"
    if "workflow" in path or ".github" in path:
        return "github-actions"
    if "k8s" in path or "manifest" in path or "kubernetes" in path:
        return "kubernetes"
    return "yaml"


def _is_spring(path: str) -> bool:
    base = path.rsplit("/", 1)[-1]
    return base.startswith("application") and base.endswith((".yml", ".yaml", ".properties"))
