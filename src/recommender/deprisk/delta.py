"""The episode answer: provenance, advisories and licence policy, in one pass, with a card to match.

:func:`measure` is the whole product in one call. It takes the delta `e2.6` computed, the intent
`e4.2` may have captured, an advisory cache somebody read earlier, and a policy — and returns the
three answers a reviewer of a dependency change wants, each with its evidence, each complete when
its inputs are missing, and none of them able to reach the network or the filesystem on the way.

```python
from recommender.deprisk import Policy, measure, parse_input
view = measure(parse_input(payload), Policy.built_in(), cache=my_cache, now=1_760_000_000)
view.summary()["sentence"]
view.verify()                       # [] when every count reconciles
```

What it guarantees, and where the guarantee lives:

* **reconciliation.** :meth:`RiskView.verify` checks the counts column by column: one licence row
  per delta row, one provenance row per new direct dependency, every advisory accounted for as
  either checked or *not-checked-with-a-reason*, every finding either emitted or counted as
  suppressed by its own switch. A number on this card has to be traceable to a row (ADR-001 §7.10).
* **stated confidence.** One discount per stated partial — no cache, no intent, unmatched layer,
  incomplete delta, over-cap lookups — floored, so a card never quietly reads as certain. The same
  shape `e2.8` uses, for the same reason.
* **rendering cannot block.** The advisory half is built from the cache value handed in. There is
  no network call anywhere between :func:`measure` and the last line of the card, which is why
  `advisories --refresh` is a separate verb with its own consent check.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from . import licence as LICENCE
from . import osv as OSV
from .licence import Row
from .model import (
    ADDED, DIRECT, REMOVED, SOURCE_SWITCH, Change, Finding, Input, Package, anchor_for,
    edge_path)
from .policy import (
    ADVISORY_FIX, ADVISORY_MISSING, ADVISORY_STALE, ADVISORY_UNCHECKED, INTENT_NOT_CAPTURED,
    LICENSE_ADDED, LICENSE_DENIED, LICENSE_DRIFT, LICENSE_FLAGGED, LICENSE_UNKNOWN, MODE_DENY,
    MODE_WARN, NEW_DIRECT, NEW_SOURCE, PROVENANCE_UNKNOWN, Policy, VCS_SOURCE, ADVISORY_RULE)
from .provenance import (
    ANSWER_NONE, INTENT_ANSWERED, INTENT_GUESS, Answer, resolve)

SEVERITY_BY_MODE: dict[str, str] = {MODE_DENY: "high", MODE_WARN: "notable"}


@dataclass(frozen=True)
class AdvisoryRow:
    """One package and what the cache said about it — including 'nothing, and here is why'."""

    lookup: OSV.Lookup
    entry: OSV.Entry | None = None
    advisories: tuple[OSV.Advisory, ...] = ()
    checked: bool = False
    reason: str = ""

    @property
    def age(self) -> int:
        return -1 if self.entry is None else self.entry.fetched_at

    @property
    def failed(self) -> bool:
        """The last query about this package failed.

        Separate from 'checked and clean' on purpose: an empty result and a failed request are two
        different facts, and printing the second as the first is exactly the kind of quiet lie this
        package exists to avoid.
        """
        return self.entry is not None and self.entry.status == OSV.STATUS_ERROR

    @property
    def worst(self) -> str:
        order = ("critical", "high", "moderate", "low", "none", "unknown")
        ranked = [item.severity for item in self.advisories]
        ranked.sort(key=lambda one: order.index(one) if one in order else len(order))
        return ranked[0] if ranked else ""

    def as_dict(self, now: int = 0) -> dict:
        return {"package": "%s %s" % (self.lookup.package, self.lookup.version),
                "ecosystem": self.lookup.osv_ecosystem, "checked": self.checked,
                "reason": self.reason, "advisories": [one.as_dict() for one in self.advisories],
                "worst": self.worst,
                "fetched_at": self.entry.fetched_at if self.entry else 0,
                "age_seconds": self.entry.age_seconds(now) if self.entry else -1,
                "scope": self.lookup.scope, "edge": list(self.lookup.edge)}


@dataclass
class RiskView:
    """Everything one episode's dependency-risk answer says, and the counts that prove it."""

    episode: Input
    policy: Policy
    now: int = 0
    answers: list[Answer] = field(default_factory=list)
    rows: list[Row] = field(default_factory=list)
    advisory_rows: list[AdvisoryRow] = field(default_factory=list)
    coverage: list[OSV.Coverage] = field(default_factory=list)
    freshness: OSV.Freshness = field(default_factory=OSV.Freshness)
    plan: OSV.Plan = field(default_factory=OSV.Plan)
    findings: list[Finding] = field(default_factory=list)
    suppressed: dict[str, int] = field(default_factory=dict)
    new_licences: list[tuple[str, str]] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    partials: list[str] = field(default_factory=list)
    cache_exists: bool = True
    cache_path: str = ""

    # -- the basics ---------------------------------------------------------
    @property
    def delta(self):
        return self.episode.delta

    @property
    def changes(self) -> tuple[Change, ...]:
        return self.delta.changes

    @property
    def new_direct(self) -> tuple[Change, ...]:
        return tuple(change for change in self.changes
                     if change.kind == ADDED and change.package.attribution == DIRECT)

    @property
    def new_transitive(self) -> tuple[Change, ...]:
        return tuple(change for change in self.changes
                     if change.kind == ADDED and change.package.attribution != DIRECT)

    def findings_of(self, kind: str) -> list[Finding]:
        return [finding for finding in self.findings if finding.kind == kind]

    def answer_for(self, package: str) -> Answer | None:
        for answer in self.answers:
            if answer.package.text == package or answer.package.name == package:
                return answer
        return None

    # -- counts -------------------------------------------------------------
    def counts(self) -> dict:
        by_severity: dict[str, int] = {}
        for finding in self.findings:
            by_severity[finding.severity] = by_severity.get(finding.severity, 0) + 1
        checked = [row for row in self.advisory_rows if row.checked]
        unchecked = [row for row in self.advisory_rows if not row.checked]
        advisories = [one for row in checked for one in row.advisories]
        return {
            "changes": len(self.changes),
            "new_direct": len(self.new_direct),
            "new_transitive": len(self.new_transitive),
            "removed": len(self.delta.of_kind(REMOVED)),
            "upgraded": len(self.delta.of_kind("upgraded")) +
            len(self.delta.of_kind("downgraded")) +
            len(self.delta.of_kind("version_changed")),
            "source_switches": len(self.delta.of_kind(SOURCE_SWITCH)),
            "new_sources": len(self.delta.sources),
            "licence_rows": len(self.rows),
            "licence_violations": len([row for row in self.rows if row.violation]),
            "licence_denied": len(self.findings_of(LICENSE_DENIED)),
            "licence_flagged": len(self.findings_of(LICENSE_FLAGGED)),
            "licence_unknown": len(self.findings_of(LICENSE_UNKNOWN)),
            "licence_drift": len(self.findings_of(LICENSE_DRIFT)),
            "licence_classes": LICENCE.class_counts(self.rows),
            "licence_ids_new": len(self.new_licences),
            "provenance_rows": len(self.answers),
            "provenance_answered": len([one for one in self.answers if one.answered]),
            "intent_answered": len([one for one in self.answers if one.intent_answered]),
            "advisories_checked": len(checked),
            "advisories_clean": len([row for row in checked if not row.advisories
                                     and not row.failed]),
            "advisories_failed": len([row for row in checked if row.failed]),
            "advisories_unchecked": len(unchecked),
            "advisories": len(advisories),
            "advisories_critical": len([one for one in advisories if one.severity == "critical"]),
            "advisories_high": len([one for one in advisories if one.severity == "high"]),
            "advisories_with_fix": len([one for one in advisories if one.has_fix]),
            "findings": len(self.findings),
            "findings_by_severity": by_severity,
            "suppressed": sum(self.suppressed.values()),
            "suppressed_by_rule": dict(self.suppressed),
            "partials": len(self.partials),
        }

    def verify(self) -> list[str]:
        """Every reconciliation this card can fail. Empty means the numbers add up."""
        problems: list[str] = []
        counts = self.counts()
        if counts["licence_rows"] != len(self.changes):
            problems.append("licence rows (%d) do not match the delta rows (%d)"
                            % (counts["licence_rows"], len(self.changes)))
        for answer in self.answers:
            if answer.package.attribution != DIRECT and \
                    answer.package.text not in self.episode.asked and \
                    answer.package.name not in self.episode.asked:
                problems.append("provenance row for %s: not a direct dependency and not asked "
                                "about" % answer.package.text)
        expected = {change.subject for change in self.new_direct}
        missing_rows = sorted(expected - {answer.package.text for answer in self.answers})
        if missing_rows:
            problems.append("no provenance row for new direct dependenc%s %s"
                            % ("y" if len(missing_rows) == 1 else "ies",
                               ", ".join(missing_rows)))
        if self.freshness.checked != counts["advisories_checked"] or \
                self.freshness.missed != counts["advisories_unchecked"]:
            problems.append("the freshness line (%d checked, %d missed) does not match the rows "
                            "(%d checked, %d missed)" % (self.freshness.checked,
                                                         self.freshness.missed,
                                                         counts["advisories_checked"],
                                                         counts["advisories_unchecked"]))
        severities = ("critical", "high", "notable", "note")
        if sum(counts["findings_by_severity"].values()) != counts["findings"]:
            problems.append("findings by severity do not sum to the finding count")
        for finding in self.findings:
            if finding.severity not in severities:
                problems.append("%s: severity %r is not a known severity"
                                % (finding.kind, finding.severity))
            if not finding.evidence:
                problems.append("%s %s: a finding with no evidence" % (finding.kind,
                                                                      finding.subject))
            if not finding.path:
                problems.append("%s %s: a finding with no path" % (finding.kind,
                                                                  finding.subject))
            if not self.policy.enabled_for(finding.kind):
                problems.append("%s was emitted while its switch is off" % finding.kind)
        # Advisory accounting: every planned lookup is either checked or has a stated reason.
        if len(self.coverage) != len(self.plan.lookups):
            problems.append("coverage rows (%d) do not match planned lookups (%d)"
                            % (len(self.coverage), len(self.plan.lookups)))
        if len(self.advisory_rows) != len(self.coverage):
            problems.append("advisory rows (%d) do not match coverage rows (%d)"
                            % (len(self.advisory_rows), len(self.coverage)))
        if counts["advisories_checked"] + counts["advisories_unchecked"] != len(self.plan.lookups):
            problems.append("checked + unchecked (%d) does not match lookups (%d)"
                            % (counts["advisories_checked"] + counts["advisories_unchecked"],
                               len(self.plan.lookups)))
        for row in self.rows:
            if row.state == LICENCE.STATE_REMOVED:
                continue
            if row.verdict.mode not in ("deny", "warn", "allow"):
                problems.append("%s: verdict mode %r is not a matrix value"
                                % (row.package.text, row.verdict.mode))
        return problems

    @property
    def confidence(self) -> float:
        """One discount per stated partial, floored — a card may not read as certain by default."""
        return max(0.5, round(1.0 - 0.1 * len(self.partials), 2))

    def summary(self) -> dict:
        """One sentence, built only from clauses whose numbers are in :meth:`counts`."""
        counts = self.counts()
        clauses: list[str] = []
        if counts["changes"] == 0:
            clauses.append("no dependency change")
        else:
            if counts["new_direct"]:
                names = ", ".join(change.subject for change in self.new_direct[:3])
                extra = "" if counts["new_direct"] <= 3 else " (+%d more)" % (
                    counts["new_direct"] - 3)
                clauses.append("%d new direct dependenc%s (%s%s)"
                               % (counts["new_direct"], "y" if counts["new_direct"] == 1 else "ies",
                                  names, extra))
            if counts["new_transitive"]:
                clauses.append("%d transitive added" % counts["new_transitive"])
            if counts["upgraded"]:
                clauses.append("%d version change(s)" % counts["upgraded"])
            if counts["removed"]:
                clauses.append("%d removed" % counts["removed"])
            if counts["source_switches"]:
                clauses.append("%d source switch(es)" % counts["source_switches"])
        if counts["provenance_rows"]:
            answered = [one for one in self.answers if one.answered]
            with_intent = [one for one in answered if one.intent_answered]
            if answered and len(with_intent) == len(answered):
                clauses.append("intent recorded for all of them")
            elif with_intent:
                clauses.append("intent recorded for %d of %d" % (len(with_intent), len(answered)))
            elif answered:
                clauses.append("no intent recorded")
        if counts["advisories"]:
            clause = "%d advisor%s (%d critical, %d high, %d with a fix)" % (
                counts["advisories"], "y" if counts["advisories"] == 1 else "ies",
                counts["advisories_critical"], counts["advisories_high"],
                counts["advisories_with_fix"])
            clauses.append(clause)
        elif counts["advisories_clean"] and self.freshness.claims_clean:
            clauses.append("no advisories found for %d package(s) checked"
                           % counts["advisories_clean"])
        elif counts["advisories_clean"]:
            clauses.append("%d package(s) checked against a cache that is too old to call clean"
                           % counts["advisories_clean"])
        if counts["advisories_failed"]:
            clauses.append("%d package(s) the last query failed for" % counts["advisories_failed"])
        if counts["advisories_unchecked"]:
            clauses.append("%d package(s) not checked" % counts["advisories_unchecked"])
        violations = counts["licence_denied"] + counts["licence_flagged"]
        if violations:
            clauses.append("licence: %d refused, %d need a decision"
                           % (counts["licence_denied"], counts["licence_flagged"]))
        if counts["licence_unknown"]:
            clauses.append("%d with no licence recorded" % counts["licence_unknown"])
        if counts["licence_drift"]:
            clauses.append("%d licence change(s) on existing packages" % counts["licence_drift"])
        if not self.delta.complete:
            clauses.append("the delta was reported incomplete")
        sentence = "%s · %s" % (self.episode.episode.id or "episode", "; ".join(clauses)
                                or "nothing dependency-risk-shaped")
        return {"episode_id": self.episode.episode.id, "sentence": sentence,
                "clauses": clauses, "confidence": self.confidence,
                "freshness": self.freshness.words,
                "headline": self.delta.headline,
                "partials": list(self.partials)}

    def as_dict(self) -> dict:
        counts = self.counts()
        return {"episode_id": self.episode.episode.id,
                "schema": "fathom.deprisk.v1",
                "policy": self.policy.as_dict(),
                "summary": self.summary(),
                "counts": counts,
                "provenance": [answer.as_dict() for answer in self.answers],
                "licenses": [row.as_dict() for row in self.rows],
                "licenses_new_in_set": [{"id": one, "brought_by": who}
                                        for one, who in self.new_licences],
                "advisories": {"freshness": self.freshness.as_dict(),
                               "cache": {"path": self.cache_path, "exists": self.cache_exists,
                                         "network": self.policy.network.as_dict()},
                               "rows": [row.as_dict(self.now) for row in self.advisory_rows],
                               "plan": self.plan.as_dict()},
                "findings": [finding.as_dict() for finding in self.findings],
                "suppressed": dict(self.suppressed),
                "notes": list(self.notes),
                "partials": list(self.partials),
                "confidence": self.confidence,
                "reconciles": not self.verify(),
                "problems": self.verify()}


