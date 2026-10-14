"""Rendering for `fathom explain <path>` — the explainability half of `e0.9`.

The ticket's sentence is *"Every decision is explainable: `fathom explain <path>` prints
which rule matched and which rule it beat.\"* So the renderer is not a debug dump of a
verdict; it answers three questions in a fixed order, in the order a person reading a
surprising answer asks them:

1. **what did it decide** — class and noise, with the score noise carries;
2. **which rule said so, and where did that rule come from** — id, the pattern that
   actually matched (not merely the rule's pattern list), the layer, and the file or
   table that contributed it;
3. **which rule it beat, and why** — every other matched rule, each with a named reason
   (`lost: layer: repo fathom.toml beats built-in default`), plus the ones that merely
   agree, because \"three rules say `generated`\" and \"one rule says so\" are different
   amounts of confidence when you are deciding whether to trust a label.

Everything here is a pure function returning strings, for the same reason the config
renderers are (`recommender.config.explain`): a diagnostic that is not text-examinable
cannot be tested, and the tests in `tests/test_taxonomy_contract.py` assert on the prose
rather than on a private structure only this module could produce.
"""

from __future__ import annotations

from . import classes as C
from . import noise as N
from .builtins import BUILTINS_VERSION
from .engine import REASON_AGREES, REASON_WINNER, Match, Taxonomy, Verdict

WIDTH = 74


def header_lines(taxonomy: Taxonomy) -> list[str]:
    out = ["fathom taxonomy · built-in table v%d (this build v%d) · %d rules, %d patterns"
           % (taxonomy.version, BUILTINS_VERSION, taxonomy.stats()["rules"],
              taxonomy.stats()["entries"]),
           "table digest %s · class axis: layer > specificity > order; "
           "noise axis: layer > derived-first > specificity"
           % taxonomy.digest[:16]]
    return out


def _rule_line(match: Match, *, outcome: str = "") -> str:
    """One rule on one line: the pattern that matched, the rule that owns it, what it
    claims, which layer declared it, and where that layer's text lives."""
    text = "%-26s %-30s %-9s %-9s %s" % (match.pattern, match.rule_id,
                                         match.rule.cls or "noise-only", match.layer,
                                         match.source)
    return (text + "  " + outcome) if outcome else text


def _axis(headline: str, winner: Match, matches: list[Match]) -> list[str]:
    lines = [headline,
             "  matched   %s" % _rule_line(winner).strip(),
             "            rule note: %s" % (winner.rule.note or "—")]
    beaten = [m for m in matches if m.outcome != REASON_WINNER]
    agrees = [m for m in beaten if m.outcome == REASON_AGREES]
    lost = [m for m in beaten if m.outcome != REASON_AGREES]
    if lost:
        lines.append("  beaten by %d rule%s:" % (len(lost), "" if len(lost) == 1 else "s"))
        for match in lost:
            lines.append("            %s" % _rule_line(match).strip())
            lines.append("            %s" % match.outcome)
    else:
        lines.append("  beaten by nothing: no other rule spoke for this path")
    if agrees:
        lines.append("  agrees (%d): %s"
                     % (len(agrees), ", ".join("%s %s" % (m.rule_id, m.pattern)
                                               for m in agrees)))
    return lines


def _class_headline(verdict: Verdict) -> str:
    info = C.CLASS_INFO[verdict.cls]
    return "CLASS  %s — %s" % (verdict.cls, info["doc"].split(". ")[0])


def _noise_headline(verdict: Verdict) -> str:
    return "NOISE  %s (%.2f) — %s" % (verdict.noise, verdict.noise_score,
                                       _noise_doc(verdict.noise))


def _noise_doc(level: str) -> str:
    return {
        N.Noise.INFORMATIVE: "reviewable signal",
        N.Noise.GENERATED: "derived from something else",
        N.Noise.LOCKFILE: "resolved dependency state",
        N.Noise.REFORMAT: "whitespace-only change (content evidence)",
        N.Noise.VENDORED: "someone else's code",
        N.Noise.UNKNOWN: "noise because a rule said so, with no named category",
    }.get(level, level)


def verdict_block(taxonomy: Taxonomy, verdict: Verdict) -> list[str]:
    """One path's full answer, without the table header: used by `fathom config explain
    --path` so the two verbs cannot give different answers about the same file."""
    return ["path    %s" % verdict.path] + _axis_text(taxonomy, verdict) + _footnote(verdict)


def explain_text(taxonomy: Taxonomy, verdicts, *, quiet: bool = False) -> str:
    """The body of `fathom explain <path>`. One block per path; quiet gives one line each."""
    if isinstance(verdicts, Verdict):
        verdicts = [verdicts]
    if quiet:
        return "\n".join(summary_line(v) for v in verdicts) + "\n"
    out = header_lines(taxonomy)
    for verdict in verdicts:
        out += [""] + verdict_block(taxonomy, verdict)
    out += _warnings_lines(taxonomy)
    return "\n".join(out).rstrip() + "\n"


