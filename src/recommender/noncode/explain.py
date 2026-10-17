"""The prose half: `fathom noncode explain`, the surface table, and the demo card.

Same engine as the card, different question. The card answers "what does this episode change";
`explain` answers the two questions a reviewer asks when they do not believe it:

* **"why does it think this file is a non-code surface, and which one?"** — the taxonomy class
  `e2.1` gave it, every reason a surface was claimed (a path table, a `fathom.toml` glob, or
  the content of the file), and what the surface's own reader extracted: the declarations and
  references, the migration's reversibility verdict, the workflow's secrets and triggers, the
  manifest's resources, the Dockerfile's stages.
* **"why did it not flag my change?"** — the finding table, the knob values in force, and the
  layer that won each one, printed from the running policy rather than from this paragraph.

The silence is the part that has to be printed. A tool that only explains its findings teaches
a reviewer to trust the ones that appear and shrug at the ones that do not; `explain` therefore
shows the parsed facts behind a *non*-firing surface too, and the checklist says when it is
empty because nothing was there rather than nothing was readable.
"""

from __future__ import annotations

from . import ci as CI
from . import container as CONTAINER
from . import env as ENV
from . import flags as FLAGS
from . import iac as IAC
from . import migrations as MIGRATIONS
from .policy import RULES, Policy, describe_rules
from .surfaces import Run

#: Which reader to run for which surface, for `explain`.
READERS = {
    "env": lambda facts, policy: {
        "declarations": [{"name": item.name, "line": item.line, "dialect": item.dialect,
                          "documentation": item.documentation, "value": _mask(item.value)}
                         for item in ENV.parse_declarations(facts.path, facts.after or "",
                                                            policy)],
        "references": [{"name": item.name, "line": item.line, "dialect": item.dialect,
                        "defaulted": item.defaulted}
                       for item in ENV.find_references(facts.path, facts.after or "", policy)]},
    "flag": lambda facts, policy: _flag_view(facts, policy),
    "migration": lambda facts, policy: MIGRATIONS.inspect(
        facts.path, facts.after if facts.after is not None else facts.before).as_dict(),
    "ci": lambda facts, policy: (CI.parse(facts.path, facts.after) or
                                 CI.Workflow(path=facts.path)).as_dict(),
    "iac": lambda facts, policy: (IAC.parse(facts.path, facts.after) or
                                  IAC.Manifest(path=facts.path)).as_dict(),
    "container": lambda facts, policy: (CONTAINER.parse(facts.path, facts.after) or
                                        CONTAINER.ImageFile(path=facts.path)).as_dict(),
}


def explain_file(path: str, before: str | None, after: str | None, *,
                 policy: Policy | None = None) -> list[str]:
    """One file pair: what it is, which surfaces claim it, and what each reader found."""
    from .surfaces import Change, Episode
    policy = policy or Policy.built_in()
    run = Run(Episode(id="", files=(Change.from_pair(path, before, after),)), policy)
    facts = run.files[0]
    lines: list[str] = [facts.path]
    lines.append("  class %s (confidence %.2f) · change %s"
                 % (facts.cls, facts.class_confidence, facts.change_type))
    lines.append("  why: %s" % (facts.class_reason or "no classifier reason recorded"))
    if facts.declared_non_code:
        lines.append("  %s" % facts.declared_non_code)
    if facts.surfaces:
        lines.append("  surfaces: %s" % ", ".join(facts.surfaces))
        for reason in facts.reasons:
            lines.append("    · %s" % reason)
    else:
        lines.append("  surfaces: none — no rule and no content claim this file")
    lines.append("  diff: +%d/−%d lines" % (len(facts.diff.added), len(facts.diff.removed)))
    for surface in facts.surfaces:
        reader = READERS.get(surface)
        if reader is None:
            continue
        view = reader(facts, policy)
        lines.append("  %s reader:" % surface)
        for key, value in view.items():
            lines.extend(_render_view(key, value))
    return lines


def explain_episode(view, *, cols: int = 100) -> list[str]:
    """The card plus the policy that produced it."""
    from . import render
    lines = render.render(view, cols=cols)
    lines.append("")
    lines.extend(explain_policy(view.policy))
    return lines


def explain_policy(policy: Policy) -> list[str]:
    return list(policy.describe())