def measure(episode: Input, policy: Policy | None = None, *, cache: OSV.Cache | None = None,
            now: int = 0) -> RiskView:
    """The whole answer for one episode. Pure: no filesystem, no clock, no socket."""
    active = policy or Policy.built_in()
    view = RiskView(episode=episode, policy=active, now=int(now))
    packages = [change.after for change in episode.delta.changes if change.after is not None]

    view.rows = LICENCE.evaluate(episode.delta, active)
    view.answers = _answers(episode, active)
    view.plan = OSV.plan(_advisory_candidates(episode, packages), active, cache)
    view.coverage = OSV.coverage(list(view.plan.lookups), cache, active)
    view.freshness = OSV.freshness(view.coverage, cache, active, int(now))
    view.advisory_rows = [AdvisoryRow(lookup=row.lookup, entry=row.entry, checked=row.checked,
                                      reason=row.reason,
                                      advisories=tuple(row.entry.advisories) if row.entry else ())
                          for row in view.coverage]
    view.new_licences = LICENCE.newly_seen_ids(view.rows, episode.delta, active)
    view.cache_exists = bool(cache and cache.exists)
    view.cache_path = (cache.path if cache and cache.path else active.text("adv_cache"))
    view.notes = _notes(episode, active, cache)
    view.partials = _partials(view, active, cache)
    view.findings, view.suppressed = _findings(view, active)
    return view


