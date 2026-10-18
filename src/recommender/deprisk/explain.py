"""The prose half: what the rules are, what the numbers in force are, and how to read a card.

Same rule as every sibling package: the analysis decides, this module explains, and neither reaches
into the other. Every number printed here is read back out of `policy`, so `fathom deprisk
definition` cannot drift from what `measure` actually did — a document quoting 24 hours as the cache
freshness bar while the running value is 6 is worse than no document.

Four readers ask for this text:

* a reviewer, who wants to know what a `license-denied` row is claiming and which key decided it;
* an operator, who wants to know which file set a knob, and whether the network is ever going to be
  touched here (`explain_policy`, `consent_text`);
* the concept document, which prints `definition`, `rules` and `matrix` verbatim instead of copying
  them into prose;
* CI, which runs the verbs so a stale explanation is a red build rather than a stale wiki.
"""

from __future__ import annotations

from .policy import (
    CLASS_NETWORK, CLASS_STRONG, CLASS_UNKNOWN, CLASS_WEAK, KNOBS, RULES, Policy, describe_rules,
    matrix_rows)
from .provenance import INTENT_LABEL, REMEDY_EPISODE, REMEDY_INTENT

#: The ladder, printed in the order it is consulted — the order is the argument.
LADDER: tuple[tuple[str, str], ...] = (
    ("1. the episode under review",
     "the package is new in this episode, so this episode introduced it. This is the common case, "
     "and it needs no index."),
    ("2. a recorded introduction",
     "an earlier episode's record says it added this package. On by default "
     "(`deps.provenance.search_previous_episodes`), because the question usually gets asked weeks "
     "after the fact."),
    ("3. not recorded",
     "nothing says. That is printed as the answer, with the two things that would fill it: "
     "`%s` and `%s`." % (REMEDY_INTENT, REMEDY_EPISODE)),
)

INTENT_LADDER: tuple[tuple[str, str], ...] = (
    ("`Agent-Intent:` trailer",
     "a line in the commit message stating what the agent was asked for. Written by hand or by an "
     "adapter, stored in git, and the path that works today."),
    ("harness turn prompt",
     "the prompt text from the turn that made the change — available only if intent capture (`e4.2`) "
     "stores it. Hooks v1 stores `prompt_sha256` and `prompt_chars` and not the text (ADR-005 §2.2), "
     "so a digest is reported as a partial, not as an answer."),
    ("plan item / review note / manual note",
     "anything else a repository records and is willing to have printed as the reason a dependency "
     "arrived."),
    ("attribution",
     "`agent codex` is not an answer to `who asked for this`: it says who executed the change. Off "
     "by default (`deps.provenance.agent_counts_as_intent`), and it should stay off."),
)


def definition_text(policy: Policy | None = None) -> list[str]:
    """The decision procedure, spelled out with the numbers currently in force."""
    active = policy or Policy.built_in()
    out: list[str] = ["what this ticket decides, and with which numbers", ""]
    out.append("provenance — who asked for this")
    for head, body in LADDER:
        out.append("  %s — %s" % (head, body))
    out.append("  an intent counts as the answer only from: %s"
               % ", ".join("`%s`" % item for item in active.words("prov_intent_sources")))
    out.append("  and only at or above confidence %s (`deps.provenance.min_confidence`); below it "
               "the intent prints as a guess" % _num(active.number("prov_min_confidence")))
    out.append("  attribution standing in for intent: %s (`deps.provenance.agent_counts_as_intent`)"
               % ("allowed" if active.flag("prov_agent_counts_as_intent") else "not allowed"))
    out.append("")
    out.append("intent evidence, weakest last")
    for head, body in INTENT_LADDER:
        out.append("  %-34s %s" % (head, body))
    out.append("")
    out.append("licence — may this repository carry this licence where it now carries it")
    out.append("  an expression is split into the options a reviewer may take:")
    out.append("    `AND` is cumulative, so the most restrictive part decides")
    out.append("    `OR` is a choice, so the least restrictive option is taken and named, and the")
    out.append("    options not taken are printed rather than dropped")
    out.append("  classes are taught by table: permissive %d ids, weak copyleft %d, strong "
               "copyleft %d, network copyleft %d"
               % (len(active.words("lic_permissive")), len(active.words("lic_weak")),
                  len(active.words("lic_strong")), len(active.words("lic_network"))))
    out.append("  `deps.license.deny` (%s) outranks the matrix wherever the taken option contains "
               "one of them" % (", ".join(active.words("lic_deny")) or "empty"))
    out.append("  the matrix, with the layer deciding which column:")
    for line in matrix_rows(active):
        out.append("    %s" % line.strip())
    out.append("  library layers: %s, matched against `[layers]` on the manifest path that "
               "declared the package" % ", ".join(active.words("lic_library_layers")))
    out.append("  a path matching no layer is treated as %s "
               "(`deps.license.unknown_layer_policy` = `%s`)"
               % ("the stricter library rule" if active.text("lic_unknown_layer") != "lenient"
                  else "not a library", active.text("lic_unknown_layer")))
    out.append("  development-only packages are %s (`deps.license.ignore_dev`)"
               % ("skipped" if active.flag("lic_ignore_dev") else "checked like any other"))
    out.append("  a licence that moved between the two revisions is `license-drift`, whether or "
               "not the version moved")
    out.append("")
    out.append("advisories — is anything known about this package-version")
    out.append("  consulted at all: %s (`deps.advisories.enabled`)"
               % ("yes" if active.flag("adv_enabled") else "no"))
    out.append("  mode: `%s` (offline = no cache read, cache = read only, refresh = may fetch "
               "with consent)" % active.text("adv_mode"))
    out.append("  cache: %s" % active.text("adv_cache"))
    out.append("  fresh under %s h; past %s h the answer stops claiming ‘no advisories’ "
               "(`max_age_hours`, `stale_limit_hours`)"
               % (_num(active.number("adv_max_age_hours")),
                  _num(active.number("adv_stale_limit_hours"))))
    out.append("  at most %s packages per episode; the remainder is counted, never dropped in "
               "silence" % _num(active.number("adv_max_packages")))
    out.append("  ecosystems asked about: %s" % ", ".join(active.words("adv_ecosystems")))
    out.append("")
    out.append("network — three gates, and the repository layer is not the one that grants")
    for line in active.network.grants:
        out.append("  %s" % line)
    out.append("  verdict: %s" % active.network.reason)
    out.append("")
    out.append("every knob, with the value in force and the layer that won it")
    for line in active.definition_lines():
        out.append("  %s" % line)
    return out


