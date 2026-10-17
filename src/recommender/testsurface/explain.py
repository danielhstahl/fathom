"""The prose half: `fathom testsurface explain`, the definition read out loud, and the demo.

Same engine as the card, different question. The card answers "what did this episode do to
the test surface"; `explain` answers the two questions a reviewer asks when they do not
believe the card:

* **"why does it think that is a test?"** — the parser, the framework evidence, the case
  list with each case's line, skip marker and assertion census;
* **"why did it not flag my new function?"** — the definition, with the layer that won each
  number and the three coverage tests spelled out per symbol, each one shown as the reason a
  finding did *not* fire.

`explain --definition` prints the tunable definition as sentences, because AC 3's "documented
rather than a guess" is only true if the documentation and the running values are the same
artifact and can be diffed.
"""

from __future__ import annotations

from . import corpus as C
from . import delta as DE
from . import diff as D
from . import parsers as P
from . import public as PUB
from .policy import Policy, describe_rules


def explain_file(path: str, before: str | None, after: str | None,
                 *, policy: Policy | None = None) -> list[str]:
    """One file pair: what is a test here, and what changed about it."""
    policy = policy or Policy.built_in()
    old = P.parse(path, before) if before is not None else None
    new = P.parse(path, after) if after is not None else None
    current = new or old
    lines: list[str] = [path]
    lines.append("  language %s · parser %s · state %s"
                 % (current.language or "—", current.parser or "—", current.state))
    if current.frameworks:
        lines.append("  framework evidence: %s" % ", ".join(current.frameworks))
    for note in current.notes:
        lines.append("  note: %s" % note)
    for evidence in current.evidence:
        lines.append("  · %s" % evidence)
    if current.test_regions:
        lines.append("  test regions inside this file: %s"
                     % ", ".join("%d–%d" % span for span in current.test_regions))
    for case in current.cases:
        flags = []
        if case.skipped:
            flags.append("skipped(%s%s)" % (case.skip_kind,
                                            " %s" % case.skip_reason if case.skip_reason
                                            else ""))
        if case.focused:
            flags.append("focused")
        if case.variants > 1:
            flags.append("%d variants" % case.variants)
        lines.append("  case %-40s %s:%d  %s"
                     % (case.qualified, path, case.line, " ".join(flags)))
        counts = {"exact": case.exact, "presence": case.presence,
                  "message": sum(1 for a in case.assertions if a.strength == P.MESSAGE)}
        lines.append("      %d assertion(s): %d exact, %d presence, %d message-only"
                     % (len(case.assertions), counts["exact"], counts["presence"],
                        counts["message"]))
        for assertion in case.assertions:
            lines.append("        %s" % assertion.describe())
    if old is not None and new is not None:
        pair = D.case_delta(path, old, new)
        counts = pair.counts()
        lines.append("  delta: +%d cases, −%d, %d skipped, %d un-skipped, %d weakened, "
                     "%d assertions lost"
                     % (counts["added"], counts["removed"], counts["skipped"],
                        counts["unskipped"], counts["weakened"], counts["assertions_lost"]))
        for item in pair.weakened:
            lines.append("    weakened: %s · %s" % (item.case, item.detail()))
            for evidence in item.evidence():
                lines.append("      · %s" % evidence)
        for item in pair.assertions_lost:
            lines.append("    lost: %s · %s" % (item.case, item.detail()))
            for evidence in item.evidence():
                lines.append("      · %s" % evidence)
        for note in pair.notes:
            lines.append("    note: %s" % note)
    symbols = PUB.public_symbols(path, after if after is not None else before)
    if symbols:
        lines.append("  public symbols: %d (%d with logic)"
                     % (len(symbols), sum(1 for item in symbols if item.has_logic)))
        for symbol in symbols:
            lines.append("    %-28s %-9s %s:%d  %d body, %d logic%s"
                         % (symbol.name, symbol.kind, path, symbol.line, symbol.body_lines,
                            symbol.logic_lines,
                            "" if symbol.visibility == "public"
                            else "  [%s]" % symbol.visibility))
    return lines


def explain_episode(view: DE.SurfaceDelta, *, cols: int = 100) -> list[str]:
    """The card plus the definition that produced it."""
    from . import render
    lines = render.render(view, cols=cols)
    lines.append("")
    lines.extend(explain_policy(view.policy))
    return lines


def explain_policy(policy: Policy) -> list[str]:
    return list(policy.describe())


def explain_definition(policy: Policy | None = None) -> list[str]:
    """The tunable definition, spelled out in the order the decision uses it."""
    policy = policy or Policy.built_in()
    logic = ("at least one body line that calls, assigns, branches or returns, and"
             if policy.flag("require_logic") else "any body, and")
    mention = " (on)" if policy.flag("mention_in_test") else " (off)"
    return [
        'the definition of "new public logic, no new test", as configured:',
        "",
        "  1. the changed path is `source` (taxonomy `e0.9`) and is not claimed by",
        "     `test.untested.exclude` (%s);"
        % (", ".join(str(item) for item in (policy.knob("exclude") or []))
           or "nothing excluded"),
        "  2. the symbol is public \u2014 `pub` / a name not starting with `_` / `export` /",
        "     an initial capital, per language, plus `test.untested.visibility` = %s;"
        % policy.text("visibility"),
        "  3. the symbol is new: present in the new revision, absent from the old, keyed by",
        "     language and name \u2014 so renaming a function reads as a new symbol, which is",
        "     what a reviewer wants it to do;",
        "  4. it is logic: %s at least %s body line(s) \u2014 `test.untested.require_logic`"
        " = %s, `test.untested.min_body_lines` = %s;"
        % (logic, policy.number("min_body_lines"),
           "true" if policy.flag("require_logic") else "false",
           policy.number("min_body_lines")),
        "  5. and no test surface in the episode covers it, where covering means any of:",
        "       \u00b7 a changed `test` file whose path shares \u2265 %s name token(s) with"
        " the source file (`test.untested.path_overlap`);"
        % policy.number("path_overlap"),
        "       \u00b7 a new test case whose name shares \u2265 %s token(s) with the symbol"
        " (`test.untested.name_overlap`);" % policy.number("name_overlap"),
        "       \u00b7 the symbol's name appearing in lines this episode added to a test",
        "         file (`test.untested.mention_in_test`%s);" % mention,
        "  6. scope: %s (`test.untested.scope`)." % _scope_line(policy),
        "",
        "Every number above is a config key; `fathom config explain --key",
        "test.untested.min_body_lines` prints the layer that won it.",
    ]


def _scope_line(policy: Policy) -> str:
    scope = policy.text("scope")
    if scope == "repo":
        return ("`repo` — a symbol the repository's existing tests already mention is not "
                "flagged, when the caller supplies those mentions")
    return "`episode` — only this episode's test surface can cover a symbol"


def explain_rules() -> list[str]:
    """The finding table, independent of any repository's configuration."""
    return ["finding table (every row is a config switch):"] + describe_rules()


def demo(policy: Policy | None = None) -> DE.SurfaceDelta:
    """The four-language demo episode every doc and CLI flag points at."""
    files = C.demo_episode()
    episode = DE.Episode(id="ep-demo", title="one episode, four languages",
                         files=tuple(DE.Change.from_pair(path, before, after)
                                     for path, (before, after) in files.items()))
    return DE.measure(episode, policy or Policy.built_in())