def _answers(episode: Input, policy: Policy) -> list[Answer]:
    """One provenance row per new direct dependency — the question the ticket poses verbatim."""
    answers: list[Answer] = []
    for change in episode.delta.changes:
        if change.kind != ADDED or change.after is None:
            continue
        if change.after.attribution != DIRECT:
            continue
        answers.append(resolve(change.after, episode, policy, introduced_here=True))
    # A dependency the caller says was added earlier still deserves its row when it shows up as a
    # version change in this episode: the reviewer's question is about the package, not about the
    # row that happened to mention it.
    mentioned = {answer.package.name for answer in answers}
    for change in episode.delta.changes:
        package = change.after
        if package is None or package.attribution != DIRECT or package.name in mentioned:
            continue
        if episode.introduction_for(package) is None:
            continue
        answers.append(resolve(package, episode, policy, introduced_here=False))
        mentioned.add(package.name)
    # And anything the caller asked about by name — "who introduced `old-crypto`?" is a question
    # about a package, not about the episode that happened to mention it.
    for name in episode.asked:
        text = str(name).strip()
        if not text or text in mentioned:
            continue
        package = _find(episode, text)
        if package is None:
            package = Package(name=text.split(" ")[0],
                              version=text.split(" ")[1] if " " in text else "",
                              attribution=DIRECT)
        answers.append(resolve(package, episode, policy, introduced_here=False))
        mentioned.add(package.name)
    return answers


