"""Who asked for this — the answer, and the honest shape of not having one.

A new dependency is the single most consequential thing an agent turn can do, and the reviewer's
question about it has two halves: **which episode** put it in the tree, and **what were they
asked to do** when it happened. The first is usually knowable; the second is knowable only if
somebody captured it, and this package's rule is that the difference between the two is always
printed as a different sentence. An empty field is a bug in a review tool; a field that says *not
recorded, and here is what would fill it* is a tool that has been thought about.

Three states, in this order of preference:

1. **this episode** — the package was added by the episode under review, so the episode and its
   intent are right there;
2. **a recorded introduction** — an earlier episode's record says it added this package
   (`deps.provenance.search_previous_episodes`), which is the case that matters two weeks later;
3. **not recorded** — and the card says so, names the evidence it looked for, and names what would
   have supplied it.

Intent has its own ladder, because not every kind of evidence is an answer. A trailer a human
wrote is; a plan item an agent wrote for itself is, at its own confidence; a harness turn is, if
its text was stored — and in hooks v1 it is not (`prompt_sha256` and `prompt_chars` only,
ADR-005 §2.2), so a digest-only turn is reported as *a request happened and its text was not
kept*, which is a partial, not an answer. `deps.provenance.intent_sources` decides which kinds may
stand as the answer, and `deps.provenance.agent_counts_as_intent` (off) decides whether "codex did
it" may ever be read as "codex was asked to".
"""

from __future__ import annotations

from dataclasses import dataclass, field

from dataclasses import replace

from .model import (
    ATTR_AGENT, ATTR_UNKNOWN, DIRECT, INTENT_LABEL, Intent, Package, edge_path)

#: Where the answer came from.
ANSWER_THIS_EPISODE = "this-episode"
ANSWER_RECORDED = "recorded-introduction"
ANSWER_NONE = "not-recorded"

#: Whether the intent half answered, and if not, why not.
INTENT_ANSWERED = "answered"
INTENT_GUESS = "guess"
INTENT_DIGEST = "digest-only"
INTENT_REFUSED = "source-not-listed"
INTENT_EMPTY = "empty"
INTENT_NONE = "none"

#: What a reviewer can do about a missing half, printed verbatim. `e4.2` is the ticket that makes
#: `harness-turn` a real source instead of a digest; until then the trailer is the working path.
REMEDY_INTENT = ("capture it: an `Agent-Intent:` trailer on the commit, or intent capture (`e4.2`, "
                 "which turns a harness turn's prompt into stored text)")
REMEDY_EPISODE = ("make it answerable: review the episode that added it (the introduction record "
                  "is written when an episode is indexed), or index from farther back")


