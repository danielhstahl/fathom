"""Pulling the claims back out of a narrative and checking them against the evidence set.

This module is where the ticket's central number comes from. "Three reviewers thought it was
accurate" is an opinion; "of 214 claims, 6 assert a figure that is not in the evidence set and 2
contradict it" is a measurement, and it is the one that decides the question, because ADR-001's bar
is *hallucination counts at zero*, not "few enough not to annoy a reviewer".

A **claim** is one sentence of narrative, minus its citation markers, which are kept beside it. A
claim is **supported** when:

1. every number it asserts appears in the evidence set (`tier="strict"`: verbatim; `tier="tolerant"`:
   also allowing the stated derivations in `Support._derive` — shares, sums of a file's own `+`/`-`,
   counts by surface / change kind / directory, and the test share, i.e. the arithmetic a careful
   reader could do with the packet open and nothing more);
2. every path, identifier and code token it names occurs in the evidence set;
3. where it counts something the evidence measures (files, commits, lines, surfaces, directories,
   tests), the counts agree — a narrative that says "3 files" about a 6-file episode is charged with
   a **contradiction**, which is worse than an invention: an invention can be caught by an absent
   citation, while a contradiction reads as authoritative while being wrong;
4. where it explains *why* (`because`, `to <verb>`, `so that`, `due to`), it cites a commit whose
   subject carries the reason — an unattributed motive is a **fabricated rationale**.

Two defect classes are counted separately and are **not** hallucinations, because collapsing them
would hide both: `uncited` (a checkable claim with no citation) and `inference` (an evaluative
sentence — "this looks mechanical", "probably safe" — which the evidence set neither states nor
contradicts). An arm can be hallucination-free and still be unship-able for either.

The two tiers are reported side by side on purpose. The tolerant tier is the headline, because the
strict one charges an arm `96.6 % reviewable` when the packet says `2 999 of 3 103`, and a
measurement that cries wolf on arithmetic is a measurement nobody re-runs. The strict tier is
printed beside it so the leniency is visible, and every finding carries its claim text so any of them
can be adjudicated by hand.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any

from .packet import collapse_groups, index, number_words_in, numbers_in, plain

#: causal / motivational constructions: a "why" must come from a commit subject, not from the author
CAUSE = re.compile(r"\b(because|since|so that|in order to|due to|instead of|rather than|whenever)\b")
CAUSE_VERB = re.compile(r"\bto\s+(add|fix|support|make|keep|allow|prevent|avoid|rename|move|split|drop|"
                        r"cover|test|teach|warn|gate|wire|replace|document|prepare|prove|reserve|pin|"
                        r"catch|stop|start|open|close|align|guard|bump|refactor|implement|introduce)\w*")
#: evaluative constructions: interpretation the evidence set does not carry
EVALUATIVE = re.compile(r"\b(likely|probably|clearly|obviously|presumably|seems|seemingly|appears?|"
                        r"looks like|looks|straightforward|mechanical|trivial|risky|dangerous|safe|"
                        r"harmless|low[- ]risk|high[- ]risk|good practice|worth noting|note that|"
                        r"it is worth|in other words|effectively|essentially|basically|clean)\b")

_ID_TOKEN = re.compile(r"^(c\d+|p\d+|f\d+|g\d+|r\d+|m\d+)$")
_CITE_GROUP = re.compile(r"\[([^\]]*)\]")
_PATHISH = re.compile(r"[\w.\-]+/[\w.\-]+|\b[\w\-]+\.(?:rs|py|ts|js|go|rb|java|toml|json|md|yml|yaml|"
                      r"sql|lock|sh|c|h|hpp|cpp|tsx|jsx|txt|ini|cast|sqlite3)\b")
_CODEWORD = re.compile(r"\b(?:[a-z][a-z0-9]*_[a-z0-9_]+|[a-z][a-z0-9]*[A-Z][A-Za-z0-9]*|fathom[-_][a-z_]+|"
                       r"reco-[a-z0-9]+|e\d+\.\d+)\b")
_SENT_BREAK = re.compile(r"(?<=[A-Za-z0-9)\]'\"])\.!?[ \n]+|\n{2,}")

_WORD_NUM = {"one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7, "eight": 8,
             "nine": 9, "ten": 10, "eleven": 11, "twelve": 12, "fifteen": 15, "twenty": 20}
#: headline counts only — "test side 4 files" and "2 test lines" are subset counts, and charging one
#: of those against the episode total is a false positive, not a finding
_COUNT = re.compile(r"\b(\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|fifteen|"
                    rf"twenty)\s+(files?|commits?)\b")
_DISQUALIFIER = ("side", "share", "per", "deletion", "addition", "listed", " binary", "surfaces over",
                 "of the")
#: the noun in a counted phrase, and where the packet keeps the authoritative figure for it
_COUNT_SOURCE = {
    "file": "n_files", "files": "n_files",
    "commit": "n_commits", "commits": "n_commits",
}


@dataclass
class Claim:
    """One asserted sentence, with its citations and everything checkable inside it."""

    text: str
    cites: list[str] = field(default_factory=list)
    numbers: list[str] = field(default_factory=list)
    paths: list[str] = field(default_factory=list)
    codewords: list[str] = field(default_factory=list)
    counts: list[tuple[str, str, str]] = field(default_factory=list)
    has_cause: bool = False
    has_eval: bool = False

    @property
    def checkable(self) -> bool:
        return bool(self.numbers or self.paths or self.codewords or self.counts or self.has_cause)

    @property
    def cited(self) -> bool:
        return bool(self.cites)


@dataclass
class Finding:
    """One defect, with the claim that caused it — every finding is hand-adjudicable."""

    kind: str
    claim: str
    detail: str
    cites: list[str]

    def as_dict(self) -> dict[str, Any]:
        return {"kind": self.kind, "claim": self.claim, "detail": self.detail, "cites": self.cites}


HALLUCINATIONS = {"invented_number", "invented_path", "invented_identifier", "contradiction",
                  "fabricated_rationale"}


_QUOTED = re.compile(r"\u201c[^\u201c\u201d]*\u201d|\"[^\"]*\"")
#: the required "generated text" line. It is a watermark, not a claim: it is checked for presence, and
#: kept out of the claim audit, or every polished narrative would be charged with inventing its model id
_WATERMARK = re.compile(r"^\s*(polished|generated|drafted) by .*\.\s*$", re.I)


def split_watermark(text: str) -> tuple[str, bool]:
    """Return (narrative without the watermark line, whether a watermark was present)."""
    lines = text.split("\n")
    for i, line in enumerate(lines[:4]):
        if _WATERMARK.match(line.strip()):
            return "\n".join(lines[:i] + lines[i + 1:]), True
    return text, False


def unquoted(body: str) -> str:
    """Blank out quoted spans: quoting a commit subject that says "2 commits" is not asserting a count.
    """
    return _QUOTED.sub(" ", body)


def claims(text: str) -> list[Claim]:
    """Split a narrative into claims. Headings and bullets count as one claim each."""
    out: list[Claim] = []
    for raw in _SENT_BREAK.split(plain(text)):
        line = raw.strip().lstrip("#->* ").strip()
        if len(line) < 3:
            continue
        cites: list[str] = []
        for group in _CITE_GROUP.findall(line):
            for token in re.split(r"[\s,]+", group.strip()):
                if _ID_TOKEN.match(token):
                    cites.append(token)
        body = collapse_groups(_CITE_GROUP.sub(" ", line))
        low = body.lower()
        out.append(Claim(
            text=line,
            cites=sorted(set(cites)),
            numbers=sorted(set(numbers_in(body)) | {str(n) for n in number_words_in(unquoted(body))}),
            paths=sorted(set(_PATHISH.findall(body))),
            codewords=sorted(set(_CODEWORD.findall(body))),
            counts=[(m.group(1).lower(), m.group(2).lower(),
                     collapse_groups(body)[max(0, m.start() - 24):m.start()].lower())
                    for m in _COUNT.finditer(collapse_groups(unquoted(body)))],
            has_cause=bool(CAUSE.search(low) or CAUSE_VERB.search(low)),
            has_eval=bool(EVALUATIVE.search(low)),
        ))
    return out


_TOTALS_RE = re.compile(r"(\d+) file\(s\), \+(\d+) -(\d+), (\d+) commit")


def parse_totals(item: dict[str, Any] | None) -> tuple[int, int, int, int]:
    """`m3`'s value line, read by shape rather than by digit position — position breaks on a `?`."""
    if not item:
        return 0, 0, 0, 0
    m = _TOTALS_RE.search(plain(str(item.get("value", ""))))
    if not m:
        return 0, 0, 0, 0
    return (int(m.group(1)), int(m.group(2)), int(m.group(3)), int(m.group(4)))