def _find(episode: Input, text: str) -> Package | None:
    """A named package, from the delta when it is in it, from its name when it is not."""
    for change in episode.delta.changes:
        candidate = change.after or change.before
        if candidate is None:
            continue
        if candidate.text == text or candidate.name == text:
            return candidate
    name, _, version = text.partition(" ")
    return Package(name=name, version=version, attribution=DIRECT)


def _advisory_candidates(episode: Input, packages: list[Package]) -> list[Package]:
    """Packages worth asking OSV about: what the repository now depends on, not what it dropped."""
    return [change.after for change in episode.delta.changes
            if change.after is not None and change.kind != REMOVED]


def _notes(episode: Input, policy: Policy, cache: OSV.Cache | None) -> list[str]:
    notes = [str(item) for item in episode.notes]
    notes += [str(item) for item in episode.delta.notes]
    if not policy.network.may_fetch:
        notes.append("advisories served from cache only: %s" % policy.network.reason)
    if cache is not None and cache.error:
        notes.append("advisory cache: %s" % cache.error)
    return notes


def _partials(view: RiskView, policy: Policy, cache: OSV.Cache | None) -> list[str]:
    """Every place this answer is missing something, in as many words, before any row is drawn."""
    partials: list[str] = []
    if policy.text("adv_mode") == "offline":
        partials.append("advisories were not consulted (`deps.advisories.mode = \"offline\"`)")
    elif cache is None or not cache.exists:
        partials.append("no advisory cache was read, so no package was checked against OSV")
    elif view.freshness.state == "expired":
        partials.append("the advisory cache is %s old, past `stale_limit_hours`"
                        % view.freshness.label)
    elif view.freshness.state == "stale":
        partials.append("the advisory cache is %s old, past `max_age_hours`"
                        % view.freshness.label)
    if any(not answer.answered for answer in view.answers):
        partials.append("%d new direct dependency(ies) have no recorded introduction"
                        % len([one for one in view.answers if one.state == ANSWER_NONE]))
    if any(not answer.intent_answered for answer in view.answers):
        partials.append("%d introducing episode(s) have no usable intent"
                        % len([one for one in view.answers if not one.intent_answered]))
    unmatched = sorted({row.package.scope for row in view.rows
                        if row.verdict.basis == "unmatched" and row.package.scope})
    if unmatched:
        partials.append("%d manifest path(s) matched no layer in `[layers]`, so `%s` decided"
                        % (len(unmatched), "the strict rule (`unknown_layer_policy = \"strict\"`)"
                           if policy.text("lic_unknown_layer") != "lenient"
                           else "the lenient rule"))
    if not view.delta.complete:
        partials.append("the dependency delta reported itself incomplete, so this answer is a "
                        "subset of the episode")
    if view.plan.over_cap:
        partials.append("%d package(s) were over `deps.advisories.max_packages` and were not "
                        "checked" % len(view.plan.over_cap))
    if cache is not None and cache.error:
        partials.append("the advisory cache could not be read: %s" % cache.error)
    for row in view.rows:
        if row.state == LICENCE.STATE_DEV_SKIPPED:
            partials.append("development-only packages were skipped by `ignore_dev`")
            break
    return partials


