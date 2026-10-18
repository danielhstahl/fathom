"""The prose half: what a card says, why it says it, and what it would say if a knob moved.

Same engine as the renderer, so the explanation cannot drift from the card it explains — which is
the only reason an `explain` verb is worth having. `definition` prints the decision procedure with
**the numbers currently in force**, not the ones in the source, because a reviewer debugging a
repository's `fathom.toml` needs the effective answer.
"""

from __future__ import annotations

from . import corpus as CORPUS
from . import eval as EVAL
from . import render as RENDER
from .blast import measure as measure_blast
from .compose import Card, collect_findings
from .links import describe_views
from .model import Episode
from .policy import (BLAST_KNOBS, CARD_KNOBS, SECTION_INDEX, STATE_UNAVAILABLE, Policy,
                     blast_definition, describe_sections, severity_rank)

#: The case the concept document quotes: a module graph, every section measured.
DEMO_CASE = "graph_full"


def demo(policy: Policy | None = None) -> Card:
    """The card the concept document prints: a graph-backed answer, every section filled."""
    return CORPUS.fixture(DEMO_CASE, policy=policy)


def explain_card(card: Card, *, verbose: bool = False) -> list[str]:
    """Walk the card in prose: what each section answered, and what stopped it answering more."""
    out: list[str] = ["card for %s%s" % (card.episode_id,
                                         " — %s" % card.title if card.title else ""),
                      "analyses: %s" % (card.analysis_source or "caller supplied"),
                      "digest %s (policy digest %s)"
                      % (card.digest(), card.policy.config_digest or "none"),
                      "sections measured: %d of %d present, %d enabled by policy"
                      % (card.measured_count(), len(card.sections), len(SECTION_INDEX))]
    for section in card.sections:
        out.append("")
        out.append("%s — %s" % (section.title, section.spec.question))
        out.append("  state %s · owner %s" % (section.state, section.owner))
        if section.state == STATE_UNAVAILABLE:
            out.append("  why it is empty: %s" % section.reason)
            out.append("  to fill it: %s" % section.run)
            continue
        out.append("  answer: %s" % section.lead)
        if section.basis:
            out.append("  basis: %s" % section.basis)
        if section.confidence is not None:
            out.append("  confidence %s" % card.policy.state(section.confidence))
        for figure in section.figures:
            out.append("  · %s: %s" % (figure.label, figure.value))
            if verbose:
                for item in figure.links:
                    out.append("      opens %s" % item.render())
                    for row in card.evidence.resolve(item).lines[:3]:
                        out.append("        – %s" % row)
        for note in section.notes:
            out.append("    note: %s" % note)
    for note in card.notes:
        out.append("note: %s" % note)
    return out


def explain_section(card: Card, section_id: str) -> list[str]:
    section = card.section(section_id)
    if section is None:
        spec = SECTION_INDEX.get(section_id)
        key = spec.config_key if spec else "card.sections.<name>"
        return ["this card has no section %r — it is switched off by %s, or was never enabled"
                % (section_id, key)]
    out = ["%s · %s" % (section.title, section.spec.question),
           "owner: %s" % section.owner,
           "what it answers: %s" % section.spec.doc,
           "switch: %s" % section.spec.config_key,
           "state: %s" % section.state]
    if section.state == STATE_UNAVAILABLE:
        out.append("why it is empty: %s" % section.reason)
        out.append("to fill it: %s" % section.run)
        return out
    out.append("answer: %s" % section.lead)
    if section.basis:
        out.append("basis: %s" % section.basis)
    for figure in section.figures:
        out.append("  · %s: %s" % (figure.label, figure.value))
        for item in figure.links:
            out.append("      → %s" % item.render())
    for note in section.notes:
        out.append("    note: %s" % note)
    return out


def explain_blast(episode: Episode, policy: Policy | None = None, graph=None) -> list[str]:
    """How the reach was decided for one episode, rule by rule."""
    active = policy or Policy.built_in()
    blast = measure_blast(episode, active, graph=graph)
    out = ["blast radius for %s" % episode.id,
           "basis: %s (%s)" % (blast.basis, blast.basis_reason or "—"),
           "confidence: %s" % active.state(blast.confidence),
           "counts: %d direct, %d one hop, %d beyond the hop limit"
           % (len(blast.direct), len(blast.one_hop), blast.beyond),
           "the procedure, with the numbers in force:"]
    for line in blast_definition(active):
        out.append("  %s" % line)
    out.extend(blast.lines())
    return out


