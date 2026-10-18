"""The prose half: what the shapes mean, what the numbers in force are, and how to read a card.

Same rule as every sibling package: the analysis decides, this module explains, and neither
reaches into the other. Everything printed here is read back out of `policy` and `delta`, so
`fathom structure definition` cannot drift from what `measure` actually did — a document that
quotes 3 as the move floor while the running value is 5 is worse than no document.

Four readers ask for this text:

* a reviewer, who wants the sentence and the receipts (`explain_episode`);
* an operator changing a knob, who wants to know which file set that knob decides
  (`explain_policy`, `explain_definition`);
* the concept document, which prints `definition` and `shapes` verbatim rather than copying
  numbers into prose;
* CI, which runs the verbs so a broken explanation is a red build rather than a stale wiki.
"""

from __future__ import annotations

from . import corpus as C
from .delta import measure
from .policy import KNOBS, RULES, SEVERITY_ORDER, Policy, shape_rules


def explain_policy(policy: Policy) -> list[str]:
    return policy.describe()


def explain_definition(policy: Policy) -> list[str]:
    """The decision procedure, spelled out with the numbers currently in force."""
    out: list[str] = ["what this ticket decides, and with which numbers"]
    out.append("")
    out.append("pairing — which file became which file")
    out.append("  1. a `renamed` entry in the change list is taken as asserted")
    out.append("  2. a caller's pairing counts as a fact at or above %.2f "
               "(`structure.move.fact_bar`)" % policy.number("move_fact_bar"))
    if policy.flag("infer_moves"):
        out.append("  3. otherwise a pairing is inferred when the line-set Jaccard reaches %s "
                   "(`structure.move.min_similarity`) and the stem matches, or the content is "
                   "at least 0.9 alike — and it is labelled a guess whatever the score"
                   % _num(policy.number("move_min_similarity")))
    else:
        out.append("  3. inference is off (`structure.move.infer_content` = false): unpaired "
                   "deletions and creations stay unpaired")
    out.append("  renames inside one directory are excluded: no directory gained or lost "
               "anything")
    out.append("")
    out.append("shapes — the four the ticket names, plus the remainder")
    for line in shape_rules():
        out.append("  %s" % line)
    out.append("  a whole-directory story needs ≥ %d file(s) "
               "(`structure.move.min_files`), ≥ %s of the directory "
               "(`structure.move.min_share`)%s"
               % (policy.integer("move_min_files"), _num(policy.number("move_min_share")),
                  ", and the source emptied (`structure.move.require_emptied`)"
                  if policy.flag("move_require_emptied") else ""))
    out.append("")
    out.append("churn — the weighted mean that makes a reorganisation obvious")
    for name in ("w_moved_files", "w_moved_lines", "w_directory_churn", "w_stories",
                 "w_shape_shift"):
        knob = next(knob for knob in KNOBS if knob.name == name)
        out.append("  %-46s = %-6s %s" % (knob.key, _num(policy.knob(name)), knob.doc))
    out.append("  bands: under %s is `local`, at or over %s is `structural`, between is "
               "`transitional`" % (_num(policy.number("band_local")),
                                   _num(policy.number("band_structural"))))
    out.append("  story saturation: %s stories (`structure.churn.story_ceiling`); depth "
               "saturation: %s levels (`structure.churn.depth_scale`)"
               % (_num(policy.number("story_ceiling")), _num(policy.number("depth_scale"))))
    out.append("")
    out.append("every knob, with the value and the layer that won it")
    for line in policy.definition_lines():
        out.append("  %s" % line)
    out.append("")
    out.append("findings — the switches, in severity order")
    for severity in SEVERITY_ORDER:
        for rule in RULES:
            if rule.severity != severity:
                continue
            enabled, source = policy.effective(rule.id)
            out.append("  %-22s %-5s %-9s %s%s"
                       % (rule.id, "on" if enabled else "off", rule.severity, rule.doc,
                          "" if source == "built-in default" else "  [%s]" % source))
    return out


def explain_shapes() -> list[str]:
    out: list[str] = ["structural shapes: what each one claims"]
    for line in shape_rules():
        out.append("  %s" % line)
    out.append("")
    out.append("  a shape is a grouping of pairings, and it is only as confident as its "
               "weakest pairing; a story built on one inferred pairing is an inferred story")
    out.append("  shapes never invent a pairing to make a story: no pairing, no shape")
    return out


def explain_episode(episode, policy: Policy | None = None, *, classifier=None) -> list[str]:
    """One episode, in prose: the sentence, then the receipts, then what could not be seen."""
    view = measure(episode, policy or Policy.built_in(), classifier)
    summary = view.summary()
    out: list[str] = [view.episode_id or "episode", ""]
    out.append("summary: %s" % summary["sentence"])
    out.append("verdict: %s" % view.headline())
    out.append("")
    out.append("chords of the sentence, each with its evidence:")
    for clause in summary["clauses"]:
        out.append("  · %s" % clause["text"])
        for evidence in clause["evidence"]:
            out.append("      — %s" % evidence)
    out.append("")
    out.append("churn: %.3f (%s)" % (view.churn.score, view.churn.band))
    for line in view.churn.lines()[1:]:
        out.append("  %s" % line.strip())
    out.append("")
    out.append("confidence %.2f: %s" % (view.confidence, "; ".join(
        view.confidence_reasons())))
    out.append("tree basis: %s" % view.basis)
    if view.findings:
        out.append("")
        out.append("findings:")
        for finding in view.findings:
            out.append("  %s" % finding.render())
    if view.partials:
        out.append("")
        out.append("what this answer could not see:")
        for partial in view.partials:
            out.append("  · %s" % partial)
    return out


def explain_directory(episode, directory: str, policy: Policy | None = None,
                      *, classifier=None) -> list[str]:
    """One directory: every line and file the table credited to it, and why."""
    view = measure(episode, policy or Policy.built_in(), classifier)
    want = str(directory).rstrip("/")
    rows = [row for row in view.rows if row.path == want]
    if not rows:
        return ["no row for `%s`: this episode did nothing there" % directory or "./"]
    row = rows[0]
    out = ["%s — depth %d" % (row.display, row.depth),
           "  files %d → %d, code lines %d → %d" % (row.files_before, row.files_after,
                                                    row.code_lines_before,
                                                    row.code_lines_after),
           "  +%d added / −%d removed / %d moved in / %d moved out"
           % (row.lines_added, row.lines_removed, row.lines_moved_in, row.lines_moved_out),
           "  %d created, %d deleted, %d moved in, %d moved out"
           % (row.files_created, row.files_deleted, row.files_moved_in, row.files_moved_out)]
    if row.directory_added:
        out.append("  new directory (basis: %s)" % row.basis)
    if row.directory_removed:
        out.append("  directory removed (basis: %s)" % row.basis)
    stories = [story for story in view.stories
               if story.source == want or want in story.destinations]
    for story in stories:
        out.append("  story: %s — %s" % (story.shape, story.summary_clause()))
    return out


def demo(name: str = "subtree_move"):
    """The card the concept document prints."""
    return measure(C.fixture(name))


def _num(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        text = ("%.2f" % value).rstrip("0").rstrip(".")
        return text or "0"
    return str(value)
