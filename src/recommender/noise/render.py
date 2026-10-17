"""Rendering the fold, and the audit that says nothing was hidden silently.

Two jobs, in one module on purpose: the renderer produces the lines, and :func:`audit`
proves the *invariant about* those lines. They live together because an invariant you can
only check by reading prose is an invariant that rots the first time somebody rewords a
row.

The invariant, from the ticket's third acceptance criterion:

    **no content is hidden without a visible reason**

which decomposes into four checkable claims:

1. every line that reports a fold carries :data:`FOLD_MARK` and with it the key that
   reveals the content (`[press b to show it]`), so "one keypress away" is on screen and
   not in a manual;
2. every such line names the rule *and the layer that configured it*
   (`policy: repo layer (fathom.toml)`), so the reader can change it without asking anyone;
3. every fold carries at least one piece of evidence, printed verbatim under the fold —
   the evidence is not metadata behind a second keypress;
4. expanding never loses a file: every path in the episode appears in the expanded render.

Those are asserted twice over — structurally, against the objects, and textually, against
the rendered lines — and `tests/test_noise_suppression_contract.py` runs the audit over
every golden it blesses, so a rewording that quietly drops the key or the policy fails CI
instead of shipping a blank. Long lines are **wrapped, never clipped**: a fold report whose
`[press b]` fell off the right-hand edge is exactly the silent hiding the AC refuses, so the
renderer wraps to the pane width and keeps every word.
"""

from __future__ import annotations

import textwrap as _textwrap
from dataclasses import dataclass, field

from .episode import EpisodeNoise
from .policy import FOLD_MARK, RULE_INDEX, Suppression

__all__ = ["FOLD_MARK", "Audit", "DEFAULT_COLS", "audit", "audit_view", "collapsed_lines",
           "expanded_lines", "fit", "fold_lines", "goldens", "policy_block", "render",
           "wrap"]

DEFAULT_COLS = 100


@dataclass
class Audit:
    """The result of auditing a rendering. `problems` empty means the invariant held."""

    problems: list[str] = field(default_factory=list)
    checked_lines: int = 0
    folds_seen: int = 0

    @property
    def ok(self) -> bool:
        return not self.problems

    def as_dict(self) -> dict:
        return {"ok": self.ok, "checked_lines": self.checked_lines,
                "folds_seen": self.folds_seen, "problems": list(self.problems)}


# ---------------------------------------------------------------------------
# text layout
# ---------------------------------------------------------------------------
def wrap(text: str, cols: int = DEFAULT_COLS, indent: int = 0) -> list[str]:
    """Fold text to the pane width, keeping every word. Continuation lines are indented."""
    pad = " " * indent
    return _textwrap.wrap(text, width=max(24, cols), initial_indent=pad,
                          subsequent_indent=pad, drop_whitespace=True,
                          break_long_words=False, break_on_hyphens=False) or [""]


def fit(text: str, cols: int = DEFAULT_COLS) -> str:
    """One line, clipped. Only for lines that must stay on one line and carry no fold."""
    if len(text) <= cols:
        return text
    cut = text[:cols - 1]
    if " " in cut[-12:]:
        cut = cut[:cut.rfind(" ")]
    return cut + "…"


# ---------------------------------------------------------------------------
# the renderer
# ---------------------------------------------------------------------------
def render(view: EpisodeNoise, *, expanded: bool = False,
           cols: int = DEFAULT_COLS) -> list[str]:
    """The pane's lines for one episode: folded, or expanded with the same reasons printed."""
    return expanded_lines(view, cols=cols) if expanded else collapsed_lines(view, cols=cols)


def collapsed_lines(view: EpisodeNoise, *, cols: int = DEFAULT_COLS) -> list[str]:
    """What an episode looks like when the noise has been folded."""
    counts = view.counts()
    # The session note belongs on the header of the *folded* view too: "nothing is folded
    # because you pressed b" is the difference between an override and a bug.
    out = wrap(view.summary_line() + _session_note(view), cols)
    if view.collapse:
        out.extend(fold_lines(view.collapse, cols=cols))
        return out
    for item in view.files:
        if item.folded and item.suppression:
            out.extend(fold_lines(item.suppression, cols=cols, indent=2))
        else:
            out.append(fit("  %s  %d changed line(s)" % (item.path, item.changed_lines), cols))
            if item.reformat and not item.reformat.is_reformat:
                out.extend(wrap("    %s" % item.reformat.reason, cols, indent=4))
            for note in item.notes:
                out.extend(wrap("    · %s" % note, cols, indent=4))
    if not counts["files"]:
        out.append("  this episode changes no files")
    return out


def expanded_lines(view: EpisodeNoise, *, cols: int = DEFAULT_COLS) -> list[str]:
    """Everything visible — and the policy that *would* have hidden it still printed.

    This is the state after `b`. The AC's other half: the raw content is one keypress away,
    and the reason it was folded stays on screen beside it rather than being replaced by it.
    """
    counts = view.counts()
    out = wrap("%s · shown (nothing folded) · %d file(s), %d changed line(s)%s"
               % (view.summary_line(), counts["files"], counts["changed_lines"],
                  _session_note(view)), cols)
    key = view.policy.expand_key
    for item in view.files:
        out.extend(wrap("  %s  %d changed line(s)" % (item.path, item.changed_lines), cols,
                        indent=2))
        if item.folded and item.suppression:
            out.extend(wrap("· shown because you asked (%s): `%s` would fold this — %s"
                            % (key, item.suppression.rule, item.suppression.reason),
                            cols, indent=4))
        for line in item.detail:
            out.extend(wrap("· " + line, cols, indent=4))
        if not item.detail and not item.folded:
            out.extend(wrap("· no detector fired: this is ordinary change", cols, indent=4))
    if view.collapse:
        out.extend(wrap("· the whole episode would collapse to one line (`%s` folds it "
                        "back): %s" % (key, view.collapse.reason), cols, indent=2))
    if not counts["files"]:
        out.append("  this episode changes no files")
    return out


