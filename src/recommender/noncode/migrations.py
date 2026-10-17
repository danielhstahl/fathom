"""Database migrations: the surface where the deploy has an order (`e2.8` AC 3).

Everything else on this ticket is a value somebody has to set. A migration is a *program*
that runs once, against a live database, in a order the release has to get right, and three
properties of it decide whether the release is boring:

**Is it reversible?** Each tool spells "the other way" differently, and the ways are few
enough to enumerate, so this module looks for the marker rather than guessing from the file
extension:

| tool | forward marker | down marker | verdict |
| --- | --- | --- | --- |
| golang-migrate | `0001_x.up.sql` | `0001_x.down.sql` | reversible when the down file exists |
| goose | `-- +goose Up` | `-- +goose Down` | reversible when the Down section has statements |
| sql-migrate | `-- +migrate Up` | `-- +migrate Down` | ditto |
| Flyway | `V1__x.sql` | `U1__x.sql` undo script | reversible when the undo is in the episode |
| Rails | `def up` / `def change` | `def down`, or `def change` with no irreversible step | `change` is reversible unless it contains `execute`/`remove_column` |
| Alembic | `def upgrade()` | `def downgrade()` with something in it | a `downgrade` whose body is `pass` is a comment, not a rollback |
| Django | `operations = [ … ]` | `reverse`-capable operations | a `RunSQL` without `reverse_sql=` is forward-only, stated as such |
| knex / Sequelize | `exports.up` / `up()` | `exports.down` / `down()` | absent down means forward-only |
| Prisma | `migration.sql` | — | always forward-only: the CLI regenerates, it does not roll back |

**Does it destroy anything?** `DROP`, `TRUNCATE`, `SET NOT NULL`, a type change, a `RENAME`,
a bare `UPDATE` over a table. Reversible and safe are different questions — Rails can roll a
`remove_column` back and still lose every value in it — so a reversible migration can still
raise the destructive finding, and does.

**What else is in the episode?** A migration beside a model change is the normal shape of a
schema change and also an ordering problem, so it gets a row with the deploy order spelled
out (`expand, migrate, contract`). A migration with *nothing* else is worse, not better:
there is no code here that is compatible with either side of it, which is the explicit
**"needs deploy coordination"** warning the acceptance criteria ask for and this module
prints in those words.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from .policy import Finding
from .surfaces import Run

#: Filename shapes that carry ordering or direction.
NUMBERED = re.compile(r"(?P<order>\d{6,}|\d{1,4}_\d{2}_\d{2}_?\d{4,6}|\d+\.\d+)")
FLYWAY = re.compile(r"^V(?P<version>[\d._]+)__(?P<title>.+)\.sql$")
FLYWAY_UNDO = re.compile(r"^U(?P<version>[\d._]+)__(?P<title>.+)\.sql$")
GOOSE_DIALECT = re.compile(r"(?i)goose|sql-migrate|\+\s*migrate")
ALEMBIC_REVISION = re.compile(r"""^\s*revision\s*(?::\s*\w+\s*)?=\s*['"](?P<rev>[^'"]+)['"]""")
ALEMBIC_DOWN = re.compile(r"""^\s*down_revision\s*(?::[^=]+)?=\s*(?P<value>['"][^'"]+['"]|None)""")

#: Statements that destroy or rewrite data when they appear in a forward path.
DESTRUCTIVE_FALLBACK: tuple[str, ...] = (
    "drop table", "drop column", "drop index", "drop constraint", "truncate",
    "set not null", "alter column", "modify column", "rename to", "delete from",
    "remove_column", "remove_index", "change_column_null", "execute ",
)

IRREVERSIBLE_RAILS = re.compile(r"\b(execute|remove_column|remove_index|drop_table)\b")


@dataclass(frozen=True)
class Statement:
    """One matched piece of SQL or code, with the line a reviewer can jump to."""

    line: int
    text: str
    matched: tuple[str, ...]

    @property
    def op(self) -> str:
        return self.matched[0]

    def render(self) -> str:
        return "%s:%d `%s`" % (self.op, self.line, self.text.strip()[:80])


@dataclass
class Migration:
    """One migration file, as far as this reader can tell."""

    path: str
    dialect: str = "sql"
    revision: str = ""
    order: str = ""
    title: str = ""
    has_down: bool = False
    down_evidence: str = ""
    forward_only: bool = False
    forward_only_reason: str = ""
    destructive: list[Statement] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    statements: int = 0
    deleted: bool = False

    @property
    def identity(self) -> str:
        if self.revision:
            return self.revision
        if self.order:
            return self.order
        return self.path.rsplit("/", 1)[-1]

    @property
    def reversibility(self) -> str:
        if self.has_down:
            return "reversible"
        if self.forward_only:
            return "forward-only"
        return "unknown"

    def as_dict(self) -> dict:
        return {"path": self.path, "dialect": self.dialect, "revision": self.revision,
                "reversibility": self.reversibility, "down_evidence": self.down_evidence,
                "forward_only_reason": self.forward_only_reason,
                "destructive": [{"line": item.line, "matched": item.matched[0],
                                 "text": item.text.strip()[:90]}
                                for item in self.destructive],
                "statements": self.statements, "deleted": self.deleted,
                "notes": list(self.notes)}


# ---------------------------------------------------------------------------
# one file
# ---------------------------------------------------------------------------
def inspect(path: str, text: str | None, *, deleted: bool = False,
            ops: tuple[str, ...] | None = None) -> Migration:
    """Read one migration file: dialect, direction markers, destructive statements."""
    migration = Migration(path=path, deleted=deleted)
    base = path.rsplit("/", 1)[-1]
    migration.dialect = _dialect(path)
    body = text or ""
    match = FLYWAY.match(base)
    if match:
        migration.revision = match.group("version")
        migration.title = match.group("title")
    if FLYWAY_UNDO.match(base):
        migration.revision = FLYWAY_UNDO.match(base).group("version")
        migration.title = "undo " + FLYWAY_UNDO.match(base).group("title")
    numbered = NUMBERED.search(base)
    if numbered:
        migration.order = numbered.group("order")
    lines = body.splitlines()
    lowered = [line.lower() for line in lines]

    if migration.dialect == "sql":
        _inspect_sql(migration, lines, lowered)
    elif migration.dialect == "python":
        _inspect_python(migration, lines, lowered)
    elif migration.dialect == "ruby":
        _inspect_ruby(migration, lines, lowered)
    elif migration.dialect in ("javascript", "typescript"):
        _inspect_js(migration, lines, lowered)
    if migration.dialect == "prisma" and not migration.has_down:
        migration.forward_only = True
        migration.forward_only_reason = ("Prisma migrations are generated SQL the CLI applies "
                                         "forward; there is no down path to write")
    _destructive(migration, lines, lowered, ops or DESTRUCTIVE_FALLBACK)
    migration.statements = sum(1 for line in lines if line.strip()
                               and not line.strip().startswith(("--", "#", "//", "*")))
    if not migration.has_down and not migration.forward_only:
        migration.forward_only = True
        if not migration.forward_only_reason:
            migration.forward_only_reason = ("no down marker for %s was found in this file or "
                                             "this episode" % migration.dialect)
    return migration


def _inspect_sql(migration: Migration, lines: list[str], lowered: list[str]) -> None:
    joined = "\n".join(lowered)
    if "-- +goose down" in joined or "-- +migrate down" in joined:
        index = next((i for i, line in enumerate(lowered)
                      if "+goose down" in line or "+migrate down" in line), None)
        body = [line for line in lowered[index + 1:] if line.strip()
                and not line.strip().startswith("--")] if index is not None else []
        if body:
            migration.has_down = True
            migration.down_evidence = "`%s` with %d statement line(s)" % (
                "goose Down" if "+goose down" in joined else "migrate Down", len(body))
        else:
            migration.forward_only_reason = "the Down section is present but empty"
    if migration.path.endswith((".down.sql", ".rollback.sql")):
        migration.has_down = True
        migration.down_evidence = "a `%s` down file accompanies the up file" \
                                  % migration.path.rsplit(".", 2)[-2]


def _inspect_python(migration: Migration, lines: list[str], lowered: list[str]) -> None:
    joined = "\n".join(lowered)
    if "class migration" in joined:
        migration.dialect = "django"
    elif "revision" in joined and ("def upgrade" in joined or "op." in joined):
        migration.dialect = "alembic"
    for lineno, raw in enumerate(lines, 1):
        match = ALEMBIC_REVISION.match(raw)
        if match:
            migration.revision = match.group("rev")
        down = ALEMBIC_DOWN.match(raw)
        if down and down.group("value") != "None":
            migration.order = down.group("value").strip("'\"")
    if migration.dialect == "alembic":
        index = next((i for i, line in enumerate(lowered) if "def downgrade" in line), None)
        if index is None:
            migration.forward_only_reason = "`def downgrade()` is missing entirely"
        else:
            body = [line for line in lines[index + 1:]
                    if line.strip() and not line.strip().startswith("#")]
            meaningful = [line for line in body if line.strip().lower() != "pass"]
            if meaningful:
                migration.has_down = True
                migration.down_evidence = "`def downgrade()` with %d line(s)" % len(meaningful)
            else:
                migration.forward_only_reason = "`def downgrade()` exists but only says `pass`"
        if "down_revision" not in joined:
            migration.notes.append("no `down_revision`: this revision does not attach to the "
                                   "chain, so `alembic upgrade head` may not pick it up")
    if migration.dialect == "django":
        if re.search(r"reverse_sql\s*=\s*['\"]", "\n".join(lines)):
            migration.has_down = True
            migration.down_evidence = "`RunSQL(..., reverse_sql=…)` supplies the reverse"
        elif re.search(r"runsql\(", "\n".join(lowered)):
            migration.forward_only = True
            migration.forward_only_reason = ("Django reverses the operations it knows; this "
                                             "`RunSQL` has no `reverse_sql=`, so it has no "
                                             "reverse")
        else:
            migration.has_down = True
            migration.down_evidence = ("Django `operations` are reversible as written (no "
                                       "`RunSQL`/custom step)")


def _inspect_ruby(migration: Migration, lines: list[str], lowered: list[str]) -> None:
    joined = "\n".join(lowered)
    if "def change" in joined:
        if IRREVERSIBLE_RAILS.search(joined):
            migration.forward_only = True
            migration.forward_only_reason = ("`def change` contains an irreversible step "
                                             "(`execute`/`remove_column`/`drop_table`), so "
                                             "Rails refuses to roll it back")
        else:
            migration.has_down = True
            migration.down_evidence = "`def change` — Rails knows the inverse of every step"
    if "def down" in joined or "def self.down" in joined:
        migration.has_down = True
        migration.down_evidence = "`def down` is written out"
    if "def up" in joined and not migration.has_down:
        migration.forward_only_reason = "`def up` is written and there is no `def down`"


def _inspect_js(migration: Migration, lines: list[str], lowered: list[str]) -> None:
    joined = "\n".join(lowered)
    if "exports.down" in joined or "down:" in joined or "exports.down = " in joined:
        migration.has_down = True
        migration.down_evidence = "`exports.down` is written"
    if "exports.up" in joined and not migration.has_down:
        migration.forward_only_reason = "`exports.up` exists with no `exports.down`"


def _destructive(migration: Migration, lines: list[str], lowered: list[str],
                 ops: tuple[str, ...]) -> None:
    """Match destructive statements against statement text, not the whole file.

    Comments are skipped: a migration that says `-- we could drop this table later` has not
    dropped anything, and a finding about it trains the reviewer to ignore the finding.
    """
    for lineno, raw, low in _forward_path(lines, lowered, migration.dialect):
        stripped = low.strip()
        if not stripped or stripped.startswith(("--", "#", "//", "*")):
            continue
        for op in ops:
            if op and op.lower() in low:
                migration.destructive.append(Statement(line=lineno, text=raw.strip(),
                                                       matched=(op.lower(),)))
                break


DOWN_MARKERS = ("-- +goose down", "-- +migrate down", "def downgrade", "exports.down",
                "def down")


def _forward_path(lines: list[str], lowered: list[str], dialect: str):
    """The lines that run *forward*.

    A `DROP INDEX` in a `-- +goose Down` section is the rollback, and a `remove_column` in
    `def downgrade()` is the undo: reporting either as destructive would be reporting the
    thing that makes the migration safe as the thing that makes it dangerous.
    """
    out: list[tuple[int, str, str]] = []
    in_forward = True
    for lineno, (raw, low) in enumerate(zip(lines, lowered), 1):
        if any(marker in low for marker in DOWN_MARKERS):
            in_forward = False
            continue
        if in_forward:
            out.append((lineno, raw, low))
    return out


def _dialect(path: str) -> str:
    base = path.rsplit("/", 1)[-1].lower()
    if "prisma" in path:
        return "prisma"
    if base.endswith((".sql", ".up")):
        return "sql"
    if base.endswith(".py"):
        return "python"
    if base.endswith(".rb"):
        return "ruby"
    if base.endswith((".js", ".ts", "mjs", "cjs")):
        return "javascript"
    if base.endswith(".php"):
        return "php"
    if base.endswith(".up"):
        return "sql"
    return "unknown"


# ---------------------------------------------------------------------------
# the episode
# ---------------------------------------------------------------------------
@dataclass
class Survey:
    migrations: list[Migration] = field(default_factory=list)
    model_files: list[str] = field(default_factory=list)
    code_files: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    @property
    def migration_only(self) -> bool:
        return bool(self.migrations) and not self.code_files

    def counts(self) -> dict[str, int]:
        return {"migrations": len(self.migrations),
                "forward_only": sum(1 for item in self.migrations if item.forward_only),
                "reversible": sum(1 for item in self.migrations if item.has_down),
                "destructive": sum(len(item.destructive) for item in self.migrations)}


def survey(run: Run) -> Survey:
    from recommender.taxonomy import classes as C
    state = Survey()
    for facts in run.files:
        if facts.on("migration"):
            text = facts.after if facts.after is not None else facts.before
            state.migrations.append(inspect(facts.path, text, deleted=facts.is_deleted,
                                            ops=_ops(run)))
        if run.policy.matches("model_globs", facts.path):
            state.model_files.append(facts.path)
        if facts.cls in (C.PathClass.SOURCE, C.PathClass.TEST):
            state.code_files.append(facts.path)
    return state


def _ops(run: Run) -> tuple[str, ...]:
    return tuple(item.strip().lower() for item in run.policy.words("destructive_ops")
                 if item.strip())


def detect(run: Run) -> tuple[list[Finding], Survey]:
    state = survey(run)
    findings: list[Finding] = []
    if not state.migrations:
        return findings, state
    _cross_file(state)
    for migration in state.migrations:
        if migration.deleted:
            continue
        if run.enabled("migration-added"):
            findings.append(_added_finding(migration, state))
        if run.enabled("migration-forward-only") and migration.forward_only:
            findings.append(_forward_only_finding(migration))
        if run.enabled("migration-destructive"):
            findings.extend(_destructive_findings(migration, run))
    if run.enabled("migration-with-model") and state.model_files:
        findings.append(_with_model_finding(state))
    if run.enabled("deploy-coordination") and state.migration_only:
        findings.append(_coordination_finding(state, run))
    return findings, state


def _cross_file(state: Survey) -> None:
    """Two migrations that collide, or a down file that answers an up file."""
    by_dir: dict[str, list[Migration]] = {}
    for migration in state.migrations:
        by_dir.setdefault(migration.path.rsplit("/", 1)[0], []).append(migration)
    for directory, group in sorted(by_dir.items()):
        seen: dict[str, Migration] = {}
        for migration in group:
            key = migration.revision or migration.order
            if not key:
                continue
            if key in seen and seen[key].path != migration.path:
                migration.notes.append(
                    "revision %s is also declared by %s: the two will fight over the ordering "
                    "column, and which one runs second is whichever the runner sorts last"
                    % (key, seen[key].path))
            else:
                seen[key] = migration
        ups = {item.path.rsplit("/", 1)[-1].replace(".up.sql", ""): item
               for item in group if item.path.endswith(".up.sql")}
        downs = {item.path.rsplit("/", 1)[-1].replace(".down.sql", "")
                 for item in group if item.path.endswith(".down.sql")}
        for stem, item in sorted(ups.items()):
            if stem in downs and not item.has_down:
                item.has_down = True
                item.down_evidence = "the companion `%s.down.sql` is in this episode" % stem
        for item in group:
            if item.path.endswith((".up.sql", ".sql")) and FLYWAY.match(
                    item.path.rsplit("/", 1)[-1]):
                undo = "U" + item.path.rsplit("/", 1)[-1][1:]
                if any(other.path.endswith(undo) for other in state.migrations):
                    item.has_down = True
                    item.down_evidence = "Flyway undo script `%s` is in this episode" % undo


def _added_finding(migration: Migration, state: Survey) -> Finding:
    evidence = ["dialect %s, reversibility %s" % (migration.dialect,
                                                  migration.reversibility),
                "%d non-blank line(s)" % migration.statements]
    evidence.extend(migration.notes)
    if migration.revision:
        evidence.append("revision `%s`" % migration.revision)
    destructive = len(migration.destructive)
    detail = "new %s migration (%s)" % (migration.dialect, migration.identity)
    if destructive:
        detail += ", with %d destructive statement(s) in the forward path" % destructive
    return Finding(kind="migration-added", name=migration.identity, path=migration.path,
                   line=1, detail=detail, value=migration.reversibility,
                   action="run this migration before the code that needs it; say who runs it",
                   evidence=evidence, dialect=migration.dialect, confidence=0.95)


def _forward_only_finding(migration: Migration) -> Finding:
    reason = migration.forward_only_reason or ("no down path found for %s"
                                               % migration.dialect)
    evidence = [reason]
    if migration.down_evidence:
        evidence.append(migration.down_evidence)
    return Finding(kind="migration-forward-only", name=migration.identity,
                   path=migration.path, line=1,
                   detail="no reversible path: %s" % reason,
                   action="write the down, or record that rolling this release back is a "
                          "manual repair",
                   value="forward-only", evidence=evidence, dialect=migration.dialect,
                   confidence=0.85)


def _destructive_findings(migration: Migration, run: Run) -> list[Finding]:
    out: list[Finding] = []
    statements = [item.strip().lower() for item in run.policy.words("destructive_ops")]
    for statement in migration.destructive:
        matched = statement.op
        if matched not in statements:
            continue                             # a repository narrowed its own list
        out.append(Finding(
            kind="migration-destructive", name="%s in %s" % (matched.upper(),
                                                             migration.identity),
            path=migration.path, line=statement.line,
            detail="`%s` in the forward path of a %s migration"
                   % (statement.text.strip()[:90], migration.dialect),
            action="check the lock this takes and the rows it touches before running it",
            evidence=["migration %s (%s)" % (migration.identity, migration.dialect),
                      "matched non_code.migration.destructive_ops = %s" % matched],
            dialect=migration.dialect, confidence=0.9))
    return out


def _with_model_finding(state: Survey) -> Finding:
    first = state.migrations[0]
    evidence = ["model files: %s" % ", ".join(state.model_files[:6])]
    evidence.extend("migration: %s" % item.path for item in state.migrations[:4])
    return Finding(
        kind="migration-with-model",
        name="%s + %s" % (first.identity, state.model_files[0].rsplit("/", 1)[-1]),
        path=first.path, line=1,
        detail="%d migration(s) and %d model/schema file(s) in one episode"
               % (len(state.migrations), len(state.model_files)),
        action="sequence the deploy: expand the schema, migrate, then ship the code that "
               "assumes it",
        evidence=evidence, confidence=0.9)


def _coordination_finding(state: Survey, run: Run) -> Finding:
    first = state.migrations[0]
    evidence = ["episode files: %s" % ", ".join(item.path for item in state.migrations[:6]),
                "no source or test change in this episode"]
    evidence.append("counts: %s" % ", ".join("%s=%d" % pair for pair in
                                              sorted(state.counts().items())))
    return Finding(
        kind="deploy-coordination",
        name="migration-only episode", path=first.path, line=1,
        detail="this episode changes durable state and nothing else, so it needs deploy "
               "coordination: the database will be ahead of the running code until somebody "
               "ships the other half",
        action="needs deploy coordination — decide the order, the window and who runs it",
        evidence=evidence, confidence=0.95)
