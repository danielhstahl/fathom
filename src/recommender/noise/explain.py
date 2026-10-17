"""Prose for one verdict, one policy, one episode — the `explain` half of the verb.

`fathom change explain` and `fathom taxonomy explain` exist because a label nobody can
question is a label nobody trusts; this is the same artifact for the noise decision. Three
shapes, all derived from the same objects the UI reads, so the CLI, the pane and the export
cannot tell three stories about one fold:

* :func:`explain_pair` — one file pair: the claim, its strength, what was normalised, what
  stopped the stronger claim, and what the detector would fold.
* :func:`explain_policy` — the eight rules with their config key, winning layer, expand key
  and session state.
* :func:`explain_episode` — one episode end to end: the fold lines, the refusals, and the
  lines that stay visible with the reason each stays visible.

Nothing here renders a fold without its reason: every function ends by delegating the fold
lines to :meth:`recommender.noise.policy.Suppression.block`, which is the only supported
way to print one.
"""

from __future__ import annotations

from . import generated as G
from . import lockfile as L
from . import norm, render
from .episode import Episode, EpisodeNoise, FileNoise, suppress
from .policy import RULES, Policy
from .reformat import Reformat, compare


def explain_pair(path: str, before: str | None, after: str | None, *,
                 change_type: str = "modified") -> list[str]:
    """One file pair, in the order a reviewer reads it."""
    verdict: Reformat = compare(path, before, after, change_type=change_type)
    out = [f"{path}: {verdict.claim}" + (f" ({verdict.strength})" if verdict.strength else "")]
    out.append(f"  reason     {verdict.reason}")
    if verdict.language != "unknown":
        language = norm.LANGUAGES.get(verdict.language)
        out.append(f"  language   {verdict.language_title} "
                   f"({verdict.strategy}; formatter {verdict.formatter})")
        if language and language.notes:
            out.append(f"  model      {language.notes}")
    else:
        out.append("  model      no token model: only the line-level claim is available")
    out.append(f"  counts     {verdict.changed_lines} changed line(s), "
               f"{verdict.semantic_lines} semantic, {verdict.tokens} token(s)")
    if verdict.digest:
        out.append(f"  digest     pair {verdict.digest}")
    for line in verdict.evidence():
        out.append(f"  · {line}")
    if verdict.refusals:
        out.append("  refused    " + "; ".join(verdict.refusals))
    return out


def explain_generated(path: str, before: str | None, after: str | None, *,
                      path_class: str = "generated", band: str = "confirmed",
                      hint: str = "") -> list[str]:
    verdict = G.inspect(path, before, after, path_class=path_class, band=band,
                        generator_hint=hint)
    out = [f"{path}: {verdict.claim}" + (f" ({verdict.kind})" if verdict.kind else ""),
           f"  reason     {verdict.reason}",
           f"  counts     {verdict.changed_lines} of {verdict.file_lines} line(s) "
           f"({verdict.bulk_ratio:.0%}), {verdict.stamp_lines} stamp line(s)",
           f"  floors     bulk needs ≥{G.BULK_MIN_LINES} changed lines and "
           f"≥{G.BULK_RATIO:.0%} of the file; a re-order needs "
           f"≥{G.RESORT_MIN_LINES} lines on both sides"]
    out.extend(f"  · {line}" for line in verdict.evidence)
    out.extend(f"    {line}" for line in verdict.samples)
    return out


def explain_lock(path: str, before: str | None, after: str | None, *,
                 changed_paths: list[str] | None = None) -> list[str]:
    summary = L.summarize(path, before, after, changed_paths=changed_paths or [path])
    out = [f"{path}: {summary.headline()}"]
    out.append(f"  reason     {summary.reason()}")
    if summary.note:
        out.append(f"  format     {summary.format or '—'} — {summary.note}")
    for row in summary.rows[:12]:
        out.append(f"  · {row['kind']:<10} {row['name']} "
                   f"{row['from'] or '—'} → {row['to'] or '—'}")
    if len(summary.rows) > 12:
        out.append(f"  · and {len(summary.rows) - 12} more")
    return out


def explain_policy(policy: Policy) -> list[str]:
    """The policy as a table a reviewer can act on."""
    out = policy.describe()
    out.append("")
    out.append("rules:")
    for rule in RULES:
        out.append("  %-16s %s" % (rule.id, rule.doc))
    out.append("")
    out.append("session: the keystroke is `%s`; it turns folding off for this process only, "
               "writes nothing, and never overrides a repository's own `= false`."
               % policy.expand_key)
    return out


def explain_episode(view: EpisodeNoise, *, expanded: bool = False) -> list[str]:
    """An episode, top to bottom, with every fold carrying its reason."""
    return render.render(view, expanded=expanded)


def explain_file_noise(item: FileNoise) -> list[str]:
    """One file's row in full: what fired, what folded, what refused and why."""
    out = [f"{item.path}: class {item.cls or '—'}, noise {item.noise}, band {item.band or '—'}"]
    if item.reformat:
        out.append(f"  reformat   {item.reformat.claim} — {item.reformat.reason}")
    if item.generated and item.generated.claim != G.NOT_GENERATED:
        out.append(f"  generated  {item.generated.claim} — {item.generated.reason}")
    if item.lock:
        out.append(f"  lockfile   {item.lock.headline()}")
    if item.suppression:
        out.extend("  " + line for line in item.suppression.block())
    for refusal in item.refused:
        out.append(f"  not folded {refusal}")
    for note in item.notes:
        out.append(f"  note       {note}")
    return out


def demo(policy: Policy | None = None) -> EpisodeNoise:
    """A tiny episode used by `--help` and the docs test, so an example is executable."""
    before = "fn a() -> i32 {\n    1 + 2\n}\n"
    after = "fn a() -> i32 {\n\t1 + 2\n}\n"
    return suppress(Episode.from_pairs("ep-demo", {"src/demo.rs": (before, after)}),
                    policy or Policy.built_in())
