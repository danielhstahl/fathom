"""The reformat detector: did this file change, or did it merely get re-laid-out?

The question this answers, per changed file:

    **claim** — `reformat` · `content` · `undetermined` · `identical` · `unavailable`

and, for `reformat`, **how strong** the claim is and **what the formatter touched**. Two
strengths, deliberately:

* `tokens` — the strong claim, from :mod:`~recommender.noise.norm`: the token streams are
  identical, which covers re-indentation *and* re-wrapping, trailing commas, quote
  delimiters, arrow parentheses and import order. This is the one that lets a rustfmt or
  prettier commit render as *0 semantic changes*.
* `whitespace` — the fallback when there is no token model for the language, or the
  scanner refused: every changed line differs only in whitespace, and no line was added or
  removed except blank ones. Weaker, still honest, and language-independent, so a
  re-indented `.proto` or `.graphql` is not automatically a "semantic" change.

`undetermined` exists because the honest answer to "a formatter re-wrapped this, or did a
person edit it?" is sometimes *I cannot tell*: the strong claim is unavailable (no model,
truncated blob, unparsable revision) and the weak claim sees a difference. That is
reported as a thing the tool cannot say, with the reason, and **never suppressed**. A
suppression that is 90 % right is a reviewer who stops looking at a class of diffs; a
refusal costs one glance.

`content` is a *definite* answer: token streams differ, so something other than layout
moved. That includes the cases a formatter does genuinely change tokens and this module
conservatively counts as content (shfmt's `${x}` quoting, a YAML block sequence becoming a
flow sequence, a Rust raw string). Favouring false work over false silence is the whole
policy of this package: `eval.py` gates precision, not recall.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from . import norm

#: The claims. `reformat` and `content` are the two that gate suppression; the rest are
#: reasons a suppression is not available, each with its own sentence in the UI.
REFORMAT = "reformat"
CONTENT = "content"
UNDETERMINED = "undetermined"
IDENTICAL = "identical"
UNAVAILABLE = "unavailable"

CLAIMS: tuple[str, ...] = (REFORMAT, CONTENT, UNDETERMINED, IDENTICAL, UNAVAILABLE)

#: Which note prefixes from :mod:`norm` name a kind of formatting difference, and what the
#: reviewer is told about it. Printed verbatim under the claim, so a claim is never a bare
#: word: the AC's "with the evidence for that claim".
NOTE_KINDS: tuple[tuple[str, str], ...] = (
    ("trailing comma", "trailing commas"),
    ("arrow paren", "arrow parentheses"),
    ("import line", "import order"),
    ("comment", "comment placement"),
    ("line-token stream", "indentation and padding"),
    ("verbatim block", "verbatim blocks kept exactly"),
    ("word stream", "paragraph re-wrapping"),
    ("parsed as JSON", "JSON whitespace"),
)

#: A pair needs at least this many changed lines before "bulk regeneration" is said by
#: `generated.py`; here it only colours the evidence wording.
BULK_LINES = 40


@dataclass
class Reformat:
    """The verdict for one file pair, with the receipts for it."""

    path: str
    claim: str = UNAVAILABLE
    strength: str = ""                            # "tokens" | "whitespace" | ""
    language: str = "unknown"
    language_title: str = "unknown"
    formatter: str = ""
    strategy: str = "none"
    tokens: int = 0
    comments: int = 0
    digest: str = ""
    kinds: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    refusals: list[str] = field(default_factory=list)
    stats: norm.LineStats = field(default_factory=norm.LineStats)
    residual: list[str] = field(default_factory=list)
    changed_lines: int = 0
    change_type: str = "modified"

    # -- the answers every consumer asks ---------------------------------
    @property
    def is_reformat(self) -> bool:
        return self.claim == REFORMAT

    @property
    def semantic_lines(self) -> int:
        """Lines carrying meaning: zero exactly when the claim is `reformat`."""
        return 0 if self.claim in (REFORMAT, IDENTICAL) else self.changed_lines

    @property
    def reason(self) -> str:
        """One sentence, with the strength in it, for a collapsed row."""
        if self.claim == REFORMAT:
            where = "token streams identical" if self.strength == "tokens" \
                else "every changed line differs only in whitespace"
            return "%s-only change: %s (%s, %s)" % (
                self.formatter or "formatting", where, self.language_title,
                ", ".join(self.kinds) if self.kinds else "layout only")
        if self.claim == IDENTICAL:
            return "the two revisions are byte-identical: this is a move or a mode change"
        if self.claim == UNAVAILABLE:
            return "; ".join(self.refusals) or "one side of the pair is not available"
        if self.claim == UNDETERMINED:
            return ("cannot tell a re-layout from a change: %s"
                    % ("; ".join(self.refusals) or "no token model"))
        return "the token streams differ: %d line(s) carry change" % self.changed_lines

    def evidence(self) -> list[str]:
        """The lines printed under the claim. Non-empty for every claim, always."""
        out: list[str] = []
        if self.claim in (REFORMAT, IDENTICAL):
            if self.strength == "tokens":
                out.append("%s: %d token(s), %d comment(s) identical across the pair "
                           "(pair %s)" % (self.language_title, self.tokens, self.comments,
                                          self.digest))
            else:
                out.append("line-level: %d changed line(s), all whitespace-only (pair %s)"
                           % (self.changed_lines, self.digest))
            if self.formatter:
                out.append("consistent with a %s run: %s"
                           % (self.formatter, self.formatter_note() or "whitespace is its "
                                                                       "only output"))
            if self.kinds:
                out.append("normalised: %s" % ", ".join(self.kinds))
            if self.notes:
                out.extend("normaliser: %s" % note for note in self.notes)
            if self.stats.blank_only or self.stats.crlf_before != self.stats.crlf_after:
                out.append("line endings %s→%s, %d blank line(s) moved"
                           % ("CRLF" if self.stats.crlf_before else "LF",
                              "CRLF" if self.stats.crlf_after else "LF",
                              self.stats.blank_only))
        elif self.claim == CONTENT:
            out.append("%s: %d of %d changed line(s) differ beyond layout"
                       % (self.language_title, self.changed_lines, self.stats.changed_lines))
            out.extend("line %s" % line for line in self.residual[:6])
        elif self.claim == UNDETERMINED:
            out.append("strong claim unavailable: %s" % ("; ".join(self.refusals) or "n/a"))
            out.append("line-level difference remains: %d changed line(s)"
                       % self.changed_lines)
        else:
            out.append("; ".join(self.refusals) or "one side of the pair is missing")
        return out

    def formatter_note(self) -> str:
        from .norm import LANGUAGES
        lang = LANGUAGES.get(self.language)
        return lang.notes if lang else ""

    def as_dict(self) -> dict:
        return {"path": self.path, "claim": self.claim, "strength": self.strength,
                "language": self.language, "formatter": self.formatter,
                "strategy": self.strategy, "tokens": self.tokens, "comments": self.comments,
                "digest": self.digest, "kinds": list(self.kinds), "notes": list(self.notes),
                "refusals": list(self.refusals), "changed_lines": self.changed_lines,
                "semantic_lines": self.semantic_lines, "reason": self.reason,
                "change_type": self.change_type, "stats": self.stats.as_dict(),
                "residual": list(self.residual[:6])}


def compare(path: str, before: str | None, after: str | None, *,
            change_type: str = "modified") -> Reformat:
    """Compare two revisions of one path and make (or refuse) the reformat claim."""
    verdict = Reformat(path=path, change_type=change_type)
    lang = norm.language_for(path)
    if lang is not None:
        verdict.language, verdict.language_title, verdict.formatter = \
            lang.id, lang.title, lang.formatter

    if change_type in ("added", "created"):
        verdict.refusals.append("the file is added: there is no earlier revision to compare")
        verdict.claim = UNAVAILABLE
        return verdict
    if change_type == "deleted":
        verdict.refusals.append("the file is deleted: there is no later revision to compare")
        verdict.claim = UNAVAILABLE
        return verdict
    if before is None or after is None:
        verdict.refusals.append("both revisions' content is needed and one is unavailable")
        verdict.claim = UNAVAILABLE
        return verdict

    stats = norm.line_stats(before, after)
    verdict.stats = stats
    verdict.changed_lines = stats.changed_lines

    if before == after:
        verdict.claim = IDENTICAL
        verdict.kinds.append("byte-identical")
        return verdict

    old = norm.normalize(path, before)
    new = norm.normalize(path, after)
    verdict.language = new.language if new.language != "unknown" else old.language
    verdict.strategy = new.strategy if new.strategy != "none" else old.strategy
    verdict.refusals = [text for text in (*old.refusals, *new.refusals)]
    verdict.notes = _merged(old.notes, new.notes)
    verdict.kinds = _kinds_of(verdict.notes, stats, old, new)

    if norm.same_tokens(old, new):
        verdict.claim = REFORMAT
        verdict.strength = "tokens"
        verdict.tokens = new.tokens
        verdict.comments = new.comment_tokens
        verdict.digest = norm.digest_pair(old, new)
        if old.lines != new.lines:
            label = "line wrapping"
            if not any(kind.startswith(label) for kind in verdict.kinds):
                verdict.kinds.append("%s (%d → %d lines)" % (label, old.lines, new.lines))
        return verdict

    strong = old.claimable and new.claimable and old.strategy == new.strategy
    if not strong and not stats.carries_change:
        # No token model (or a refusal), but nothing but whitespace moved: the weak claim.
        verdict.claim = REFORMAT
        verdict.strength = "whitespace"
        verdict.digest = norm.digest_pair(old, new)
        verdict.tokens = new.tokens
        verdict.comments = new.comment_tokens
        if not any(kind.startswith("indentation") for kind in verdict.kinds):
            verdict.kinds.append("indentation")
        return verdict
    if strong:
        verdict.claim = CONTENT
    else:
        verdict.claim = UNDETERMINED

    verdict.residual = list(stats.samples)
    verdict.changed_lines = _residual_lines(stats)
    verdict.tokens = new.tokens
    verdict.comments = new.comment_tokens
    verdict.digest = norm.digest_pair(old, new)
    return verdict


def _residual_lines(stats: norm.LineStats) -> int:
    """Changed lines that are not explained by layout."""
    return max(1, (stats.changed_pairs - stats.ws_only_pairs)
               + (stats.inserted + stats.deleted - stats.blank_only))


def _kinds_of(notes: list[str], stats: norm.LineStats, old: norm.Normal,
              new: norm.Normal) -> list[str]:
    kinds: list[str] = []
    for prefix, label in NOTE_KINDS:
        if any(note.startswith(prefix) for note in notes) and label not in kinds:
            kinds.append(label)
    if stats.ws_only_pairs:
        kinds.append("indentation (%d line(s))" % stats.ws_only_pairs)
    if stats.wrap_only and "line wrapping" not in kinds:
        kinds.append("line wrapping")
    if stats.crlf_before != stats.crlf_after:
        kinds.append("line endings (CRLF↔LF)")
    return kinds


def _merged(a: list[str], b: list[str]) -> list[str]:
    out: list[str] = []
    for note in (*a, *b):
        if note not in out:
            out.append(note)
    return out