def _findings(view: RiskView, policy: Policy) -> tuple[list[Finding], dict[str, int]]:
    """Every finding this episode raises, filtered by its own switch — with the refusals counted."""
    raised: list[Finding] = []
    raised.extend(_provenance_findings(view, policy))
    raised.extend(_licence_findings(view, policy))
    raised.extend(_advisory_findings(view, policy))
    raised.extend(_source_findings(view, policy))
    suppressed: dict[str, int] = {}
    kept: list[Finding] = []
    for finding in raised:
        if policy.enabled_for(finding.kind):
            kept.append(finding)
        else:
            suppressed[finding.kind] = suppressed.get(finding.kind, 0) + 1
    kept.sort(key=_order)
    return kept, suppressed


def _order(finding: Finding) -> tuple:
    order = ("critical", "high", "notable", "note")
    index = order.index(finding.severity) if finding.severity in order else len(order)
    return (index, finding.kind, finding.location, finding.subject)


def _provenance_findings(view: RiskView, policy: Policy) -> list[Finding]:
    findings: list[Finding] = []
    for answer in view.answers:
        package = answer.package
        findings.append(Finding(
            kind=NEW_DIRECT, subject=package.text, path=package.scope or "./",
            detail=answer.says, edge=package.asked_by, ecosystem=package.ecosystem,
            package=package.name, version=package.version,
            introduced_by=answer.episode_id,
            intent=(answer.intent.text if answer.intent and answer.intent_answered else ""),
            confidence=max(float(answer.confidence), 0.01) if answer.answered else 0.5,
            evidence=list(answer.evidence) or [answer.says],
            numbers={"state": answer.state, "intent_state": answer.intent_state,
                     "attribution": answer.attribution}))
        if answer.state == ANSWER_NONE:
            findings.append(Finding(
                kind=PROVENANCE_UNKNOWN, subject=package.text, path=package.scope or "./",
                detail="Nothing records which episode introduced this dependency: %s"
                       % answer.missing,
                edge=package.asked_by, ecosystem=package.ecosystem, package=package.name,
                version=package.version, confidence=0.5,
                evidence=list(answer.evidence) + ["what would fill it: %s" % answer.remedy],
                numbers={"looked_for": ["this episode", "recorded introductions"]}))
        elif not answer.intent_answered and policy.flag("prov_require_intent"):
            findings.append(Finding(
                kind=INTENT_NOT_CAPTURED, subject=package.text, path=package.scope or "./",
                detail="Episode %s introduced this dependency and its intent was not captured."
                       % answer.episode_id,
                edge=package.asked_by, ecosystem=package.ecosystem, package=package.name,
                version=package.version, introduced_by=answer.episode_id,
                confidence=max(float(answer.confidence), 0.01),
                evidence=list(answer.evidence) + ["what would fill it: %s" % answer.intent_note],
                numbers={"intent_state": answer.intent_state}))
    return findings


