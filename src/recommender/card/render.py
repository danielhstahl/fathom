"""The card, as bytes: a one-screen summary block, the figures behind it, and an auditor.

Two blocks, because the card is asked to be two things at once.

**The summary block** is the one-screen answer: one line per section, in the ticket's order, so
`fathom card ep-1` fits a laptop pane. A section that could not be measured still gets its line,
saying so and naming the verb that would fill it — AC 3 is about the card being *complete* when
the analyses are missing, and a section that vanished is not a complete card.

**The figure block** is why every figure has a link: each number, its buttons, and up to
`card.max_evidence` evidence rows so a reviewer can see the proof without leaving the terminal.
The caps are honoured (`card.max_figures` per section) and a cap **names its remainder** — a card
that trimmed a figure in silence is a card that lied by omission, and `audit` fails it.

Determinism is the whole point of a golden, so the renderer is a pure function of the card: no
clock, no terminal width probe (the caller passes `cols`), no colour, no locale-sensitive
formatting, dictionary iteration ordered by explicit sorts. `test_the_card_is_the_blessed_text`
then pins the bytes, and `test_a_card_rendered_twice_is_the_same_bytes` pins the property.

The auditor (:func:`audit`) re-reads the produced text and fails when the render lost something
the model had: a section, a state, an unavailable section's reason or fill-verb, a figure value,
a figure's link, a stated confidence, a named remainder, a switched-off section, the digest. The
CLI runs it on its own output and exits non-zero from it, and the contract suite hands it four
tampered renders so the auditor is known to be capable of failing.
"""

from __future__ import annotations

import re
import textwrap as _textwrap
from dataclasses import dataclass, field

from .policy import (SECTION_INDEX, STATE_MEASURED, STATE_PARTIAL, STATE_UNAVAILABLE, Policy)

DEFAULT_COLS = 100
RULE = "─" * 8

#: The state marker, so a scan of the block shows what is missing without reading it.
_STATE_MARK = {STATE_MEASURED: " ", STATE_PARTIAL: "~", STATE_UNAVAILABLE: "?"}


@dataclass
class Render:
    """One render plus the audit that goes with it."""

    lines: list[str] = field(default_factory=list)
    problems: list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.problems

    def text(self) -> str:
        return "\n".join(self.lines)


def render(card, *, cols: int = DEFAULT_COLS, brief: bool = False,
           max_figures: int | None = None, show_evidence: bool = True,
           show_links: bool = True, header: bool = True) -> list[str]:
    """The card. Same card in, same bytes out — that is the acceptance criterion."""
    policy = card.policy
    cap = policy.integer("max_figures") if max_figures is None else max_figures
    lines: list[str] = []
    if header:
        title = " · %s — %s" % (card.episode_id, card.title) if card.title \
            else " · %s" % card.episode_id
        lines.extend(_wrap("%s episode card%s" % (RULE, title), cols, indent=2))
    lines.extend(_summary_block(card, cols=cols, explain_missing=brief))
    if not brief:
        for section in card.sections:
            lines.extend(_section_block(section, card, cols=cols, cap=cap,
                                        show_evidence=show_evidence, show_links=show_links))
    for note in card.notes:
        lines.extend(_wrap("  note: %s" % note, cols))
    footer = "  card digest %s" % card.digest()
    if policy.config_digest:
        # Sixteen characters, like the card digest: the footer has to stay on one line, and a
        # digest a reader cannot eyeball against a screenshot is not evidence.
        footer += " · policy digest %s" % policy.config_digest[:16]
    footer += " · %d of %d sections measured" % (card.measured_count(), len(card.sections))
    lines.extend(_wrap(footer, cols))
    return lines


def _summary_block(card, *, cols: int, explain_missing: bool = True) -> list[str]:
    """The one-screen answer: one line per section, in the ticket's order."""
    # Not "what this episode is": a header that begins with the word `what` is indistinguishable
    # from the `what` row by anything that scans the block by section id, including this module's
    # own auditor.
    lines = ["  at a glance"]
    for section in card.sections:
        mark = _STATE_MARK.get(section.state, " ")
        head = "  %s %-12s" % (mark, section.id)
        lead = section.brief or section.reason
        lines.extend(_wrap("%s %s%s" % (head, lead,
                                        "" if section.state == STATE_MEASURED
                                        else " [%s]" % section.state),
                           cols, indent=len(head)))
        if explain_missing and section.state == STATE_UNAVAILABLE:
            lines.extend(_wrap("    why: %s" % section.reason, cols, indent=10))
            lines.extend(_wrap("    fill it: %s" % section.run, cols, indent=10))
    if card.hidden:
        for section_id, source in card.hidden:
            spec = SECTION_INDEX[section_id]
            off = "  x %-12s" % section_id
            lines.extend(_wrap("%s switched off by %s (%s) — the card would have answered it"
                               % (off, spec.config_key, source), cols, indent=len(off)))
    return lines