def explain_policy(policy: Policy) -> list[str]:
    return policy.describe()


def explain_definition(policy: Policy | None = None) -> list[str]:
    """The decision procedure as configured: the numbers a card is being built with right now."""
    active = policy or Policy.built_in()
    out: list[str] = ["what the card decides, and with which numbers", "",
                      "sections, in the order the card prints them:"]
    for line in describe_sections():
        out.append("  %s" % line)
    out.append("")
    out.append("the card's own numbers:")
    for knob in CARD_KNOBS:
        out.append("  %-30s %s — %s" % (knob.key, active.knob(knob.name), knob.doc))
    out.append("")
    out.append("the blast-radius procedure:")
    for line in blast_definition(active):
        out.append("  %s" % line)
    for knob in BLAST_KNOBS:
        out.append("  %-30s %s — %s" % (knob.key, active.knob(knob.name), knob.doc))
    return out


def explain_views() -> list[str]:
    return ["the evidence views a card figure can link to",
            "status `shipped` runs in this repository today; the others name the ticket that owns "
            "them, and the card prints that rather than pretending."] + \
           ["  %s" % line for line in describe_views()]


def explain_sections() -> list[str]:
    out: list[str] = ["the seven questions the card answers"]
    for spec in SECTION_INDEX.values():
        out.append("  %-13s %s" % (spec.id, spec.question))
        out.append("  %-13s owner: %s" % ("", spec.owner))
        out.append("  %-13s %s" % ("", spec.doc))
        out.append("  %-13s switch: %s" % ("", spec.config_key))
    return out


def explain_figure(card: Card, section_id: str, label: str) -> list[str]:
    """One figure, fully: its value, its buttons, and every evidence row behind it."""
    section = card.section(section_id)
    if section is None:
        return ["no section %r on this card" % section_id]
    for figure in section.figures:
        if figure.label != label:
            continue
        out = ["%s / %s: %s" % (section_id, figure.label, figure.value)]
        if figure.basis:
            out.append("  basis: %s" % figure.basis)
        if figure.confidence is not None:
            out.append("  confidence %s" % card.policy.state(float(figure.confidence)))
        out.append("  the evidence behind it must contain: %r" % figure.key)
        for item in figure.links:
            resolution = card.evidence.resolve(item)
            out.append("  → %s%s" % (item.render(),
                                     "" if resolution.found else "  (RESOLVES TO NOTHING)"))
            for row in resolution.lines:
                out.append("      · %s" % row)
        return out
    return ["no figure %r in section %r" % (label, section_id)]


def explain_attention(card: Card, *, show_all: bool = False) -> list[str]:
    """The attention roll-up with its provenance: which analysis raised what, and where."""
    if card.analyses is None:
        return ["this card carries no analyses to roll findings up from"]
    items = collect_findings(card.analyses)
    floor = card.policy.attn_floor()
    shown = [item for item in items if severity_rank(item.severity) <= floor]
    out = ["attention for %s — %d finding(s) raised, %d at or above `card.attention.severity "
           "= %s`" % (card.episode_id, len(items), len(shown),
                       card.policy.text("attention_severity"))]
    for item in (items if show_all else shown):
        out.append("  %s" % item.render())
        out.append("      %s" % item.detail)
        for row in item.evidence[:3]:
            out.append("        · %s" % row)
    if len(items) > len(shown) and not show_all:
        out.append("  … %d lighter finding(s) counted and not listed (--all prints them)"
                   % (len(items) - len(shown)))
    if not items:
        out.append("  nothing any supplied analysis flagged; a risk score would come from `e4.4`")
    return out


def golden(name: str = DEMO_CASE, *, cols: int = RENDER.DEFAULT_COLS) -> str:
    """The blessed text the docs quote, produced by the same call every time."""
    return "\n".join(RENDER.render(CORPUS.fixture(name), cols=cols)) + "\n"


def score_report(*, policy: Policy | None = None, repeats: int = 3,
                 verbose: bool = False) -> EVAL.Report:
    return EVAL.score(policy=policy, repeats=repeats)