def _licence_findings(view: RiskView, policy: Policy) -> list[Finding]:
    findings: list[Finding] = []
    for row in view.rows:
        if row.state == LICENCE.STATE_DRIFT:
            findings.append(Finding(
                kind=LICENSE_DRIFT, subject=row.package.text, path=row.anchor,
                detail="Licence was %s before this episode and is %s now."
                       % (row.before_licence or "nothing", row.licence_label),
                edge=row.package.asked_by, ecosystem=row.package.ecosystem,
                package=row.package.name, version=row.package.version, layer=row.verdict.layer,
                licence=row.package.licence,
                evidence=list(row.evidence) +
                           ["before: %s" % (row.before_licence or "nothing declared"),
                            "after: %s" % (row.package.licence or "nothing declared")],
                numbers={"before": row.before_licence, "after": row.package.licence}))
        if not row.violation:
            continue
        kind = row.rule_id
        if kind not in (LICENSE_DENIED, LICENSE_FLAGGED, LICENSE_UNKNOWN):
            continue
        findings.append(Finding(
            kind=kind, subject="%s — %s" % (row.package.text, row.licence_label),
            path=row.anchor, detail=row.verdict.why, edge=row.package.asked_by,
            severity_override=SEVERITY_BY_MODE.get(row.verdict.mode, "notable"),
            ecosystem=row.package.ecosystem, package=row.package.name,
            version=row.package.version, layer=row.verdict.layer,
            licence=row.package.licence,
            confidence=1.0 if row.verdict.basis != "unmatched" else 0.9,
            evidence=row.evidence + ["offending edge path: %s" % row.edge] +
                     (["alternatives not taken: %s" % ", ".join(row.verdict.expression.rejected)]
                      if row.verdict.expression.rejected else []),
            numbers={"class": row.verdict.cls, "mode": row.verdict.mode,
                     "library": row.verdict.library, "state": row.state,
                     "knob": row.verdict.knob}))
    for licence_id, who in view.new_licences:
        findings.append(Finding(
            kind=LICENSE_ADDED, subject=licence_id, path="./",
            detail="%s is newly in the set of licences this repository ships." % licence_id,
            licence=licence_id, confidence=1.0,
            evidence=["brought by: %s" % who,
                      "the delta model's licence-set delta and this episode's added packages "
                      "both feed this row"]))
    return findings