def fold_lines(suppression: Suppression, *, cols: int = DEFAULT_COLS,
               indent: int = 0) -> list[str]:
    """One fold: the header (never wrapped: its key and policy must survive a narrow pane),
    then the reason and the evidence, wrapped."""
    pad = " " * indent
    out = [pad + suppression.render()]
    out.extend(wrap(suppression.reason_line(), cols, indent=indent + 2))
    for line in suppression.evidence:
        out.extend(wrap("· %s" % line, cols, indent=indent + 2))
    return out


def policy_block(view: EpisodeNoise, *, cols: int = DEFAULT_COLS) -> list[str]:
    """The footer strip: what is folded, by which rule, and how to stop it."""
    out = ["suppression policy"]
    for line in view.policy.describe()[1:]:
        out.extend(wrap("  " + line, cols, indent=2))
    key = view.policy.expand_key
    out.extend(wrap("  the key: %s shows what a rule folded and folds it again; the file: "
                    "`noise.suppress.*` in fathom.toml; the session: %s changes nothing on "
                    "disk" % (key, key), cols, indent=2))
    return out


def _session_note(view: EpisodeNoise) -> str:
    if view.policy.session.active and view.policy.session.suppress is False:
        return " · session override active (`%s` pressed: nothing is folded)" \
               % view.policy.session.pressed
    return ""


# ---------------------------------------------------------------------------
# the audit
# ---------------------------------------------------------------------------
def audit(lines: list[str] | str, *, view: EpisodeNoise | None = None) -> Audit:
    """Prove the four claims. Returns problems; an empty list means the render is honest."""
    if isinstance(lines, str):
        lines = lines.splitlines()
    report = Audit(checked_lines=len(lines))
    for index, line in enumerate(lines):
        if FOLD_MARK not in line:
            continue
        report.folds_seen += 1
        if "[press " not in line:
            report.problems.append("a fold names no expand key: %s" % line.strip())
        if "policy:" not in line:
            report.problems.append("a fold names no policy: %s" % line.strip())
        if "folded (" not in line:
            report.problems.append("a fold names no rule: %s" % line.strip())
        block = _block_of(lines, index)
        if not any(item.lstrip().startswith("\u2014") for item in block):
            report.problems.append("a fold prints no reason: %s" % line.strip())
        if not any(item.lstrip().startswith("·") for item in block):
            report.problems.append("a fold prints no evidence: %s" % line.strip())
    if view is not None:
        report.problems.extend(audit_view(view))
    return report


def _block_of(lines: list[str], start: int) -> list[str]:
    """The lines belonging to the fold whose header is at `start`: header, reason, evidence.

    A block ends at the next fold, or at the first non-blank line that is not *deeper* than
    the header — which is how a fold's own wrapped reason (indent 4) stays in the block while
    the next file's row (same indent as the header) does not.
    """
    header = lines[start]
    indent = len(header) - len(header.lstrip())
    block = [header]
    for line in lines[start + 1:]:
        if FOLD_MARK in line:
            break
        if line.strip() and (len(line) - len(line.lstrip())) <= indent:
            break
        block.append(line)
        if len(block) > 60:
            break
    return block


def audit_view(view: EpisodeNoise) -> list[str]:
    """The structural half: every fold has reason, evidence and key; expansion loses nothing."""
    problems: list[str] = []
    for item in view.files:
        if not item.folded:
            continue
        suppression = item.suppression
        if suppression is None:
            problems.append("%s is folded with nothing to show for it" % item.path)
            continue
        if not suppression.reason.strip():
            problems.append("%s is folded without a reason" % item.path)
        if not suppression.evidence:
            problems.append("%s is folded without evidence" % item.path)
        if not suppression.expand:
            problems.append("%s is folded without an expand key" % item.path)
        if suppression.rule not in RULE_INDEX:
            problems.append("%s names an unknown rule %r" % (item.path, suppression.rule))
    if view.collapse is not None:
        if not view.collapse.reason.strip():
            problems.append("the episode collapsed without a reason")
        if not view.collapse.evidence:
            problems.append("the episode collapsed without evidence")

    collapsed_text = "\n".join(collapsed_lines(view))
    for line in collapsed_text.splitlines():
        if FOLD_MARK in line and ("[press " not in line or "policy:" not in line):
            problems.append("a fold line lost its key or policy: %s" % line.strip())
    for item in view.files:
        if item.path not in "\n".join(expanded_lines(view)):
            problems.append("%s disappears when the episode is expanded" % item.path)
        if not (item.folded and item.suppression):
            continue
        if view.collapse is not None:
            # The episode collapsed, so the per-file folds are behind the episode line.
            continue
        tail = " · %s folded (" % item.path
        if tail not in collapsed_text:
            problems.append("%s's fold is not printed" % item.path)
    return problems


def goldens(view: EpisodeNoise, *, cols: int = DEFAULT_COLS) -> dict[str, str]:
    """The blessed text block for one episode: every state, as one string per state."""
    return {"collapsed": "\n".join(collapsed_lines(view, cols=cols)) + "\n",
            "expanded": "\n".join(expanded_lines(view, cols=cols)) + "\n",
            "policy": "\n".join(policy_block(view, cols=cols)) + "\n"}