class Support:
    """Everything one evidence packet entitles a narrative to say, as sets."""

    def __init__(self, packet: dict[str, Any]):
        self.packet = packet
        self.items = index(packet)
        parts: list[str] = []
        for it in packet["items"]:
            parts.extend([str(it.get("label", "")), str(it.get("value", "")),
                          str(it.get("path", "")), str(it.get("oid", ""))])
        parts.append(packet["episode"].get("subject", ""))
        self.blob = plain(" ".join(parts)).lower()
        self.words = set(re.findall(r"[a-z0-9][a-z0-9._/\-]{2,}", self.blob))

        self.paths = {it["path"] for it in packet["items"] if it.get("path")}
        self.path_bits: set[str] = set()
        for p in sorted(self.paths):
            segs = p.split("/")
            for i in range(len(segs)):
                self.path_bits.add("/".join(segs[i:]))
                self.path_bits.add("/".join(segs[:i + 1]))

        stated: set[str] = set()
        for it in packet["items"]:
            stated.update(numbers_in(str(it.get("value", ""))))
            stated.update(numbers_in(str(it.get("label", ""))))
            stated |= {str(n) for n in number_words_in(str(it.get("value", "")))}
            for key in ("score", "add", "del"):
                if isinstance(it.get(key), int):
                    stated.add(str(it[key]))
        stated |= {str(n) for n in number_words_in(packet["episode"].get("subject", ""))}
        stated.update(numbers_in(packet["episode"].get("subject", "")))
        ep = packet["episode"]
        stated |= {str(ep.get("n_commits", "")), str(len(self.paths))}
        self.strict_numbers = {s for s in stated if s not in ("", "None")}
        self.files = [it for it in packet["items"] if it.get("kind") == "file"]
        n_files_t, n_add_t, n_del_t, n_commits_t = parse_totals(
            next((i for i in packet["items"] if i["id"] == "m3"), None))
        # the delta document's own totals win over counting the listed rows: the index can hold more
        # files than the delta detailed, and charging a truthful "6 files" as a contradiction because
        # only 4 rows were expanded is exactly the false positive that would discredit the audit
        self.tot_n_files = n_files_t or len(self.files)
        self.tot_n_commits = n_commits_t
        self.tot_add, self.tot_del = n_add_t, n_del_t
        self._tallies()
        self.derived = self._derive(packet)
        self.numbers = self.strict_numbers | self.derived

    # -- the stated derivations ------------------------------------------------------------------
    def _derive(self, packet: dict[str, Any]) -> set[str]:
        """The arithmetic a careful reader could do with this packet open. Nothing more."""
        out: set[str] = set()
        files = self.files
        for kind, group in (("gap", "gap"), ("commit", "commit"), ("figure", "figure")):
            out.add(str(len([i for i in packet["items"] if i["kind"] == group])))
        out.add(str(len(files)))
        for it in files:
            a, d = (numbers_in(it["value"]) + ["", ""])[:2]
            if a and d:
                out |= {a, d, str(int(a) + int(d)), str(abs(int(a) - int(d)))}
        totals = next((i for i in packet["items"] if i["id"] == "m3"), None)
        churn = 0
        if totals is not None and self.tot_n_commits:
            add, dele, ncommits = self.tot_add, self.tot_del, self.tot_n_commits
            churn = add + dele
            out |= {str(churn), str(abs(add - dele))}
            if files:
                out |= _shares(churn, len(files))
            for n in (add, dele, churn, ncommits, len(files)):
                if churn:
                    out |= _shares(n, churn)
        for key in ("surface",):
            tally: dict[str, int] = {}
            for it in files:
                k = str(it.get(key))
                tally[k] = tally.get(k, 0) + 1
            out |= {str(v) for v in tally.values()}
            out.add(str(len(tally)))
            if churn:
                for n in tally.values():
                    out |= _shares(n, len(files))
        kinds: dict[str, int] = {}
        for it in files:
            k = it["value"].split(" ")[0]
            kinds[k] = kinds.get(k, 0) + 1
        out |= {str(v) for v in kinds.values()}
        tops: dict[str, int] = {}
        for it in files:
            top = str(it.get("path", "")).split("/")[0]
            tops[top] = tops.get(top, 0) + 1
        out |= {str(v) for v in tops.values()}
        out.add(str(len(tops)))
        tests = [it for it in files if _is_test(str(it.get("path", "")))]
        out.add(str(len(tests)))
        if files:
            out |= _shares(len(tests), len(files))
            for it in tests:
                a = numbers_in(it["value"])[:1]
                if a:
                    out |= _shares(int(a[0]), churn or 1)
        return out

    def authoritative(self, noun: str) -> set[int]:
        """The values a count of `noun` could take in this packet without being wrong.

        A set, deliberately: "0 files" is true of the test side of an episode with no tests, and
        charging it as a contradiction because the episode changed 6 files overall is the kind of
        false positive that gets an audit ignored. What survives is the case that matters — a count
        the packet cannot produce at all, like "14 files" about a 25-file episode.
        """
        n = noun.rstrip("s") if noun not in ("data", "media") else noun
        if n in ("file",):
            return ({self.tot_n_files, len(self.files), self.n_tests, len(self.top_dirs)}
                    | self.surface_counts | self.kind_counts | self.dir_counts)
        if n in ("commit",):
            return {self.tot_n_commits, int(self.packet["episode"].get("n_commits") or 0)}
        if n in ("line",):
            return {self.tot_add + self.tot_del, self.tot_add, self.tot_del}
        if n in ("surface",):
            return {len(self.surfaces)}
        if n in ("director", "dir"):
            return {len(self.top_dirs)}
        if n in ("test",):
            return {self.n_tests}
        return set()

    def _tallies(self) -> None:
        self.n_tests = len([f for f in self.files if _is_test(str(f.get("path", "")))])
        self.surfaces = {str(f.get("surface")) for f in self.files}
        self.top_dirs = {str(f.get("path", "")).split("/")[0] for f in self.files}
        self.surface_counts = set()
        self.kind_counts = set()
        self.dir_counts = set()
        by_surface: dict[str, int] = {}
        by_kind: dict[str, int] = {}
        by_dir: dict[str, int] = {}
        for f in self.files:
            s = str(f.get("surface"))
            by_surface[s] = by_surface.get(s, 0) + 1
            k = str(f.get("value", "")).split(" ")[0]
            by_kind[k] = by_kind.get(k, 0) + 1
            d = str(f.get("path", "")).split("/")[0]
            by_dir[d] = by_dir.get(d, 0) + 1
            parent = str(f.get("path", "")).rsplit("/", 1)[0] if "/" in str(f.get("path", "")) else "."
            by_dir[parent] = by_dir.get(parent, 0) + 1
        self.surface_counts = set(by_surface.values())
        self.kind_counts = set(by_kind.values())
        self.dir_counts = set(by_dir.values())

    # -- checking --------------------------------------------------------------------------------
    def check(self, claim: Claim, tier: str) -> list[Finding]:
        findings: list[Finding] = []
        pool = self.strict_numbers if tier == "strict" else self.numbers

        for c in claim.cites:
            if c not in self.items:
                findings.append(Finding("unknown_citation", claim.text,
                                        f"cites {c}, which is not in the evidence set", claim.cites))

        for n in claim.numbers:
            if n in pool:
                continue
            if n in self.derived and tier == "strict":
                findings.append(Finding("derived_number", claim.text,
                                        f"{n} is derivable from the packet but not stated in it", claim.cites))
            else:
                findings.append(Finding("invented_number", claim.text,
                                        f"asserts {n}, which the evidence set does not carry", claim.cites))

        for asserted, noun, context in claim.counts:
            if any(tok in context for tok in _DISQUALIFIER):
                continue
            n = _WORD_NUM.get(asserted, int(asserted) if asserted.isdigit() else None)
            allowed = self.authoritative(noun)
            if n is None or not allowed:
                continue
            # a count copied out of the evidence it cites is not a contradiction: "two commits
            # claiming one ticket are one episode" is a paraphrase of a commit subject, and an audit
            # that charges paraphrase as fabrication will be switched off inside a week
            phrase = f"{asserted} {noun}"
            digit_phrase = f"{n} {noun}"
            if any(phrase in str(i.get("value", "")).lower() or digit_phrase in str(i.get("value", "")).lower()
                   for i in (self.items[c] for c in claim.cites if c in self.items)):
                continue
            if n not in allowed:
                findings.append(Finding("contradiction", claim.text,
                                        f"says {n} {noun}; the packet can produce none of "
                                        f"{sorted(allowed)} for that count", claim.cites))

        for path in claim.paths:
            if path in self.paths or path in self.path_bits or path.strip("/") in self.blob:
                continue
            findings.append(Finding("invented_path", claim.text,
                                    f"names {path}, which is not in the change set", claim.cites))

        for word in claim.codewords:
            if word.lower() in self.blob or any(word.lower() in w for w in self.words):
                continue
            findings.append(Finding("invented_identifier", claim.text,
                                    f"names {word}, which is nowhere in the evidence set", claim.cites))

        if claim.has_cause:
            cited_items = [self.items[c] for c in claim.cites if c in self.items]
            if not cited_items:
                findings.append(Finding("fabricated_rationale", claim.text,
                                        "states a motive with nothing cited at all", claim.cites))
            elif not any(_overlaps(claim.text, str(i.get("value", ""))) for i in cited_items):
                findings.append(Finding("fabricated_rationale", claim.text,
                                        "states a motive none of the cited evidence carries", claim.cites))

        if claim.has_eval:
            findings.append(Finding("inference", claim.text,
                                    "evaluative claim; the evidence set neither states nor contradicts it",
                                    claim.cites))

        if claim.checkable and not claim.cited:
            findings.append(Finding("uncited", claim.text, "a checkable claim with no evidence citation",
                                    claim.cites))
        if not claim.checkable and not claim.has_eval:
            findings.append(Finding("unverifiable", claim.text,
                                    "no figure, path, identifier or motive to check", claim.cites))
        return findings