def _advisory_findings(view: RiskView, policy: Policy) -> list[Finding]:
    findings: list[Finding] = []
    for row in view.advisory_rows:
        if not row.checked:
            continue
        subject_package = "%s %s" % (row.lookup.package, row.lookup.version)
        for advisory in row.advisories:
            findings.append(Finding(
                kind=ADVISORY_RULE.get(advisory.severity, "advisory-moderate"),
                subject="%s · %s" % (subject_package, advisory.id),
                path=row.lookup.scope or "./",
                detail="%s" % (advisory.summary or "no summary recorded upstream"),
                edge=row.lookup.edge, ecosystem=row.lookup.ecosystem or row.lookup.osv_ecosystem,
                package=row.lookup.package, version=row.lookup.version,
                advisory_id=advisory.id, fix=advisory.fixed_in,
                confidence=0.9,
                evidence=["%s: %s" % (advisory.id, advisory.summary or "no summary"),
                          "severity: %s%s" % (advisory.severity,
                                              " (upstream %r)" % advisory.upstream_severity
                                              if advisory.upstream_severity else ""),
                          "database: %s" % (advisory.database or "OSV"),
                          "cached %s ago" % (OSV.age_text(max(0, view.now - advisory_age(row))))]
                         + (["reference: %s" % advisory.url] if advisory.url else []),
                numbers={"severity": advisory.severity, "has_fix": advisory.has_fix,
                         "fixed_in": advisory.fixed_in,
                         "aliases": list(advisory.aliases)[:4]}))
            if advisory.has_fix:
                findings.append(Finding(
                    kind=ADVISORY_FIX, subject="%s → %s" % (subject_package, advisory.fixed_in),
                    path=row.lookup.scope or "./",
                    detail="%s is fixed in %s, so the action is a bump."
                           % (advisory.id, advisory.fixed_in),
                    edge=row.lookup.edge, ecosystem=row.lookup.ecosystem,
                    package=row.lookup.package, version=row.lookup.version,
                    advisory_id=advisory.id, fix=advisory.fixed_in, confidence=0.9,
                    evidence=["%s names %s as fixed" % (advisory.id, advisory.fixed_in),
                              "installed: %s" % row.lookup.version]))
    # Everything not checked, grouped by the reason, so "no advisories found" is never bare.
    groups: dict[str, list[AdvisoryRow]] = {}
    for row in view.advisory_rows:
        if not row.checked:
            groups.setdefault(row.reason, []).append(row)
    for _reason, rows in sorted(groups.items()):
        anchor = next((row.lookup.scope for row in rows if row.lookup.scope), "./")
        findings.append(Finding(
            kind=ADVISORY_UNCHECKED, subject="%d package(s)" % len(rows), path=anchor_for(anchor),
            detail="Not checked against OSV: %s" % _reason,
            confidence=1.0,
            evidence=["reason: %s" % _reason] +
                     ["%s %s (%s)" % (row.lookup.package, row.lookup.version,
                                      row.lookup.osv_ecosystem) for row in rows[:6]] +
                     (["… %d more (`--json`)" % (len(rows) - 6)] if len(rows) > 6 else []) +
                     [("fill it: `fathom deprisk advisories --refresh` (needs "
                       "`privacy.advisories` consent) or import a cache")],
            numbers={"packages": len(rows), "reason": _reason}))
    for reason, names in view.plan.skipped:
        if reason in groups:
            continue
        findings.append(Finding(
            kind=ADVISORY_UNCHECKED, subject="%d package(s)" % len(names), path="./",
            detail="Not checked against OSV: %s" % reason, confidence=1.0,
            evidence=["reason: %s" % reason] + ["%s" % name for name in list(names)[:6]] +
                     (["… %d more (`--json`)" % (len(names) - 6)] if len(names) > 6 else []),
            numbers={"packages": len(names), "reason": reason}))
    if view.freshness.state in ("stale", "expired"):
        findings.append(Finding(
            kind=ADVISORY_STALE, subject=view.freshness.label, path=view.freshness.path or "./",
            detail=view.freshness.words, confidence=1.0,
            evidence=["cache: %s" % (view.freshness.path or "not configured"),
                      "age: %s (`max_age_hours` %d h, `stale_limit_hours` %d h)"
                      % (view.freshness.label, view.freshness.max_age_hours,
                         view.freshness.stale_limit_hours),
                      "claims_clean: %s" % ("yes — 'no advisories found' is a claim this answer "
                                            "may make" if view.freshness.claims_clean
                                            else "no — absence is reported as 'not checked "
                                                 "recently'")]))
    if view.freshness.state in ("missing", "offline"):
        findings.append(Finding(
            kind=ADVISORY_MISSING, subject=view.freshness.path or "the configured cache",
            path=view.freshness.path or "./", detail=view.freshness.words, confidence=1.0,
            evidence=[view.freshness.words,
                      "consent: %s" % policy.network.reason,
                      "the card renders regardless: advisories are an enrichment, not a "
                      "precondition"]))
    return findings


