"""The episode: fold the noise, then collapse what is left over, and say why both happened.

One call answers the question a reviewer actually asks of an episode:

    from recommender.noise import Episode, Change, suppress

    view = suppress(Episode(id="ep-4f2a", files=[Change(path=…, before=…, after=…)]))
    view.headline()      # 'nothing to review: 0 semantic changes across 3 files (rustfmt)'
    view.counts()        # {'files': 3, 'folded_files': 3, 'semantic_lines': 0, …}
    view.as_dict()       # the same facts as structure, for the store and `e2.10`

Three things are decided here and nowhere else, so that the diff tree, the timeline, the
card and the export cannot each grow a slightly different answer:

**Which rule folds a file.** Several detectors may speak for one file — a regenerated
protobuf stub is `generated-churn` *and* usually a reformat *and* its path is generated
noise. Every matching detector is kept (they are all printed under the row, because the
reviewer is entitled to the whole story), and one is named as the primary in rule order, so
the one-line summary is stable. A detector that fires but is disabled by policy, or is
overridden by `noise.keep`, leaves the file visible *and says which of the two it was* —
"the rule fired and you turned it off" is a different sentence from "nothing fired".

**Whether the episode collapses.** Only when every file folded. One visible file is enough
to keep the episode expanded, because the visible file is the reason the episode exists and
a collapse would bury it under a summary of the noise around it.

**What the line says.** Wording follows the evidence: all-reformat episodes say *0 semantic
changes* (AC 1), all-lockfile episodes name the moved packages and say no manifest moved
(AC 2), and everything else says which rules folded what. Each ends with the key that
expands it.

Like the rest of the package this is pure: no clock, no filesystem, no store. Episode
history — "this file has been regenerated in four of the last six episodes" — arrives as a
number the caller supplies and is used as corroboration in evidence, never as a gate.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from recommender.change import markers as M
from recommender.change.engine import Change as ChangeBlob, ChangeClassifier
from recommender.taxonomy import classes as C

from . import generated as G
from . import lockfile as L
from . import reformat as R
from .policy import (DEFAULT_EXPAND_KEY, GENERATED_CHURN, LOCKFILE, NOISE_ONLY, REFORMAT,
                     RULE_INDEX, USER_GLOB, VENDORED, Policy, Session, Suppression)

#: Rule order for choosing the primary fold of a file. Most specific evidence first.
PRIMARY_ORDER: tuple[str, ...] = (REFORMAT, GENERATED_CHURN, LOCKFILE, VENDORED, USER_GLOB)


@dataclass(frozen=True)
class Change:
    """One file in an episode: the path and whichever revisions the caller has."""

    path: str
    before: str | None = None
    after: str | None = None
    change_type: str = "modified"
    old_path: str = ""
    added_lines: int = 0
    deleted_lines: int = 0

    @property
    def subject(self) -> str:
        return self.path or self.old_path

    @property
    def content(self) -> str | None:
        return self.after if self.after is not None else self.before

    @property
    def line_count(self) -> int:
        """Changed lines, from the caller if it has real diff stats, else from the pair."""
        if self.added_lines or self.deleted_lines:
            return self.added_lines + self.deleted_lines
        verdict = R.compare(self.subject, self.before, self.after,
                            change_type=self.change_type)
        return verdict.changed_lines


@dataclass(frozen=True)
class Episode:
    """A change set: one episode's file pairs, in review order."""

    id: str
    files: tuple[Change, ...] = ()
    title: str = ""

    @classmethod
    def from_pairs(cls, episode_id: str, pairs: dict[str, tuple[str | None, str | None]],
                   **kwargs) -> "Episode":
        return cls(id=episode_id,
                   files=tuple(Change(path=path, before=before, after=after)
                               for path, (before, after) in sorted(pairs.items())),
                   **kwargs)

    @property
    def paths(self) -> list[str]:
        return [change.subject for change in self.files]