@dataclass(frozen=True)
class Answer:
    """The provenance answer for one package — or the well-formed statement that there is none."""

    package: Package
    state: str = ANSWER_NONE
    episode_id: str = ""
    commit: str = ""
    intent: Intent | None = None
    intent_state: str = INTENT_NONE
    attribution: str = ATTR_UNKNOWN
    agent: str = ""
    identity: str = ""
    confidence: float = 1.0
    basis: str = ""
    missing: str = ""
    remedy: str = ""
    evidence: list[str] = field(default_factory=list)
    reviewed: bool = False

    @property
    def answered(self) -> bool:
        return self.state != ANSWER_NONE

    @property
    def intent_answered(self) -> bool:
        return self.intent_state in (INTENT_ANSWERED, INTENT_GUESS)

    @property
    def who(self) -> str:
        if self.attribution == ATTR_AGENT:
            return "agent %s" % (self.agent or "unnamed")
        if self.attribution == "bot":
            return "bot %s" % (self.agent or self.identity or "unnamed")
        if self.attribution == "human":
            return "human %s" % (self.identity or "unnamed")
        return "author not recorded"

    @property
    def says(self) -> str:
        """The one line the card prints. Every clause names its own evidence."""
        if not self.answered:
            return "introduced by: not recorded — %s" % (self.missing or "nothing says")
        head = "introduced by episode %s" % self.episode_id
        if self.commit:
            head += " (commit %s)" % self.commit
        if self.basis:
            head += " — %s" % self.basis
        if self.intent_state == INTENT_ANSWERED:
            return '%s, intent: "%s" (%s, confidence %.2f)' % (
                head, self.intent.text, INTENT_LABEL.get(self.intent.source, self.intent.source),
                float(self.intent.confidence))
        if self.intent_state == INTENT_GUESS:
            return '%s, intent (guess, confidence %.2f): "%s" (%s)' % (
                head, float(self.intent.confidence), self.intent.text,
                INTENT_LABEL.get(self.intent.source, self.intent.source))
        if self.intent_state == INTENT_DIGEST:
            return ("%s, intent: not captured — the harness stored only a prompt digest "
                    "(sha256 %s, %d chars) and hooks v1 does not store prompt text "
                    "(ADR-005 §2.2)" % (head, (self.intent.prompt_sha256[:12] or "?"),
                                        int(self.intent.prompt_chars or 0)))
        if self.intent_state == INTENT_REFUSED:
            return ("%s, intent: not used — %s is not in `deps.provenance.intent_sources`"
                    % (head, INTENT_LABEL.get(self.intent.source, self.intent.source)))
        return "%s, intent: not recorded" % head

    @property
    def intent_note(self) -> str:
        """What would fill the gap, when there is one."""
        if self.intent_state in (INTENT_ANSWERED, INTENT_GUESS):
            return ""
        return self.remedy or REMEDY_INTENT

    def rows(self) -> list[str]:
        """The card block: the sentence, then the receipts."""
        lines = ["introduced by: %s" % self._short_says()]
        if self.intent_state in (INTENT_ANSWERED, INTENT_GUESS):
            lines.append('intent: "%s"' % self.intent.text)
            lines.append("  source: %s (confidence %.2f) · attribution: %s"
                         % (INTENT_LABEL.get(self.intent.source, self.intent.source),
                            float(self.intent.confidence), self.who))
        else:
            lines.append("intent: %s" % self._intent_short())
            note = self.intent_note
            if note:
                lines.append("  to fill it: %s" % note)
        lines.append("asked by: %s" % edge_path(self.package.asked_by, self.package.text))
        if self.package.scope:
            lines.append("declared in: %s" % self.package.scope)
        if self.reviewed:
            lines.append("already signed off by a reviewer")
        return lines

    def _short_says(self) -> str:
        if not self.answered:
            return "not recorded (%s)" % (self.missing or "nothing says")
        text = "episode %s" % self.episode_id
        if self.commit:
            text += " (commit %s)" % self.commit
        if self.basis:
            text += " — %s" % self.basis
        return text

    def _intent_short(self) -> str:
        if self.intent_state == INTENT_DIGEST:
            return ("not captured — prompt digest only (sha256 %s, %d chars), text not stored "
                    "(ADR-005 §2.2)" % ((self.intent.prompt_sha256[:12] or "?"),
                                        int(self.intent.prompt_chars or 0)))
        if self.intent_state == INTENT_REFUSED:
            return ("not used — %s is not in `deps.provenance.intent_sources`"
                    % INTENT_LABEL.get(self.intent.source, self.intent.source))
        if self.intent_state == INTENT_GUESS:
            return 'guess (confidence %.2f): "%s"' % (float(self.intent.confidence),
                                                      self.intent.text)
        return "not recorded"

    def as_dict(self) -> dict:
        return {"package": self.package.text, "name": self.package.name,
                "version": self.package.version, "ecosystem": self.package.ecosystem,
                "state": self.state, "answered": self.answered, "episode_id": self.episode_id,
                "commit": self.commit, "intent_state": self.intent_state,
                "intent_answered": self.intent_answered,
                "intent": self.intent.as_dict() if self.intent else None,
                "attribution": self.attribution, "agent": self.agent, "identity": self.identity,
                "confidence": float(self.confidence), "basis": self.basis,
                "missing": self.missing, "remedy": self.remedy, "says": self.says,
                "who": self.who, "reviewed": self.reviewed,
                "edge": edge_path(self.package.asked_by, self.package.text),
                "scope": self.package.scope, "evidence": list(self.evidence),
                "rows": self.rows()}


