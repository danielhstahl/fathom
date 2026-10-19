"""The evidence set a narrative is allowed to use, and the citation index over it.

`e4.7`'s whole claim to trustworthiness is "every claim carries a resolvable evidence reference".
That is only checkable if the evidence has *addresses*, so the first thing the spike does is put an
address on every fact in the `fathom-delta/1` document `fathom analyze` prints: every figure, every
changed file, every gap, the risk vector, the attribution, the boundary rule. From then on a
narrative is a list of `(sentence, cited item ids)` pairs, and "supported" has a definition:

    a claim is supported when every value it asserts occurs in the items it cites, or in the small,
    stated set of figures derivable from them (`support.py`).

Nothing in here reads the world. `build_packet` is a pure function of a delta document plus the
commit subjects the runner supplies; `fathom analyze --json` is the only IO and `spike.py` does it.

### What goes into the packet, and why the caps are what they are

The packet is deliberately *generous*: every changed file is listed (not a top-N), because a cap
would turn a truthful sentence about the ninth file into a hallucination — and a false positive in
this experiment pushes towards "do not ship", which is the wrong direction to bias a decision. What
is left out is prose the delta document itself marks as not-mine: the long `gap.reason` strings are
reduced to their `short` form, since a narrative that paraphrases a gap reason is paraphrasing an
ADR rather than reporting the episode.
"""

from __future__ import annotations

import json
import re
import unicodedata
from typing import Any

SCHEMA = "fathom.narrative-evidence/1"

#: how many figures a narrative may cite from one section before the rest are folded
MAX_SECTION_FIGURES = 6

_THIN = {"\u2009": " ", "\u200a": " ", "\u202f": " ", "\u00a0": " ", "\u2212": "-", "\u2013": "-"}


def plain(text: str) -> str:
    """Normalise the typographic noise the delta document carries (thin spaces, U+2212 minus)."""
    for bad, good in _THIN.items():
        text = text.replace(bad, good)
    return unicodedata.normalize("NFC", text)


def _short(oid: str) -> str:
    return oid[:12] if len(oid) > 12 else oid


# ---------------------------------------------------------------------------------------------
# building
# ---------------------------------------------------------------------------------------------


def build_packet(delta: dict[str, Any], subjects: dict[str, str] | None = None) -> dict[str, Any]:
    """Address every fact in one `fathom-delta/1` document, so a narrative can cite it.

    `subjects` maps a member oid to its commit subject. It is optional because the delta document
    does not carry per-commit messages — only `episode.summary`, which is the lead commit's subject
    as stored — and the spike wants the difference: an LLM given only the lead subject writes a
    different *why* than one given all of them, and arm C measures the narrower case.
    """
    subjects = subjects or {}
    ep = delta.get("episode", {})
    items: list[dict[str, Any]] = []

    members = list(ep.get("members") or [])
    lead = members[0] if members else ep.get("first_oid", "")
    for i, oid in enumerate(members, start=1):
        items.append({
            "id": f"c{i}",
            "kind": "commit",
            "label": "commit",
            "oid": _short(oid),
            "value": subjects.get(oid) or (ep.get("summary") or "" if oid == lead else ""),
        })

    fid = 0
    for section in delta.get("sections", []):
        figures = [f for f in section.get("figures", []) if f.get("known", True) and f.get("value")]
        for fig in figures[:MAX_SECTION_FIGURES]:
            fid += 1
            items.append({
                "id": f"f{fid}",
                "kind": "figure",
                "section": section.get("id", ""),
                "label": fig.get("label", ""),
                "value": plain(str(fig.get("value", ""))),
                "pointer": (fig.get("evidence") or {}).get("pointer", ""),
            })

    for i, f in enumerate(delta.get("files", []), start=1):
        items.append({
            "id": f"p{i}",
            "kind": "file",
            "label": "file",
            "path": f.get("path", ""),
            "value": (f"{f.get('change','')} {f.get('path','')} "
                      f"+{f.get('add',0)} -{f.get('del',0)} "
                      f"{f.get('surface') or f.get('noise_class') or 'unknown'}"),
            "surface": f.get("surface") or f.get("noise_class") or "unknown",
        })

    risk = delta.get("risk") or {}
    if risk:
        items.append({"id": "r1", "kind": "risk", "label": "risk",
                      "value": plain(str(risk.get("vector", ""))) or f"score {risk.get('score')}",
                      "score": risk.get("score")})
        for i, comp in enumerate(risk.get("components") or [], start=2):
            items.append({"id": f"r{i}", "kind": "risk", "label": f"risk component {comp.get('name')}",
                          "value": plain(f"{comp.get('name')} {comp.get('points')}/{comp.get('cap')} "
                                         f"{comp.get('detail','')}")})

    for i, g in enumerate(delta.get("gaps") or [], start=1):
        items.append({"id": f"g{i}", "kind": "gap", "label": f"gap ({g.get('section')})",
                      "value": plain(f"{g.get('short','')} — not answered here, it is {g.get('owner','')}'s")})

    attribution = ep.get("attribution") or {}
    items.append({"id": "m1", "kind": "meta", "label": "author",
                  "value": plain(f"{ep.get('author_name','')} ({ep.get('author_kind','?')} author, "
                                 f"{attribution.get('source','?')} at {attribution.get('confidence','?')} confidence)")})
    items.append({"id": "m2", "kind": "meta", "label": "boundary",
                  "value": plain(f"boundary rule {ep.get('boundary_rule','?')} at "
                                 f"{ep.get('boundary_confidence','?')} confidence")})
    totals = delta.get("totals") or {}
    items.append({"id": "m3", "kind": "meta", "label": "totals",
                  "value": plain(f"{totals.get('n_files','?')} file(s), +{totals.get('n_add','?')} "
                                 f"-{totals.get('n_del','?')}, {totals.get('n_commits', ep.get('n_commits','?'))} commit(s), "
                                 f"{totals.get('n_binary',0)} binary")})

    first, last = ep.get("first_utc"), ep.get("last_utc")
    span = "" if not (first and last) else f"{max(0, int(last - first)) // 60000} minute(s) long"
    if span:
        items.append({"id": "m4", "kind": "meta", "label": "window", "value": span})

    return {
        "schema": SCHEMA,
        "episode": {
            "id": ep.get("id", ""),
            "subject": ep.get("summary") or "",
            "n_commits": ep.get("n_commits", len(members)),
            "lineage_seq": ep.get("lineage_seq"),
            "delta_digest": sha(delta),
        },
        "items": items,
    }