def explain_definition(policy: Policy | None = None) -> list[str]:
    """The rules this package applies, spelled out with the numbers actually in force."""
    policy = policy or Policy.built_in()
    return [
        "how a non-code finding is decided, as configured:",
        "",
        "  1. a file is on a surface when the `e2.1` class says so, a path table claims it,",
        "     or the content says so. The reason is always printed (`explain <path>` shows",
        "     all three side by side).",
        "  2. env: declarations come from %s; references are read in the" % ", ".join(
            sorted(set(policy.words("declaration_globs")))[:3]) + "…",
        "     dialects each language actually uses, and a read with no fallback is marked.",
        "     'Nothing reads this' is claimed about %s (`non_code.env.reference_scope`)."
        % ("this episode only" if policy.text("reference_scope") == "episode"
           else "the repository, if the caller supplied the reference set"),
        "  3. a rename needs name-token similarity ≥ %s" % policy.number("rename_similarity"),
        "     (`non_code.env.rename_similarity`), and the old name being still referenced turns",
        "     it into the half-done-rename finding rather than a neutral one.",
        "  4. a committed value is a leak when the name is secret-shaped or the value is",
        "     credential-shaped and the value is not one of %s placeholder(s)" % len(
            policy.words("placeholder_values")),
        "     (`non_code.env.placeholder_values`).",
        "  5. migration reversibility is decided by the tool's own down marker; a missing one",
        "     is forward-only, and destructive statements come from",
        "     `non_code.migration.destructive_ops` (%d statements) — comments never match."
        % len(policy.words("destructive_ops")),
        "  6. CI, IaC and container findings are computed from the **delta**: a secret,",
        "     permission, trigger, resource, exposure or base image that was already there is",
        "     not this episode's finding. Escalating scopes: %s."
        % ", ".join(policy.words("escalating_permissions")[:4]) + ", …",
        "  7. every finding carries an action and an owner, so the checklist can be printed",
        "     without inventing either; `%s` rows per card before the remainder is counted"
        % policy.number("max_actions"),
        "",
        "Every number above is a config key; `fathom config explain --key",
        "non_code.findings.env_added` prints the layer that won it.",
    ]


def explain_rules() -> list[str]:
    """The finding table, independent of any repository's configuration."""
    return ["finding table (every row is a config switch):"] + describe_rules()


def explain_surfaces() -> list[str]:
    """The surface table: what each reader looks at, and where it comes from."""
    out = ["surfaces (a file can be on several at once):"]
    for surface in ("env", "flag", "migration", "ci", "iac", "container"):
        kinds = [rule.id for rule in RULES if rule.surface == surface]
        out.append("  %-10s %d findings: %s" % (surface, len(kinds), ", ".join(kinds)))
    return out


def demo(policy: Policy | None = None):
    """The docs' episode: six surfaces, one finding of every severity."""
    from .corpus import demo_episode
    return demo_episode(policy=policy)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _flag_view(facts, policy) -> dict:
    flags, notes = FLAGS.parse_declarations(facts.path, facts.after or "")
    return {"flags": [{"name": item.name, "line": item.line, "default": item.default,
                       "provider": item.provider} for item in flags],
            "reads": [{"name": item.name, "line": item.line, "default": item.default,
                       "provider": item.provider}
                      for item in FLAGS.parse_reads(facts.path, facts.after or "")],
            "notes": list(notes)}


def _render_view(key: str, value) -> list[str]:
    """One reader's field, as prose. Structured data in, text out, no silent omissions."""
    lines: list[str] = []
    if isinstance(value, list):
        if not value:
            lines.append("    %-14s (none)" % key)
            return lines
        lines.append("    %-14s %d" % (key, len(value)))
        for item in value[:12]:
            lines.append("      %s" % _one_line(item))
        if len(value) > 12:
            lines.append("      … %d more" % (len(value) - 12))
        return lines
    if isinstance(value, dict):
        for sub_key, sub_value in value.items():
            lines.append("    %-14s %s" % (sub_key, _one_line(sub_value)))
        return lines
    return ["    %-14s %s" % (key, _one_line(value))]


def _one_line(value) -> str:
    if isinstance(value, dict):
        bits = ["%s=%s" % (key, _one_line(item)) for key, item in value.items()
                if item not in ("", None, [], {}, 0, False)]
        return " ".join(bits) or "—"
    if isinstance(value, (list, tuple)):
        return ", ".join(_one_line(item) for item in value) or "—"
    text = str(value)
    return text if len(text) <= 110 else text[:109] + "…"


def _mask(value: str) -> str:
    return ENV._mask(value)