def _axis_text(taxonomy: Taxonomy, verdict: Verdict) -> list[str]:
    lines = _axis(_class_headline(verdict), verdict.class_match, verdict.class_matches)
    noise = verdict.noise_match
    if noise is None:
        return lines
    same_rule = (noise.rule.id, noise.pattern) == (verdict.class_match.rule.id,
                                                   verdict.class_match.pattern)
    if same_rule:
        # One rule decided both axes. Printing its provenance twice would read like two
        # independent facts, so the noise block keeps its headline and its agreements and
        # points at the class block for the rest.
        lines.append("  %s — same rule as above" % _noise_headline(verdict))
        agrees = [m for m in verdict.noise_matches
                  if m.outcome == REASON_AGREES and m.rule.id != noise.rule_id]
        if agrees:
            lines.append("  agrees (%d): %s"
                         % (len(agrees), ", ".join("%s %s" % (m.rule_id, m.pattern)
                                                   for m in agrees)))
        cancelled = [m for m in verdict.noise_matches if m.rule.negated]
        if cancelled:
            lines.append("  %s named this path, so the noise claim was withdrawn"
                         % ", ".join("`%s`" % m.pattern for m in cancelled))
        return lines
    lines += _axis(_noise_headline(verdict), noise, verdict.noise_matches)
    return lines


def _footnote(verdict: Verdict) -> list[str]:
    """What the decision *means* downstream, so the answer is actionable, not just sourced."""
    lines = ["  diffable  %s · reviewable %s"
             % (_yes_no(verdict.diffable), _yes_no(verdict.reviewable))]
    if verdict.cls == C.PathClass.OTHER:
        lines.append("  note      nothing matched: `other` is honest, not a guess; a "
                     "recurring `other` is a missing rule in this table")
    if any(m.rule.negated for m in verdict.noise_matches):
        lines.append("  note      a `!`-negation named this path: noise cleared, class "
                     "untouched - re-promotion is the escape hatch for a directory a glob "
                     "over-claimed")
    if verdict.is_noise:
        lines.append("  note      noise is a view, never a deletion: the raw change stays "
                     "one keypress away and this rule is printed beside it")
    return lines


def _yes_no(value: bool) -> str:
    return "yes" if value else "no"


def summary_line(verdict: Verdict) -> str:
    return "%-11s %-11s %-26s %-8s %s" % (verdict.cls, verdict.noise, verdict.rule_id,
                                          verdict.layer, verdict.path)


SUMMARY_HEADER = "%-11s %-11s %-26s %-8s %s" % ("CLASS", "NOISE", "RULE", "LAYER", "PATH")


def _warnings_lines(taxonomy: Taxonomy) -> list[str]:
    if not taxonomy.warnings:
        return []
    out = ["", "%d taxonomy warning%s:" % (len(taxonomy.warnings),
                                           "" if len(taxonomy.warnings) == 1 else "s")]
    out += ["  " + w.render() for w in taxonomy.warnings]
    return out


def classes_text() -> str:
    """The two vocabularies, as the reference `fathom taxonomy classes` prints."""
    out = ["path classes (%d) — what a path IS" % len(C.PATH_CLASSES), ""]
    for name in C.PATH_CLASSES:
        info = C.CLASS_INFO[name]
        out.append("  %-10s %-11s noise=%-11s diffable=%-3s reviewable=%-3s %s"
                   % (name, info["title"], info["noise"], _yes_no(info["diffable"]),
                      _yes_no(info["reviewable"]), info["doc"]))
    out += ["", "noise levels (%d) — how much reviewable information a change carries"
            % len(N.NOISE_LEVELS), ""]
    for level in N.NOISE_LEVELS:
        out.append("  %-11s score=%.2f  %s" % (level, N.score_of(level), _noise_doc(level)))
    out += ["", "  path rules may not claim: %s — that fact lives in the diff, not the "
                "path (e2.2 owns reformat detection)" % ", ".join(N.CONTENT_ONLY_LEVELS)]
    return "\n".join(out) + "\n"


def rules_text(taxonomy: Taxonomy, *, cls: str | None = None, pattern: str = "",
               layer: str | None = None) -> str:
    """The effective table, filtered — the answer to \"is there already a rule for this?\"."""
    rows = []
    for rule in taxonomy.rules:
        if cls and rule.cls != cls:
            continue
        if layer and rule.layer != layer:
            continue
        if pattern and not any(pattern in p for p in rule.patterns):
            continue
        rows.append(rule)
    out = ["%d of %d rules" % (len(rows), len(taxonomy.rules)), ""]
    for rule in rows:
        out.append("  %-16s %-10s %-9s %s"
                   % (rule.id, rule.cls or "noise-only", rule.layer,
                      ", ".join(rule.patterns)))
        if rule.note:
            out.append("  %-16s %s" % ("", rule.note))
    if not rows:
        out.append("  (nothing matched the filter; `fathom taxonomy classes` lists the "
                   "class names)")
    return "\n".join(out) + "\n"