@dataclass
class FileNoise:
    """One file's full noise story: what fired, what folded, and what stopped it."""

    path: str
    cls: str = ""
    noise: str = "informative"
    band: str = ""
    confidence: float = 0.0
    change_type: str = "modified"
    reformat: R.Reformat | None = None
    generated: G.Generated | None = None
    lock: L.LockfileSummary | None = None
    suppression: Suppression | None = None
    fired: list[str] = field(default_factory=list)
    refused: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    @property
    def rule(self) -> str:
        return self.suppression.rule if self.suppression else ""

    @property
    def folded(self) -> bool:
        return self.suppression is not None

    @property
    def semantic_lines(self) -> int:
        if self.suppression:
            return 0
        return self.reformat.semantic_lines if self.reformat else 0

    @property
    def changed_lines(self) -> int:
        return self.reformat.changed_lines if self.reformat else 0

    @property
    def detail(self) -> list[str]:
        """The detector lines, the rule that folded first. What the expanded row prints.

        Ordering is not decoration: a lockfile folded by its own rule should not lead with
        "there is no token model for .lock", which is true, irrelevant, and the sort of
        noise that makes an evidence block unreadable. The reformat lines appear when the
        reformat claim is itself part of the answer.
        """
        out: list[str] = []
        rule = self.rule
        if rule == LOCKFILE and self.lock:
            out.append(self.lock.headline())
        if self.reformat and (rule == REFORMAT or self.reformat.claim in
                             (R.REFORMAT, R.CONTENT, R.IDENTICAL)):
            out.extend(self.reformat.evidence())
        if rule == GENERATED_CHURN and self.generated:
            out.extend(self.generated.evidence[:4])
        elif self.generated and self.generated.claim != G.NOT_GENERATED and self.generated.evidence:
            out.append(self.generated.evidence[0])
        if self.lock and rule != LOCKFILE:
            out.append(self.lock.headline())
        for note in self.notes:
            out.append("note: %s" % note)
        for refusal in self.refused:
            out.append("not folded: %s" % refusal)
        return out

    def as_dict(self) -> dict:
        return {"path": self.path, "class": self.cls, "noise": self.noise, "band": self.band,
                "confidence": round(self.confidence, 2), "change_type": self.change_type,
                "folded": self.folded, "rule": self.rule, "fired": list(self.fired),
                "refused": list(self.refused), "notes": list(self.notes),
                "changed_lines": self.changed_lines, "semantic_lines": self.semantic_lines,
                "reformat": self.reformat.as_dict() if self.reformat else None,
                "generated": self.generated.as_dict() if self.generated else None,
                "lock": self.lock.as_dict() if self.lock else None,
                "suppression": self.suppression.as_dict() if self.suppression else None}


