"""Arm A — the deterministic narrative: a template that can only say what it cites.

`e4.7` describes "a deterministic, template-composed narrative per episode built from the delta model
and findings, with every claim citing an evidence id (no unattached assertions)". This is that thing,
in the smallest form that answers the ticket's question: the composer has no vocabulary of its own
beyond the connectives below, and every value it prints is copied out of an evidence item whose id it
prints beside it. That is what makes arm A the control: it is not *less likely* to hallucinate than
an LLM, it has no channel to hallucinate through — a template cannot name a file the delta document
did not name, and cannot invent a motive because it does not write motives at all (the *why* line
quotes a commit subject and cites it, which is the only motive-shaped sentence arm A is allowed).

The point of the exercise is to find out what that costs in readability. Output is a byte function of
the delta document: no clock, no locale, no dict iteration order, no terminal probe, so arm A can
serve `e4.7`'s byte-determinism acceptance criterion and the golden tests that depend on it.
"""

from __future__ import annotations

from typing import Any

from .packet import plain

#: section id -> the sentence-opening phrase used for it. Fixed order, fixed words.
SECTIONS = [
    ("what", "What changed"),
    ("where", "Where it landed"),
    ("dependencies", "Dependencies"),
    ("surface", "Surfaces and operator actions"),
    ("tests", "Tests"),
    ("blast", "Blast radius"),
    ("attention", "What to read first"),
]

_LEAD = {"what": "What", "where": "Where", "dependencies": "Dependencies", "surface": "Surface",
         "tests": "Tests", "blast": "Blast", "attention": "Attention"}


def _fig(item: dict[str, Any]) -> str:
    label = str(item.get("label", "")).strip()
    value = plain(str(item.get("value", ""))).strip()
    if not value:
        return ""
    if label and label.lower() not in value.lower():
        return f"{label} {value} [{item['id']}]"
    return f"{value} [{item['id']}]"


def render(packet: dict[str, Any]) -> str:
    """Compose arm A's narrative for one evidence packet. Pure, ordered, byte-stable."""
    ep = packet["episode"]
    by_section: dict[str, list[dict[str, Any]]] = {}
    for it in packet["items"]:
        if it["kind"] == "figure":
            by_section.setdefault(it.get("section", ""), []).append(it)

    files = [it for it in packet["items"] if it["kind"] == "file"]
    commits = [it for it in packet["items"] if it["kind"] == "commit"]
    gaps = [it for it in packet["items"] if it["kind"] == "gap"]
    totals = next((it for it in packet["items"] if it["id"] == "m3"), None)
    meta_author = next((it for it in packet["items"] if it["id"] == "m1"), None)
    meta_bound = next((it for it in packet["items"] if it["id"] == "m2"), None)
    risk_items = [it for it in packet["items"] if it["kind"] == "risk"]

    lines: list[str] = []
    subject = plain(str((commits[0].get("value") if commits and commits[0].get("value")
                         else ep.get("subject")) or "(no subject)")).strip()
    lines.append(f"{subject} [{commits[0]['id'] if commits else 'm1'}].")

    if meta_author:
        lines.append(f"Authorship: {_fig(meta_author)}.")
    if meta_bound:
        lines.append(f"Grouping: {_fig(meta_bound)}.")

    for sid, heading in SECTIONS:
        figs = [_fig(f) for f in by_section.get(sid, [])]
        figs = [f for f in figs if f]
        if not figs:
            continue
        body = "; ".join(figs)
        lines.append(f"**{_LEAD[sid]}** — {body}.")

    if totals:
        lines.append(f"Scale — {_fig(totals)}.")

    if files:
        biggest = max(files, key=lambda f: _churn(f))
        lines.append(f"The largest single file is {biggest['path']} "
                     f"(+{_add(biggest)} -{_del(biggest)}, {biggest.get('surface')}) [{biggest['id']}].")

    if commits and len(commits) > 1:
        quoted = "; ".join(f"“{plain(str(c['value']))}” [{c['id']}]" for c in commits if c.get("value"))
        if quoted:
            lines.append(f"The episode's commits, in order: {quoted}.")

    why = next((c for c in commits if c.get("value")), None)
    if why:
        lines.append(f"Why — as the commit says it: “{plain(str(why['value']))}” [{why['id']}].")

    if gaps:
        listed = "; ".join(f"{g['value']} [{g['id']}]" for g in gaps)
        lines.append(f"What this narrative does not answer — {listed}.")

    if risk_items:
        head = next((r for r in risk_items if r["id"] == "r1"), risk_items[0])
        rest = [r for r in risk_items if r is not head]
        text = f"Risk — {plain(str(head['value']))} [{head['id']}]"
        if rest:
            text += ", from " + "; ".join(f"{plain(str(r['value']))} [{r['id']}]" for r in rest)
        lines.append(text + ".")

    return "\n\n".join(lines) + "\n"


def _num(text: str) -> list[int]:
    import re
    return [int(x) for x in re.findall(r"\d+", plain(text))]


def _add(item: dict[str, Any]) -> int:
    nums = _num(str(item.get("value", "")))
    return nums[0] if nums else 0


def _del(item: dict[str, Any]) -> int:
    nums = _num(str(item.get("value", "")))
    return nums[1] if len(nums) > 1 else 0


def _churn(item: dict[str, Any]) -> int:
    return _add(item) + _del(item)
