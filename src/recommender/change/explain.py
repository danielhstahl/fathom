"""Rendering a classification: what a reviewer reads, and what the TUI is handed instead.

Same contract as `recommender.taxonomy.explain`: text for eyes, `--json` for the surfaces,
and neither allowed to contain something the other does not have. A detector, a pane and a
CI log must be able to quote the *same* sentence, which is why the prose lives here rather
than being re-derived in each consumer.

The UI reachability AC asks for is this module's job: `badge()` is the tree-pane label
(ADR-006 §6.2 groups the diff tree by taxonomy class), `detail()` is the one-sentence
reason, and both are also fields of `Classification.as_dict()`, so the TUI never parses a
line to get them. `ui_block_text` prints them the way a pane would, which is what the
contract test renders.
"""

from __future__ import annotations

from recommender.taxonomy import classes as C

from .episode import EpisodeClassification

RULE_WIDTH = 10
SUMMARY_HEADER = "%-46s %-11s %-6s %-10s %s" % ("path", "class", "conf", "band", "why")


def _clip(text: str, width: int) -> str:
    text = " ".join(str(text).split())
    return text if len(text) <= width else text[:max(1, width - 1)] + "…"


def short_reason(decision, width: int = 62) -> str:
    """The one-line claim id for a table row: `via marker:gen.protoc`, `via rule:gen.protobuf`.

    A table row that repeats the sentence is a row nobody reads; a table row that names the
    rule or marker is a row a reviewer can grep, and the full sentence is one keypress (or
    one `explain`) away.
    """
    winner = decision.winner or {}
    source = winner.get("source", "")
    if not source:
        return _clip(decision.reason.split(";")[0], width)
    label = "path rule" if source.startswith("rule:") else source
    if decision.overrode_path:
        label += " (content overruled the path)"
    return _clip("via %s" % label, width)


def row_line(decision) -> str:
    """One line per path, for `fathom change explain -q` and for a file list."""
    return "%-46s %-11s %-6.2f %-10s %s" % (
        _clip(decision.path, 46), decision.cls, decision.confidence, decision.band,
        short_reason(decision))


def table_text(decisions) -> str:
    lines = [SUMMARY_HEADER]
    for decision in decisions:
        lines.append(row_line(decision))
    return "\n".join(lines) + "\n"


def explain_text(decisions) -> str:
    """The full receipts for one or more paths: class, why, path rule, scores, evidence."""
    blocks: list[str] = []
    for decision in decisions:
        lines: list[str] = []
        lines.append("path    %s" % decision.path)
        lines.append("CLASS   %s — confidence %.2f (%s)%s"
                     % (decision.cls, decision.confidence, decision.band,
                        "  [from cache]" if decision.cached else ""))
        lines.append("  why        %s" % decision.reason)
        lines.append("  path       %s ← rule %s (layer %s, taxonomy v%s)"
                     % (decision.path_class or "—", decision.path_rule or "none",
                        decision.path_layer or "—", decision.taxonomy_version))
        if decision.scores:
            lines.append("  scores     %s" % " · ".join("%s %.2f" % (cls, score)
                                                        for cls, score in decision.scores))
        if decision.votes:
            lines.append("  evidence   %d claim%s:" % (len(decision.votes),
                                                       "" if len(decision.votes) == 1 else "s"))
            for vote in decision.votes:
                lines.append("             %-10s %-24s %.2f  %s"
                             % (vote.get("class", ""), _clip(vote.get("source", ""), 24),
                                vote.get("weight", 0.0), _clip(vote.get("note", ""), 70)))
        lines.append("  noise      %s (%.2f) — rule %s" % (decision.noise, decision.noise_score,
                                                          decision.noise_rule or "—"))
        facts = decision.facts or {}
        if facts.get("available"):
            lines.append("  content    text=%s size=%s lines=%s parsed=%s%s"
                         % (facts.get("is_text"), facts.get("size"), facts.get("lines"),
                            facts.get("parsed") or "—",
                            " minified" if facts.get("minified") else ""))
            for hit in (facts.get("markers") or [])[:4]:
                lines.append("             marker %-14s line %-3s %s"
                             % (hit.get("marker"), hit.get("line"), _clip(hit.get("matched", ""), 40)))
        else:
            lines.append("  content    unavailable — classified from the path alone")
        for note in decision.notes:
            lines.append("  note       %s" % note)
        lines.append("  ui         badge %r · detail %r" % (decision.badge(),
                                                            _clip(decision.detail(), 78)))
        blocks.append("\n".join(lines))
    return "\n\n".join(blocks) + "\n"


def episode_text(episode: EpisodeClassification, *, show: int = 0) -> str:
    """The episode view: the summary line, the class table, and the honest-tail list."""
    lines = [episode.summary_line(), ""]
    counts = episode.class_counts()
    lines.append("classes: " + ", ".join("%s=%d" % kv for kv in counts.items()))
    lines.append("noise:   " + ", ".join("%s=%d" % kv for kv in episode.noise_counts().items()))
    lines.append("bands:   " + ", ".join("%s=%d" % kv for kv in episode.band_counts().items())
                 + "   test-surface ratio %.2f" % episode.test_surface_ratio())
    if episode.overridden():
        lines.append("")
        lines.append("content overruled the path rule on %d path%s:"
                     % (len(episode.overridden()), "" if len(episode.overridden()) == 1 else "s"))
        for decision in episode.overridden():
            lines.append("  %-44s %s ← path said %s" % (decision.path, decision.cls,
                                                        decision.path_class))
    low = episode.low_confidence()
    if low:
        lines.append("")
        lines.append("below 0.70 confidence (%d) — the reviewer should look here first:" % len(low))
        for decision in low[:12]:
            lines.append("  %-44s %-10s %.2f %s" % (decision.path, decision.cls,
                                                    decision.confidence, decision.band))
    unclassified = episode.unclassified()
    if unclassified:
        lines.append("")
        lines.append("no rule matched (%d) — `other` is the answer, and a missing row in the "
                     "table is the fix:" % len(unclassified))
        for decision in unclassified:
            lines.append("  %-44s %s" % (decision.path, short_reason(decision, 58)))
    if show:
        lines.append("")
        lines.append(table_text(episode.decisions[:show]))
    return "\n".join(lines) + "\n"


def vocabulary_text() -> str:
    """The twelve classes with their meaning — the same table `fathom taxonomy classes` prints."""
    lines = ["%-11s %-11s %s" % ("class", "default noise", "meaning")]
    for name in C.PATH_CLASSES:
        info = C.CLASS_INFO[name]
        lines.append("%-11s %-11s %s" % (name, info["noise"], info["doc"]))
    return "\n".join(lines) + "\n"


def ui_block_text(decision) -> str:
    """Exactly what a pane gets: the fields, not a paragraph."""
    return ("badge:  %s\n"
            "detail: %s\n"
            "title:  %s\n"
            "color:  %s\n" % (decision.badge(), decision.detail(),
                              "%s (%s)" % (decision.cls, decision.band),
                              "noise" if decision.is_noise else "signal"))