def cited_ids(text: str) -> set[str]:
    """Every evidence id a narrative cites — the denominator for "how much of the packet survived".

    Arm A cites nearly everything it is given; a polished arm that buys readability by dropping
    figures shows up here as a coverage loss, which is a measured cost rather than a vibe.
    """
    out: set[str] = set()
    for group in _CITE_GROUP.findall(plain(text)):
        for token in re.split(r"[\s,]+", group.strip()):
            if _ID_TOKEN.match(token):
                out.add(token)
    return out


def audit(text: str, support: Support, tiers: tuple[str, ...] = ("strict", "tolerant")) -> dict[str, Any]:
    """Audit one narrative against one evidence set, in both tiers."""
    cs = claims(text)
    cited = cited_ids(text)
    body, watermarked = split_watermark(text)
    cs = claims(body)
    cited = cited_ids(body)
    result: dict[str, Any] = {"claims": len(cs), "checkable": sum(1 for c in cs if c.checkable),
                              "words": len(body.split()), "watermarked": watermarked,
                              "cited_items": len(cited),
                              "coverage": round(len(cited & set(support.items)) / max(1, len(support.items)), 3)}
    for tier in tiers:
        found: list[Finding] = []
        for c in cs:
            found.extend(support.check(c, tier))
        by: dict[str, int] = {}
        for f in found:
            by[f.kind] = by.get(f.kind, 0) + 1
        hallucinated = [f for f in found if f.kind in HALLUCINATIONS]
        # a claim can fail in several ways at once; the claim-level count is what a human would call
        # "a hallucinated claim", so it is the headline and the finding-level count sits beside it
        result[tier] = {
            "hallucinated_claims": len({f.claim for f in hallucinated}),
            "hallucination_findings": len(hallucinated),
            "by_kind": by,
            "findings": [f.as_dict() for f in found],
        }
    return result