def sha(obj: Any) -> str:
    import hashlib
    return hashlib.sha256(canonical(obj).encode()).hexdigest()


def canonical(obj: Any) -> str:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def index(packet: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """`item id -> item`, the citation index a narrative is checked against."""
    return {it["id"]: it for it in packet["items"]}


def render_index(packet: dict[str, Any]) -> str:
    """The citation index as the text an author (human or model) is handed."""
    lines = [f"# evidence for {packet['episode']['id']}: {packet['episode']['subject']}",
             "Every fact you may assert, with the id you must cite for it. Nothing else is evidence."]
    for it in packet["items"]:
        lines.append(f"[{it['id']}] {it['label']}: {it['value']}".rstrip())
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------------------------
# numbers
# ---------------------------------------------------------------------------------------------

_UNITS = {
    "zero": 0, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7,
    "eight": 8, "nine": 9, "ten": 10, "eleven": 11, "twelve": 12, "thirteen": 13, "fourteen": 14,
    "fifteen": 15, "sixteen": 16, "seventeen": 17, "eighteen": 18, "nineteen": 19,
    "a": 1, "an": 1, "single": 1, "both": 2, "dozen": 12,
}
_TENS = {"twenty": 20, "thirty": 30, "forty": 40, "fifty": 50, "sixty": 60, "seventy": 70,
         "eighty": 80, "ninety": 90}
_UNIT_ALT = "|".join(k for k in _UNITS if k not in ("a", "an", "single", "both", "dozen"))
_WORD_NUM_RE = re.compile(
    r"\b(?:(" + "|".join(_TENS) + r")(?:[- ](" + _UNIT_ALT + r"))?|(" + "|".join(_UNITS) + r"))\b", re.I)

_DECIMAL = re.compile(r"-?\d[\d,]*(?:\.\d+)?")


def collapse_groups(text: str) -> str:
    """`+3 664 -47` -> `+3664 -47`: fold digit-grouping spaces so `3 664` and `3,664` agree."""
    prev = None
    while prev != text:
        prev = text
        text = re.sub(r"(\d)[ \u2009\u200a\u00a0](\d{3})(?!\d)", r"\1\2", text)
    return text


def numbers_in(text: str) -> list[str]:
    """Every number a piece of text asserts, normalised to bare digits, unsigned.

    `+3 102 −1` (a delta figure with thin-space grouping) and `3,102` both yield `3102`, so an LLM
    that reformats a number is not charged for hallucinating it. Signs are dropped: `-47` and `47`
    are the same figure about a deletion, and an arm that writes "47 lines removed" is right.
    """
    text = collapse_groups(plain(text)).replace(",", "")
    return [m.lstrip("-") for m in _DECIMAL.findall(text)]


def number_words_in(text: str) -> list[int]:
    """Spelled-out counts, so "five commits" is checked against the same figure as "5 commits".

    Compounds are summed, not split: "forty-five" is 45. A first attempt here read it as 40 and 5 and
    charged every spelled number as a hallucination, which is exactly the false positive that gets an
    audit switched off.
    """
    out = []
    for tens, unit, word in _WORD_NUM_RE.findall(text):
        if word:
            out.append(_UNITS[word.lower()])
        else:
            value = _TENS[tens.lower()]
            if unit:
                value += _UNITS[unit.lower()]
            out.append(value)
    return out