def _section_block(section, card, *, cols: int, cap: int, show_evidence: bool,
                   show_links: bool) -> list[str]:
    """One figure per line: its value, its buttons, and up to `card.max_evidence` proof rows."""
    lines = _wrap("  %s %s · %s · %s" % (RULE, section.title, section.state, section.owner),
                  cols, indent=2)
    if section.state == STATE_UNAVAILABLE:
        # The detail block of a section that could not be measured is its apology: the reason in
        # full, and the verb that would fill it. This is the half of AC 3 the summary block
        # cannot carry once the reasons are longer than a line.
        lines.extend(_wrap("  why: %s" % section.reason, cols, indent=6))
        lines.extend(_wrap("  fill it: %s" % section.run, cols, indent=9))
        return lines
    if section.lead:
        lines.extend(_wrap("  %s" % section.lead, cols, indent=2))
    if section.confidence is not None or section.basis:
        stated = ("confidence %s" % card.policy.state(section.confidence)
                  if section.confidence is not None else "confidence not stated")
        lines.extend(_wrap("  %s%s" % (stated, " · basis: %s" % section.basis if section.basis
                                       else ""), cols, indent=2))
    shown = section.figures[:cap]
    for figure in shown:
        value = "    %s%s" % (_pad(figure.label), figure.value)
        lines.extend(_wrap(value, cols, indent=len("    %s" % _pad(figure.label))))
        if figure.note:
            lines.extend(_wrap("      (%s)" % figure.note, cols, indent=6))
        if show_links:
            for item in figure.links:
                lines.extend(_wrap("      → %s" % item.render(), cols, indent=6))
        if show_evidence:
            limit = card.policy.integer("max_evidence")
            for item in figure.links:
                for row in card.evidence.resolve(item).lines[:limit]:
                    lines.extend(_wrap("      · %s" % row, cols, indent=6))
    hidden = len(section.figures) - len(shown)
    if hidden > 0:
        lines.extend(_wrap("    ... %d more figure%s in this section: fathom card figures %s "
                           "--section %s" % (hidden, "y" if hidden == 1 else "s",
                                             card.episode_id, section.id),
                         cols, indent=8))
    for note in section.notes:
        lines.extend(_wrap("    note: %s" % note, cols, indent=10))
    for detail in section.detail:
        lines.extend(_wrap("    · %s" % detail, cols, indent=6))
    return lines


# ---------------------------------------------------------------------------
# the auditor
# ---------------------------------------------------------------------------
def audit(lines, *, card=None, cols: int = DEFAULT_COLS, brief: bool = False,
          max_figures: int | None = None) -> Render:
    """Re-read the render and fail it when it lost a fact the card had.

    The checks are the render's own promise, not a general "is the card good": a brief render
    promises one line per section (plus a reason and a fill verb for each gap) and no figures, so
    a brief render is not failed for the figures it never claimed to print. A full render promises
    the figures, their buttons, the confidence each one carries, the remainder each cap left
    behind, the reason behind each gap, the key behind each switch and the digest at the bottom —
    and loses the audit the moment any one of them goes missing.
    """
    problems: list[str] = []
    if card is None:
        return Render(lines=list(lines),
                      problems=["audit refused: no card to audit against"])
    flat = " ".join(" ".join(lines).split())
    policy = card.policy
    cap = policy.integer("max_figures") if max_figures is None else max_figures

    def require(text: str, problem: str) -> None:
        if text not in flat:
            problems.append(problem)

    for section in card.sections:
        # The summary block is the part that fits a screen; a section that is not in it is a
        # section the reader never learns is missing.
        pattern = re.compile(r"^ *[~x ?]?\s*%s\s" % re.escape(section.id))
        if not any(pattern.match(line) for line in lines):
            problems.append("the summary block has no line for section %s" % section.id)
        if not brief:
            # Each section opens with its own heading — title, state, owner. A section that lost
            # its heading lost its block, figures or no figures.
            require("%s · %s · %s" % (section.title, section.state, section.owner),
                    "section %s lost its heading" % section.id)
        if section.state == STATE_UNAVAILABLE:
            require(section.short or section.reason,
                    "section %s is unavailable without saying what is missing" % section.id)
            require("why: %s" % section.reason,
                    "section %s is unavailable without saying why" % section.id)
            require("fill it: %s" % section.run,
                    "section %s is unavailable without naming the verb that fills it"
                    % section.id)
            continue
        require(section.brief,
                "section %s lost its summary line (%r)" % (section.id, section.brief))
        if brief:
            continue
        if section.confidence is not None:
            require(policy.state(section.confidence),
                    "section %s states no confidence" % section.id)
        for figure in section.figures[:cap]:
            require(figure.value,
                    "%s/%s: the figure value vanished from the render (%r)"
                    % (section.id, figure.label, figure.value))
            for item in figure.links:
                require(item.render(), "%s/%s: a figure lost its button (%s)"
                        % (section.id, figure.label, item.render()))
        left = len(section.figures) - min(len(section.figures), cap)
        if left > 0:
            require("more figure",
                    "section %s trimmed %d figure(s) without naming the remainder"
                    % (section.id, left))
    if not brief:
        for section_id, source in card.hidden:
            spec = SECTION_INDEX[section_id]
            require(spec.config_key,
                    "section %s was switched off and the render did not say which key did it"
                    % section_id)
        for note in card.notes:
            require(note, "a composition note vanished from the render: %s" % note[:60])
    require(card.digest(),
            "the render carries no card digest, so its byte-determinism is unverifiable")
    return Render(lines=list(lines), problems=problems)


# ---------------------------------------------------------------------------
# layout helpers
# ---------------------------------------------------------------------------
def _pad(label: str, width: int = 24) -> str:
    """Labels are padded, never clipped: a clipped label would be a clipped question."""
    if len(label) <= width:
        return "%-*s " % (width, label)
    return "%s " % label




def _wrap(text: str, cols: int = DEFAULT_COLS, indent: int = 6) -> list[str]:
    """Fold a line to the width, hanging every continuation under the label it belongs to.

    Words are never dropped and lines are never clipped: a fold that loses a word loses a claim,
    and the auditor compares text, so the fold has to be lossless.
    """
    return _textwrap.wrap(text, width=max(24, cols), subsequent_indent=" " * max(0, indent),
                          break_long_words=False, break_on_hyphens=False) or [""]
