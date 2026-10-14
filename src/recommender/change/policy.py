"""How a path rule and content evidence are combined, and what the confidence number means.

This is the whole of the classifier's judgement, in one file, so it can be read in one
sitting and argued with in one place. Three decisions matter:

**1. Content evidence sits *above* the built-in table and *below* a configured assertion.**
`e0.9` refuses content-sniffing inside the rule table
([paths.md](../../../docs/concepts/paths.md) §9) because a table entry must stay
overrulable: "a file that *looks* generated and is hand-maintained is exactly the case a
user must be able to overrule, and a path rule is overrulable in a way a sniff is not."
This package honours that sentence rather than reversing it:

```
assertion (a path rule from a config layer: flag > env > repo > user)   0.98    always wins
path convention (built-in `vendored`, built-in `lockfile`)              0.97    content cannot refute
byte facts (NUL / undecodable head)                                     0.94
decisive content markers and parsed shapes                              0.75 – 0.95
built-in path rule, by specificity                                      0.65 – 0.75
content evidence, suggestive, and path hints                            0.45 – 0.62
nothing                                                                   → other
```

So a `fathom.toml` that says `generated = ["src/protos/**"]` beats a byte-sniff, a protoc
banner beats `**/*.rs`, and `vendor/` beats both. All three are tested; none is a special
case.

**2. Confidence is a claim about evidence, not a probability.** Scores are per-class sums of
the strongest claim for that class plus a small corroboration bonus, then read against four
bands (`CONFIRMED / LIKELY / WEAK / AMBIGUOUS`). Nothing here is calibrated against a
labelled *distribution of repositories* — there is no such thing to calibrate against, and a
number that looks like a probability and is not one is worse than a band. The bands are the
interface: `AMBIGUOUS` means the reviewer should look, and per AC 3 it is the only band
`other` may carry.

**3. Ambiguity resolves to `other`, and never to `source`.** Two claims within
`AMBIGUITY_GAP` of each other and neither decisive → `other`, with both claims in the reason.
Nothing at all → `other`. The asymmetry is deliberate: guessing `source` inflates the
reviewable line count and *lowers* the noise share, which is the failure mode that makes a
risk score lie; answering `other` costs a reviewer one glance at a path the table has not
been taught yet, and `fathom change explain` prints the rule to add.

Weights are constants, not config. A tunable ambiguity threshold would be a way to make the
classifier *look* right on a repository, which is the opposite of what a label that
everything downstream trusts needs. They are pinned in the contract test, which means
changing one is a deliberate act with a failing test attached.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from recommender.taxonomy import classes as C

# ---------------------------------------------------------------------------
# the weight table (see the module docstring's ladder)
# ---------------------------------------------------------------------------

#: Weight of a path rule that came from a config layer (`repo`/`user`/`env`/`flag`): an
#: assertion somebody wrote down, which outranks a guess about their intent — and outranks
#: everything else in this file, which is what keeps e0.9's "overrulable" promise true.
W_ASSERTION = 0.98

#: Classes whose claim is about *why the file is in this tree*, not about the file's own
#: bytes: somebody else's code (`vendored`), or resolved dependency state (`lockfile`). No
#: header inside the file can refute either — a protoc stub under `vendor/` is not ours,
#: whatever its banner says, and a lockfile with a generated-code banner is still a lockfile.
#: Content may corroborate them and raise their confidence, and a user's assertion can still
#: demote them (`source = ["vendor/our-fork/**"]`), but a sniff cannot overrule them.
PATH_CONVENTION_CLASSES = (C.PathClass.VENDORED, C.PathClass.LOCKFILE)
W_PATH_CONVENTION = 0.97

#: Base weight of a rule from the built-in table, plus up to `W_SPECIFICITY_MAX` for how much
#: of the path the winning pattern actually named (`recommender.taxonomy.rules.specificity`).
#: A built-in gets 0.65–0.75: strong enough to stand on its own, below every decisive marker.
W_BUILTIN_BASE = 0.65
W_SPECIFICITY_MAX = 0.09

#: The weight a *content* claim needs to overrule a built-in path rule. `markers.DECISIVE` is
#: the same number on the marker side; the two must agree and the contract test says so.
DECISIVE = 0.75

#: A hint from the file's extension alone. Below the built-in path rule on purpose: the table
#: matches far more than extensions, so a hint may corroborate and may rescue an unknown path,
#: but it may not contradict the table.
W_EXTENSION = 0.55

#: A path named by a build config (`CMakeLists.txt`, `Cargo.toml` `path =`, `package.json`
#: `main`, a `Dockerfile` `COPY`, `tsconfig` `include`). Someone maintains that list by hand,
#: so the thing it names is hand-written — which is why the vote is `source` and why it is only
#: allowed to speak when nothing about the path already has an opinion.
W_BUILD_MEMBER = 0.55

#: A structured file that parses as configuration (`{…}`, `[table]`, `key: value`).
W_PARSED_CONFIG = 0.55

#: The file does not decode as text at all, or carries a container/executable magic number.
#: Not higher: a user's assertion still outranks it, which is what keeps the "overrulable"
#: promise true even for bytes.
W_BINARY = 0.94

#: Rows below this are not a claim; a class whose best score is under the floor is not
#: asserted, and if no class clears it the answer is `other`.
CONFIDENCE_FLOOR = 0.50

#: Two classes within this of each other, neither decisive: the honest answer is "unknown".
AMBIGUITY_GAP = 0.06

#: Each agreeing vote past the first adds this, up to the cap. Corroboration is confidence,
#: not certainty: four weak hints about the same class must not reach a banner's weight.
CORROBORATION = 0.04
CORROBORATION_MAX = 0.12

#: Reported confidences are clamped below 1.0: no rule-based classifier is certain, and a
#: `1.0` in a UI teaches reviewers to stop looking.
CONFIDENCE_MAX = 0.99

#: Band floors, descending. `fathom change explain` prints the band, not just the number,
#: because "0.72" invites more argument than "likely".
BAND_CONFIRMED = "confirmed"
BAND_LIKELY = "likely"
BAND_WEAK = "weak"
BAND_AMBIGUOUS = "ambiguous"
BANDS: tuple[str, ...] = (BAND_CONFIRMED, BAND_LIKELY, BAND_WEAK, BAND_AMBIGUOUS)
BAND_FLOORS: dict[str, float] = {BAND_CONFIRMED: 0.85, BAND_LIKELY: 0.70, BAND_WEAK: 0.50,
                                 BAND_AMBIGUOUS: 0.0}

#: Tie-break between two classes with equal score, biggest wins. The order is "how much does
#: the reviewer's next action change if this class is the right one", with the derived
#: classes first because a wrong `generated` hides a change and a wrong `source` only
#: invents one: binary, lockfile, vendored, generated, migration, schema, asset, test,
#: config, docs, source, other.
CLASS_PRIORITY: dict[str, int] = {name: len(C.PATH_CLASSES) - i for i, name in enumerate(
    (C.PathClass.BINARY, C.PathClass.LOCKFILE, C.PathClass.VENDORED, C.PathClass.GENERATED,
     C.PathClass.MIGRATION, C.PathClass.SCHEMA, C.PathClass.ASSET, C.PathClass.TEST,
     C.PathClass.CONFIG, C.PathClass.DOCS, C.PathClass.SOURCE, C.PathClass.OTHER))}

#: Where a vote came from. Printed in the receipts, and used for exactly one rule: an
#: `assertion` outranks everything, and `source` may not be asserted without affirmative
#: evidence (`affirmative`).
KIND_PATH = "path"
KIND_ASSERTION = "assertion"
KIND_CONTENT = "content"
KIND_HINT = "hint"


@dataclass(frozen=True)
class Vote:
    """One claim that a file is some class, with the receipt that produced it."""

    cls: str
    weight: float
    source: str                       # `rule:gen.protobuf`, `marker:gen.protoc`, `ext:.rs` …
    note: str = ""
    kind: str = KIND_CONTENT

    @property
    def decisive(self) -> bool:
        return self.weight >= DECISIVE

    def as_dict(self) -> dict:
        return {"class": self.cls, "weight": round(self.weight, 3), "source": self.source,
                "kind": self.kind, "note": self.note, "decisive": self.decisive}


@dataclass
class Result:
    """The outcome of combining votes: a class, a confidence, and why."""

    cls: str
    confidence: float
    band: str
    winner: Vote | None = None
    votes: list[Vote] = field(default_factory=list)
    scores: list[tuple[str, float]] = field(default_factory=list)
    ambiguous: bool = False
    overrode_path: bool = False
    path_class: str = ""
    notes: list[str] = field(default_factory=list)

    @property
    def runner_up(self) -> str:
        return self.scores[1][0] if len(self.scores) > 1 else ""

    def as_dict(self) -> dict:
        return {"class": self.cls, "confidence": round(self.confidence, 2), "band": self.band,
                "ambiguous": self.ambiguous, "overrode_path": self.overrode_path,
                "path_class": self.path_class, "runner_up": self.runner_up,
                "winner": self.winner.as_dict() if self.winner else None,
                "scores": [[cls, round(score, 3)] for cls, score in self.scores],
                "votes": [v.as_dict() for v in self.votes], "notes": list(self.notes)}


def band_of(confidence: float) -> str:
    for band in BANDS:
        if confidence >= BAND_FLOORS[band]:
            return band
    return BAND_AMBIGUOUS


def refine_not_text(votes: list[Vote], path_class: str) -> list[Vote]:
    """Let the table, not the sniff, choose *which* non-text class applies.

    A NUL byte or an undecodable head says "this is not text"; it does not say whether that
    means `binary` (a compiled artefact, a packed archive) or `asset` (a font, an image, a
    PDF), and the difference matters to a reviewer — one is counted, the other is opened and
    looked at. The path table already knows the media extensions, because a repository's
    `.png` is a `.png` whatever its bytes; so when the table has claimed one of the two
    non-text classes, the byte fact corroborates that claim instead of contradicting it.
    Where the table had nothing to say, "not text" is answered as `binary`, which is the
    conservative reading: a diff of it is meaningless.
    """
    if path_class not in (C.PathClass.ASSET, C.PathClass.BINARY):
        return votes
    out: list[Vote] = []
    for vote in votes:
        if vote.kind == "content" and vote.cls == C.PathClass.BINARY and \
                vote.source.startswith("bytes:"):
            vote = Vote(cls=path_class, weight=0.90, kind="content", source=vote.source,
                        note="%s; the path table already names it %s, which is the "
                             "non-text class that applies" % (vote.note, path_class))
        out.append(vote)
    return out


def affirmative(cls: str, votes: list[Vote]) -> bool:
    """Whether some vote *positively* supports `cls`, rather than nothing contradicting it.

    The one thing AC 3 is really about: a `source` label must be earned. Silence is not
    evidence of code, so `source` (and every other class) needs a vote for that class at or
    above `CONFIDENCE_FLOOR`, not merely the absence of a vote against it.
    """
    return any(v.cls == cls and v.weight >= CONFIDENCE_FLOOR for v in votes)


def path_weight(verdict) -> Vote | None:
    """The path axis as a vote: e0.9's answer, weighted by where the winning rule came from.

    `recommender.config.schema.Layer.DEFAULTS` means "the built-in table guessed from the
    shape of the tree"; anything else means a person asserted this path's class in
    `fathom.toml`, a flag or an environment variable, and per §1 above that wins outright.
    A built-in rule claiming one of :data:`PATH_CONVENTION_CLASSES` gets the same order of
    magnitude as an assertion, for the reason given there.
    """
    from recommender.config.schema import Layer

    match = verdict.class_match
    if match is None or match.rule.id == "fallback.unclassified":
        return None
    if match.rule.origin == "config" or match.rule.layer != Layer.DEFAULTS:
        return Vote(cls=verdict.cls, weight=W_ASSERTION, kind=KIND_ASSERTION,
                    source="rule:%s" % match.rule_id,
                    note="asserted by %s at layer %s (%s)" % (match.rule.key or match.rule_id,
                                                              match.rule.layer,
                                                              match.pattern))
    if verdict.cls in PATH_CONVENTION_CLASSES:
        return Vote(cls=verdict.cls, weight=W_PATH_CONVENTION, kind=KIND_PATH,
                    source="rule:%s" % match.rule_id,
                    note="path rule %s matched `%s`; %s is a claim about why this file is in "
                         "the tree, which content cannot refute" % (match.rule_id, match.pattern,
                                                                    verdict.cls))
    spec = match.specificity[1] if match.specificity else 0
    bonus = min(W_SPECIFICITY_MAX, 0.01 * spec)
    return Vote(cls=verdict.cls, weight=W_BUILTIN_BASE + bonus, kind=KIND_PATH,
                source="rule:%s" % match.rule_id,
                note="path rule %s matched `%s` (%s)" % (match.rule_id, match.pattern,
                                                         match.rule.source))


def score_classes(votes: list[Vote]) -> list[tuple[str, float]]:
    """Per-class score: strongest claim, plus a capped corroboration bonus, biggest first."""
    grouped: dict[str, list[Vote]] = {}
    for vote in votes:
        grouped.setdefault(vote.cls, []).append(vote)
    out: list[tuple[str, float]] = []
    for cls, group in grouped.items():
        best = max(vote.weight for vote in group)
        bonus = min(CORROBORATION_MAX, CORROBORATION * (len(group) - 1))
        out.append((cls, round(min(CONFIDENCE_MAX, best + bonus), 4)))
    out.sort(key=lambda item: (-item[1], -CLASS_PRIORITY.get(item[0], 0)))
    return out


def guard_path_conventions(votes: list[Vote]) -> list[Vote]:
    """Cap content claims that try to overrule a path convention (see the module docstring).

    `vendor/legacy/lock.json` is the case this exists for. Its bytes are a lockfile and its
    path is `vendor/`, and the reviewer's action is decided by the second one: it is not
    ours. The claim is not silenced — it stays in the receipts at a capped weight, with the
    reason in its note — because "the generator wrote it, but it is vendored, so nobody is
    editing it here" is a sentence worth printing.
    """
    convention = [v for v in votes if v.kind == KIND_PATH and v.cls in PATH_CONVENTION_CLASSES]
    if not convention:
        return votes
    claimed = max(convention, key=lambda v: v.weight)
    ceiling = W_PATH_CONVENTION - 0.05
    out: list[Vote] = []
    for vote in votes:
        if vote.kind == KIND_CONTENT and vote.cls != claimed.cls and vote.weight > ceiling:
            vote = Vote(cls=vote.cls, weight=ceiling, kind=vote.kind, source=vote.source,
                        note="%s; noted, but the path makes this `%s`, and that is a claim "
                             "about why the file is in the tree" % (vote.note, claimed.cls))
        out.append(vote)
    return out


def combine(votes: list[Vote], *, path_class: str = "") -> Result:
    """Turn a set of votes into one class, one confidence and a reason anyone can read."""
    votes = [v for v in votes if v.cls and v.cls != C.PathClass.OTHER]
    votes = guard_path_conventions(votes)
    scores = score_classes(votes)
    notes: list[str] = []

    assertions = [v for v in votes if v.kind == KIND_ASSERTION]
    if assertions:
        winner = max(assertions, key=lambda v: v.weight)
        return Result(cls=winner.cls, confidence=min(CONFIDENCE_MAX, winner.weight),
                      band=BAND_CONFIRMED, winner=winner, votes=votes,
                      scores=score_classes(votes), path_class=path_class,
                      notes=["an assertion in this repository's own configuration settled "
                             "the class; content evidence was read but cannot overrule it"])

    if not scores or scores[0][1] < CONFIDENCE_FLOOR:
        best = scores[0][1] if scores else 0.0
        notes.append("nothing about the path or the content names a class: no claim reached "
                     "the confidence floor (%.2f), so the answer is the honest one, `other`"
                     % CONFIDENCE_FLOOR)
        return Result(cls=C.PathClass.OTHER, confidence=round(max(0.15, best * 0.7), 2),
                      band=BAND_AMBIGUOUS, winner=None, votes=votes, scores=scores,
                      ambiguous=True, path_class=path_class, notes=notes)

    top_cls, top_score = scores[0]
    second = scores[1] if len(scores) > 1 else ("", 0.0)
    top_decisive = any(v.cls == top_cls and v.decisive for v in votes)

    if not top_decisive and second[1] and top_score - second[1] < AMBIGUITY_GAP:
        notes.append("`%s` (%.2f) and `%s` (%.2f) are within %.2f of each other and neither "
                     "claim is decisive: an ambiguous path is `other`, per e2.1"
                     % (top_cls, top_score, second[0], second[1], AMBIGUITY_GAP))
        return Result(cls=C.PathClass.OTHER, confidence=0.40, band=BAND_AMBIGUOUS,
                      winner=None, votes=votes, scores=scores, ambiguous=True,
                      path_class=path_class, notes=notes)

    if top_cls == C.PathClass.SOURCE and not affirmative(C.PathClass.SOURCE, votes):
        notes.append("the best claim was `%s`, but `source` needs affirmative evidence and "
                     "only the absence of contradiction" % top_cls)
        return Result(cls=C.PathClass.OTHER, confidence=0.40, band=BAND_AMBIGUOUS,
                      winner=None, votes=votes, scores=scores, ambiguous=True,
                      path_class=path_class, notes=notes)

    winner = max((v for v in votes if v.cls == top_cls), key=lambda v: v.weight)
    if path_class and path_class != top_cls and top_decisive:
        notes.append("content evidence overruled the path rule: the path said `%s`, the "
                     "bytes said `%s`" % (path_class, top_cls))
    return Result(cls=top_cls, confidence=round(min(CONFIDENCE_MAX, top_score), 2),
                  band=band_of(top_score), winner=winner, votes=votes, scores=scores,
                  overrode_path=bool(path_class and path_class != top_cls),
                  path_class=path_class, notes=notes)