@dataclass
class EpisodeNoise:
    """The suppressed view of one episode."""

    episode_id: str
    title: str = ""
    files: list[FileNoise] = field(default_factory=list)
    policy: Policy = field(default_factory=Policy.built_in)
    collapse: Suppression | None = None
    collapse_reason: str = ""
    history: dict[str, int] = field(default_factory=dict)

    # -- counts -------------------------------------------------------------
    def counts(self) -> dict:
        folded = [item for item in self.files if item.folded]
        by_rule: dict[str, int] = {}
        by_formatter: dict[str, int] = {}
        for item in folded:
            by_rule[item.rule] = by_rule.get(item.rule, 0) + 1
            if item.reformat and item.reformat.formatter and item.rule == REFORMAT:
                name = item.reformat.formatter
                by_formatter[name] = by_formatter.get(name, 0) + 1
        return {"files": len(self.files), "folded_files": len(folded),
                "visible_files": len(self.files) - len(folded),
                "semantic_files": len(self.files) - len(folded),
                "semantic_lines": sum(item.semantic_lines for item in self.files),
                "folded_lines": sum(item.changed_lines for item in folded),
                "changed_lines": sum(item.changed_lines for item in self.files),
                "by_rule": dict(sorted(by_rule.items())),
                "by_formatter": dict(sorted(by_formatter.items())),
                "collapsed": self.collapse is not None}

    @property
    def noise_only(self) -> bool:
        """Every file folded. The precondition for the collapse, and nothing else."""
        return bool(self.files) and all(item.folded for item in self.files)

    @property
    def rules_used(self) -> list[str]:
        seen: list[str] = []
        for item in self.files:
            if item.folded and item.rule not in seen:
                seen.append(item.rule)
        return sorted(seen)

    @property
    def semantic_changes(self) -> int:
        return self.counts()["semantic_lines"]

    # -- the lines a human reads -------------------------------------------
    def headline(self) -> str:
        """The single line the episode renders as, expanded or collapsed."""
        counts = self.counts()
        if self.collapse:
            return self.collapse.render()
        if not counts["semantic_lines"] and counts["folded_files"]:
            return "%d file(s), %d semantic change(s): %s" % (
                counts["files"], counts["semantic_lines"], self._fold_phrase(counts))
        return "%d file(s), %d semantic change(s), %d folded: %s" % (
            counts["files"], counts["semantic_lines"], counts["folded_files"],
            self._fold_phrase(counts) or "nothing folded")

    def summary_line(self) -> str:
        """The deterministic one-liner for the rail and the card."""
        counts = self.counts()
        parts = [self.episode_id] if self.episode_id else []
        parts.append("%d file%s" % (counts["files"], "" if counts["files"] == 1 else "s"))
        parts.append("%d semantic change%s"
                     % (counts["semantic_lines"], "" if counts["semantic_lines"] == 1 else "s"))
        if counts["folded_files"]:
            parts.append("%d folded (%s)"
                         % (counts["folded_files"], self._fold_phrase(counts)))
        if self.collapse:
            parts.append("collapsed")
        return " · ".join(parts)

    def detail(self) -> list[str]:
        """The expanded block: one row per file, folded or not, each with its reason."""
        out = [self.headline()]
        for item in self.files:
            out.append("  %s%s" % (item.path,
                                   "  [%s]" % item.rule if item.folded else ""))
            for line in item.detail:
                out.append("    %s" % line)
            if item.suppression:
                out.append("    %s" % item.suppression.render())
        if self.collapse:
            out.append("  %s" % self.collapse.render())
        return out

    def as_dict(self) -> dict:
        return {"episode_id": self.episode_id, "title": self.title,
                "counts": self.counts(), "noise_only": self.noise_only,
                "collapsed": self.collapse is not None,
                "headline": self.headline(), "summary": self.summary_line(),
                "collapse": self.collapse.as_dict() if self.collapse else None,
                "collapse_reason": self.collapse_reason,
                "policy": self.policy.as_dict(),
                "files": [item.as_dict() for item in self.files]}

    # -- internals ----------------------------------------------------------
    def _fold_phrase(self, counts: dict) -> str:
        by_rule = counts["by_rule"]
        bits = ["%s ×%d" % (RULE_INDEX[rule].phrase, count) for rule, count in by_rule.items()]
        if counts["by_formatter"]:
            bits.append("formatter%s: %s"
                        % ("" if len(counts["by_formatter"]) == 1 else "s",
                           ", ".join("%s ×%d" % (name, count)
                                     for name, count in counts["by_formatter"].items())))
        return "; ".join(bits)


# ---------------------------------------------------------------------------
# the run
# ---------------------------------------------------------------------------
def suppress(episode: Episode, policy: Policy | None = None, *,
             classifier: ChangeClassifier | None = None,
             history: dict[str, int] | None = None,
             history_window: int = 8) -> EpisodeNoise:
    """Fold one episode's noise. The ticket, in one call."""
    active = policy or Policy.built_in()
    clf = classifier or ChangeClassifier.default()
    history = dict(history or {})
    paths = episode.paths
    view = EpisodeNoise(episode_id=episode.id, title=episode.title, policy=active,
                        history=history)
    for change in sorted(episode.files, key=lambda item: item.subject):
        view.files.append(_inspect_file(change, active, clf, paths, history, history_window))
    view.collapse, view.collapse_reason = _collapse(view, active)
    return view