def rules_text() -> list[str]:
    return describe_rules()


def matrix_text(policy: Policy | None = None) -> list[str]:
    return matrix_rows(policy)


def how_to_read(policy: Policy | None = None) -> list[str]:
    """What each section of the card claims, and where it comes from."""
    active = policy or Policy.built_in()
    out: list[str] = ["how to read a dependency-risk card", ""]
    out.append("header")
    out.append("  one sentence built only from counts that have rows behind them, then the")
    out.append("  advisory freshness clause, then the confidence and how many parts of the answer")
    out.append("  are stated partials. A clean episode still prints its confidence: 1.00 means")
    out.append("  nothing was missing, not that nothing was found.")
    out.append("")
    out.append("`who asked for this`")
    out.append("  one block per new direct dependency. The `introduced by:` line is the episode;")
    out.append("  the `intent:` line is the reason, with its source and confidence; the `asked by:`")
    out.append("  line is the graph path. When any of the three is missing the line says which, and")
    out.append("  names what would fill it: %s" % REMEDY_INTENT)
    out.append("")
    out.append("`advisories`")
    out.append("  the cache line comes first, always, because an advisory answer without its age is")
    out.append("  an unanchored claim. Past `stale_limit_hours` (%s h) the clean line changes tense"
               % _num(active.number("adv_stale_limit_hours")))
    out.append("  on purpose: 'not checked recently' is what an old cache actually licenses.")
    out.append("  Every package that was *not* checked is listed by reason, so 'no advisories' never")
    out.append("  stands alone.")
    out.append("")
    out.append("`licence`")
    out.append("  one row per package the episode touched: the expression, the class, the mode, the")
    out.append("  layer and the edge path that asked for it. `OR` rows name the option taken and the")
    out.append("  ones not taken. A drift row prints before and after, because the version may not")
    out.append("  have moved and the terms did.")
    out.append("")
    out.append("`findings`")
    out.append("  severity order, then kind, then path. Every row carries evidence lines; a row with")
    out.append("  no path or no evidence is refused by the model before it can be rendered. Findings")
    out.append("  switched off by their own key are counted at the bottom rather than vanishing.")
    out.append("")
    out.append("what makes a row a fact rather than an opinion")
    for rule in RULES:
        first = rule.doc.split(". ")[0].rstrip(".")
        out.append("  %-26s %s." % (rule.id, first))
    return out


def consent_text(policy: Policy) -> list[str]:
    network = policy.network
    return (["network consent for advisory lookups"]
            + ["  %s" % line for line in network.grants]
            + ["  verdict: %s" % network.reason])


def plan_text(plan) -> list[str]:
    """What a refresh would do — printed before anybody consents to it."""
    out: list[str] = ["advisory refresh plan", ""]
    out.append("  endpoint: %s (fixed in code: a config key here would let a clone redirect the "
               "fetch)" % plan.endpoint)
    out.append("  fetch allowed: %s — %s" % ("yes" if plan.may_fetch else "no", plan.reason))
    out.append("  per-request timeout: %s ms" % plan.timeout_ms)
    out.append("  would query %d package(s):" % len(plan.lookups))
    for lookup in plan.lookups[:40]:
        out.append("    %-32s POST %s/v1/query %s"
                   % (lookup.text, plan.endpoint,
                      jsonish(lookup.package, lookup.osv_ecosystem, lookup.version)))
    if len(plan.lookups) > 40:
        out.append("    … %d more" % (len(plan.lookups) - 40))
    if plan.over_cap:
        out.append("  over `deps.advisories.max_packages` (%d), not queried: %s"
                   % (len(plan.over_cap), ", ".join(list(plan.over_cap)[:6])))
    for reason, names in plan.skipped:
        out.append("  skipped (%d): %s — %s" % (len(names), reason,
                                                ", ".join(list(names)[:4])))
    return out


def jsonish(package: str, ecosystem: str, version: str) -> str:
    return '{"package":{"name":"%s","ecosystem":"%s"},"version":"%s"}' % (package, ecosystem,
                                                                         version)


def severity_of_modes() -> str:
    """How a matrix mode becomes a severity, printed once rather than per row."""
    return "`deny` → high, `warn` → notable, `allow` → no finding"


def class_names() -> tuple[str, ...]:
    return (CLASS_STRONG, CLASS_WEAK, CLASS_NETWORK, CLASS_UNKNOWN)


def mode_names() -> tuple[str, ...]:
    return ("deny", "warn", "allow")


def knob_lines() -> list[str]:
    return ["%s (%s): %s" % (knob.key, knob.units or "—", knob.doc) for knob in KNOBS]


def intent_labels() -> dict[str, str]:
    return dict(INTENT_LABEL)


def _num(value: float) -> str:
    text = ("%.2f" % float(value)).rstrip("0").rstrip(".")
    return text or "0"