def _shares(part: int, whole: int) -> set[str]:
    if whole <= 0:
        return set()
    pct = 100.0 * part / whole
    return {str(round(pct)), str(round(pct, 1)), str(int(pct)), str(round(pct / 10) * 10)}


def _is_test(path: str) -> bool:
    low = path.lower()
    return "/test" in low or "test_" in low or "_test." in low or low.startswith("tests/") or "spec" in low


_STOP = {"the", "and", "for", "with", "that", "this", "from", "into", "onto", "their", "which",
         "when", "then", "than", "have", "has", "was", "were", "are", "its", "it", "of", "to",
         "in", "on", "as", "by", "at", "is", "an", "not", "no", "so", "itself", "them", "they",
         "does", "did", "all", "more", "also", "new", "adds", "add", "make", "makes", "use",
         "uses", "via", "per", "some", "what", "here", "over", "both", "same", "into", "keeps",
         "kepp", "about", "after", "before", "while", "against", "under", "across", "without",
         "within", "during", "because", "which", "each", "every", "other", "only", "just"}


def _overlaps(claim: str, subject: str) -> bool:
    """Does the cited commit subject actually carry the reason the claim states?

    Two shared content words is the bar. It is a cheap proxy and says so: it admits a claim whose
    shared words are accidental and fails one that paraphrases well. The spike's threats section
    lists the hand-checked false positives rather than pretending there are none.
    """
    left = {w for w in re.findall(r"[a-z]{4,}", plain(claim).lower()) if w not in _STOP}
    right = {w for w in re.findall(r"[a-z]{4,}", plain(subject).lower()) if w not in _STOP}
    return len(left & right) >= 2