def _inspect_file(change: Change, policy: Policy, clf: ChangeClassifier,
                  paths: list[str], history: dict[str, int],
                  window: int) -> FileNoise:
    path = change.subject
    classification = clf.classify(ChangeBlob(path=change.path or path, content=change.content,
                                             change_type=change.change_type,
                                             old_path=change.old_path))
    item = FileNoise(path=path, cls=classification.cls, noise=classification.noise,
                     band=classification.band, confidence=classification.confidence,
                     change_type=change.change_type)
    item.reformat = R.compare(path, change.before, change.after,
                              change_type=change.change_type)

    generator_hint = _generator_hint(change.content)
    if change.change_type not in ("added", "deleted"):
        item.generated = G.inspect(path, change.before, change.after,
                                   path_class=classification.cls, band=classification.band,
                                   generator_hint=generator_hint,
                                   change_type=change.change_type,
                                   history=history.get(path, 0), history_window=window,
                                   inputs_moved=_inputs_moved(paths, path, classification.cls))
    if L.format_for(path) is not None or classification.cls == C.PathClass.LOCKFILE:
        item.lock = L.summarize(path, change.before, change.after,
                                added_lines=change.added_lines,
                                deleted_lines=change.deleted_lines,
                                changed_paths=paths)

    candidates: dict[str, Suppression] = {}
    if item.reformat and item.reformat.is_reformat:
        candidates[REFORMAT] = _suppression(REFORMAT, path, item.reformat.reason,
                                            item.reformat.evidence(),
                                            policy, lines=item.reformat.changed_lines,
                                            detail=item.reformat.formatter)
        item.fired.append(REFORMAT)
    if item.generated and item.generated.is_churn:
        candidates[GENERATED_CHURN] = _suppression(
            GENERATED_CHURN, path, item.generated.reason, item.generated.evidence,
            policy, lines=item.generated.changed_lines, detail=item.generated.kind)
        item.fired.append(GENERATED_CHURN)
    if item.lock is not None and classification.cls == C.PathClass.LOCKFILE:
        if item.lock.manifest_changed:
            item.refused.append("%s: %s" % (LOCKFILE, item.lock.reason()))
        elif item.lock.unreadable:
            # A lockfile this tool cannot parse cannot be *counted*, and "N dependencies
            # upgraded" is the claim the fold line makes. Refusing is the honest answer: the
            # diff stays on screen, with the reason it stayed.
            item.refused.append("%s: the fold line would have to count the entries and this "
                                "lockfile has no entry structure this tool can read (%s)"
                                % (LOCKFILE, item.lock.note or "unparsed"))
        else:
            candidates[LOCKFILE] = _suppression(LOCKFILE, path, item.lock.reason(),
                                                [item.lock.headline()] +
                                                (["declared by %s" % item.lock.note]
                                                 if item.lock.note else []),
                                                policy, lines=item.lock.added_lines +
                                                item.lock.deleted_lines,
                                                detail=item.lock.headline())
            item.fired.append(LOCKFILE)
    if classification.cls == C.PathClass.VENDORED:
        candidates[VENDORED] = _suppression(
            VENDORED, path, "vendored source: someone else's code, committed to the tree",
            ["path rule %s claims `vendored`" % (classification.path_rule or "vendor/**")],
            policy)
        item.fired.append(VENDORED)
    if classification.noise == "unknown":
        candidates[USER_GLOB] = _suppression(
            USER_GLOB, path, "this repository's own noise list claimed the path",
            ["noise rule %s (%s)" % (classification.noise_rule or "noise.globs",
                                     classification.path_layer or "repo layer")],
            policy)
        item.fired.append(USER_GLOB)

    for rule_id in PRIMARY_ORDER:
        if rule_id not in candidates:
            continue
        allowed, why = policy.fold_allowed(rule_id, path)
        if allowed:
            item.suppression = candidates[rule_id]
            break
        item.refused.append("%s fired but is not folding: %s" % (rule_id, why))
        item.suppression = None
    if item.generated and item.generated.claim == G.HAND_EDIT:
        item.notes.append(item.generated.reason)
    return item