def advisory_age(row: AdvisoryRow) -> int:
    return int(row.entry.fetched_at) if row.entry else 0


def _source_findings(view: RiskView, policy: Policy) -> list[Finding]:
    findings: list[Finding] = []
    for source in view.delta.sources:
        findings.append(Finding(
            kind=NEW_SOURCE, subject=source.identity, path=source.scope or "./",
            detail="This repository resolved from %s for the first time in this episode."
                   % source.identity,
            confidence=1.0,
            evidence=["source kind: %s" % (source.kind or "unknown"),
                      "first package seen from it: %s" % (source.first_package or "not recorded"),
                      "declared in: %s" % (source.scope or "not recorded")],
            numbers={"kind": source.kind}))
    for row in view.rows:
        if row.state == LICENCE.STATE_REMOVED or not row.package.is_vcs:
            continue
        findings.append(Finding(
            kind=VCS_SOURCE, subject=row.package.text, path=row.anchor,
            detail="Resolved from %s: no immutable version, no registry signature."
                   % row.package.source_display,
            edge=row.package.asked_by, ecosystem=row.package.ecosystem,
            package=row.package.name, version=row.package.version, layer=row.verdict.layer,
            confidence=1.0,
            evidence=["source: %s (kind %s)" % (row.package.source_display,
                                                row.package.source_kind or "unknown"),
                      "declared in: %s" % (row.package.scope or "not recorded"),
                      "offending edge path: %s" % row.edge]))
    return findings