def resolve(package: Package, input, policy, *, introduced_here: bool) -> Answer:
    """The answer for one package, following the ladder and refusing to invent a rung."""
    episode = input.episode
    if introduced_here:
        return _finish(package, policy, answer=Answer(
            package=package, state=ANSWER_THIS_EPISODE, episode_id=episode.id or "(no id)",
            commit=episode.commit, intent=episode.intent, attribution=episode.attribution,
            agent=episode.agent, identity=episode.identity, confidence=float(episode.confidence),
            basis="added by the episode under review",
            evidence=["episode %s is the episode under review and this package is new in it"
                      % (episode.id or "(no id)")]))
    record = input.introduction_for(package) if policy.flag("prov_search_previous") else None
    if record is not None:
        evidence = ["recorded introduction: %s (basis %s, confidence %.2f)"
                    % (record.episode_id, record.basis, float(record.confidence))]
        if record.commit:
            evidence.append("commit %s" % record.commit)
        recorded = str(record.package).split("|")
        if len(recorded) > 2 and recorded[2] and recorded[2] != package.version:
            evidence.append("the record is for version %s and this is %s; the introducing episode "
                            "is a question about the name, so the older record answers it and says so"
                            % (recorded[2], package.version))
        return _finish(package, policy, answer=Answer(
            package=package, state=ANSWER_RECORDED, episode_id=record.episode_id,
            commit=record.commit, intent=record.intent,
            attribution=(record.intent.attribution if record.intent else ATTR_UNKNOWN),
            agent=(record.intent.agent if record.intent else ""),
            identity=(record.intent.identity if record.intent else ""),
            confidence=float(record.confidence),
            basis="recorded when %s was indexed" % record.episode_id,
            evidence=evidence, reviewed=record.reviewed))
    looked = ("this episode did not add it"
              + (", and `deps.provenance.search_previous_episodes` is off"
                 if not policy.flag("prov_search_previous")
                 else ", and no recorded introduction names it"))
    return Answer(package=package, state=ANSWER_NONE, missing=looked, remedy=REMEDY_EPISODE,
                  confidence=0.0,
                  evidence=["looked for: this episode's own additions, then the recorded "
                            "introductions — %s" % looked])


def _finish(package: Package, policy, answer: Answer) -> Answer:
    """Decide whether the intent half of an answer actually answers."""
    intent = answer.intent
    allowed = {item.strip() for item in policy.words("prov_intent_sources")}
    floor = policy.number("prov_min_confidence")
    if intent is None:
        if policy.flag("prov_agent_counts_as_intent") and answer.attribution != ATTR_UNKNOWN:
            return _with(answer, intent_state=INTENT_ANSWERED,
                         intent=Intent(text="attribution only: %s" % answer.who,
                                       source="manual-note",
                                       confidence=min(answer.confidence, 0.5)),
                         evidence=answer.evidence +
                         ["`deps.provenance.agent_counts_as_intent` is on, so attribution stood "
                          "in for intent — it says who executed the change, not who asked"])
        return _with(answer, intent_state=INTENT_NONE,
                     evidence=answer.evidence +
                     ["no intent evidence of any kind was handed over for this episode"])
    if intent.digest_only:
        # Checked before the source list, on purpose: a prompt digest is not an answer to "what was
        # asked", whatever a repository's `intent_sources` says.
        return _with(answer, intent_state=INTENT_DIGEST,
                     evidence=answer.evidence +
                     ["prompt_sha256=%s, prompt_chars=%d; hooks v1 stores neither the prompt nor "
                      "a recoverable digest of it (ADR-005 §2.2)"
                      % (intent.prompt_sha256 or "?", int(intent.prompt_chars or 0))])
    if intent.source not in allowed:
        return _with(answer, intent_state=INTENT_REFUSED,
                     evidence=answer.evidence +
                     ["intent evidence %s exists but is not in "
                      "`deps.provenance.intent_sources`" % intent.label])
    if not intent.has_text:
        return _with(answer, intent_state=INTENT_EMPTY,
                     evidence=answer.evidence + ["the intent field is present and empty"])
    if float(intent.confidence) < floor:
        return _with(answer, intent_state=INTENT_GUESS,
                     evidence=answer.evidence +
                     ["confidence %.2f is below `deps.provenance.min_confidence` (%.2f), so it is "
                      "printed as a guess rather than as the answer"
                      % (float(intent.confidence), floor)])
    return _with(answer, intent_state=INTENT_ANSWERED,
                 evidence=answer.evidence +
                 ["intent from %s at confidence %.2f" % (intent.label,
                                                        float(intent.confidence))])


def _with(answer: Answer, **changes) -> Answer:
    return replace(answer, **changes)


def is_direct(package: Package) -> bool:
    """The question is asked of direct dependencies; a transitive one is answered by its edge."""
    return package.attribution == DIRECT