def _collapse(view: EpisodeNoise, policy: Policy) -> tuple[Suppression | None, str]:
    """The episode-level fold, or the reason the episode stays expanded."""
    enabled, source = policy.effective(NOISE_ONLY)
    if not view.files:
        return None, "the episode changes no files"
    if not view.noise_only:
        visible = [item.path for item in view.files if not item.folded]
        return None, ("%d file(s) carry change: %s"
                      % (len(visible), ", ".join(visible[:3])))
    if not enabled:
        return None, "%s is off (%s)" % ("noise.collapse.noise_only", source)
    counts = view.counts()
    rules = view.rules_used
    keep = policy.keeps
    if counts["visible_files"]:
        return None, "policy keeps at least one file visible"
    kind = rules[0] if len(rules) == 1 else "mixed"
    if kind == LOCKFILE:
        reason = "lockfile-only: %s" % _lock_phrase(view)
    elif kind == REFORMAT:
        reason = ("0 semantic changes: %s"
                  % _reformat_phrase(view, counts))
    elif kind == GENERATED_CHURN:
        reason = "generated-file churn only: %d generated file(s) regenerated" \
                 % counts["folded_files"]
    else:
        reason = "nothing but noise: %s" % view._fold_phrase(counts)
    expanded = [item.path for item in view.files]
    evidence = ["%d file(s) folded: %s" % (counts["folded_files"],
                                           ", ".join("%s (%s)" % (item.path, item.rule)
                                                     for item in view.files[:6]))]
    evidence.append("%d changed line(s) hidden behind one line" % counts["folded_lines"])
    for item in view.files:
        evidence.extend(item.detail[:2])
    for path in sorted(set(expanded)):
        kept = keep(path)
        if kept:
            evidence.append("  · `%s` is protected by noise.keep %s" % (path, kept))
    suppression = Suppression(rule=NOISE_ONLY, path="", reason=reason, evidence=evidence,
                              expand=policy.expand_key, policy_source=source,
                              detail=reason, lines=counts["folded_lines"])
    suppression.policy_source = source
    return suppression, reason


def _lock_phrase(view: EpisodeNoise) -> str:
    locks = [item.lock for item in view.files if item.lock]
    if not locks:
        return "no lockfile to describe"
    head = "; ".join(lock.headline() for lock in locks[:3])
    extra = "" if len(locks) <= 3 else " (and %d more lockfile(s))" % (len(locks) - 3)
    return "%s%s" % (head, extra)


def _reformat_phrase(view: EpisodeNoise, counts: dict) -> str:
    """AC 1's sentence: which formatter, how many files, and what was proven identical."""
    formatters = counts["by_formatter"]
    who = ("rewritten by " + ", ".join("%s ×%d" % (name, count)
                                       for name, count in formatters.items())
           if formatters else "rewritten by each language's formatter")
    return ("%d file(s) %s with no token-level change: the tokens, the comments and every "
            "literal are identical before and after" % (counts["folded_files"], who))


def _suppression(rule: str, path: str, reason: str, evidence: list[str], policy: Policy,
                 lines: int = 0, detail: str = "") -> Suppression:
    _enabled, source = policy.effective(rule)
    payload = [line for line in evidence if line] or ["the detector gave no evidence"]
    return Suppression(rule=rule, path=path, reason=reason, evidence=payload,
                       expand=policy.expand_key, policy_source=source, detail=detail,
                       lines=lines)


def _generator_hint(content: str | None) -> str:
    if not content:
        return ""
    for hit in M.scan(content):
        if hit.cls == C.PathClass.GENERATED:
            return "%s (%s)" % (hit.marker.id, hit.matched.strip()[:40])
    return ""


def _inputs_moved(paths: list[str], path: str, cls: str) -> list[str]:
    """The changed paths that are not themselves noise-class: the possible generator inputs."""
    if cls != C.PathClass.GENERATED:
        return []
    directory = path.rsplit("/", 1)[0] if "/" in path else ""
    return [other for other in paths
            if other != path and (not directory or other.rsplit("/", 1)[0] == directory)]


def session_policy(policy: Policy | None = None, *, pressed: bool = True) -> Policy:
    """Convenience: the policy after the keystroke. Used by the UI seam and the tests."""
    base = policy or Policy.built_in()
    return base.with_session(Session(suppress=not pressed,
                                       pressed=DEFAULT_EXPAND_KEY))
