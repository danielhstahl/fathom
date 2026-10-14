"""Per-episode aggregation: the answer for a whole change set, deterministically.

An episode is the reviewable unit (thesis glossary), and a reviewer does not read twelve
per-file sentences first — they read one line that says *what kind of episode this is*:
"12 source, 4 test, 1 migration, 2 lockfile · 0.88 mean confidence · 3 paths the table has
no rule for". That line drives `e2.3`'s summary card, `e2.9`'s test-surface ratio and
`e2.2`'s "this episode is nothing but noise" collapse, so it is computed here once, from the
same decisions the file list shows, rather than being re-derived three times downstream.

Everything in :class:`EpisodeClassification` is a pure function of the decisions it lists,
sorted by path, with no clock and no dict-order dependence: **the same episode yields the
same bytes**, which ADR-001 §7.4 requires of anything an export or a golden test will ever
carry.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from recommender.taxonomy import classes as C
from recommender.taxonomy.noise import NOISE_SCORE

from .engine import Change, ChangeClassifier, Classification, default_classifier
from .policy import BAND_AMBIGUOUS, BAND_CONFIRMED, BAND_LIKELY, BAND_WEAK


@dataclass(frozen=True)
class Episode:
    """A change set: one episode's paths with their content, keyed in review order."""

    id: str
    changes: tuple[Change, ...] = ()
    title: str = ""

    @classmethod
    def from_files(cls, episode_id: str, files: dict[str, str | bytes], **kwargs) -> "Episode":
        return cls(id=episode_id,
                   changes=tuple(Change(path=path, content=content, **kwargs)
                                 for path, content in sorted(files.items())))

    @property
    def paths(self) -> list[str]:
        return [change.subject for change in self.changes]


@dataclass
class EpisodeClassification:
    """The episode-level view of one classification run."""

    episode_id: str
    decisions: list[Classification] = field(default_factory=list)
    title: str = ""

    # -- the counts --------------------------------------------------------
    def class_counts(self) -> dict[str, int]:
        """Per-class counts in the table's display order, ties broken by count."""
        counts: dict[str, int] = {}
        for decision in self.decisions:
            counts[decision.cls] = counts.get(decision.cls, 0) + 1
        return dict(sorted(counts.items(), key=lambda kv: (-kv[1], C.PATH_CLASSES.index(kv[0]))))

    def noise_counts(self) -> dict[str, int]:
        counts: dict[str, int] = {}
        for decision in self.decisions:
            counts[decision.noise] = counts.get(decision.noise, 0) + 1
        return dict(sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])))

    def paths_by_class(self) -> dict[str, list[str]]:
        out: dict[str, list[str]] = {}
        for decision in self.decisions:
            out.setdefault(decision.cls, []).append(decision.path)
        return {cls: sorted(paths) for cls, paths in sorted(out.items())}

    def noise_share(self) -> float:
        """Line-free noise share: the mean of the per-path noise weights.

        `e0.9`'s `noise_share(paths, weights)` is line-weighted and needs diff stats this
        ticket does not own; the unweighted mean is what the classifier can honestly compute,
        and `e2.2` re-weights it with real line counts.
        """
        if not self.decisions:
            return 0.0
        return sum(NOISE_SCORE[d.noise] for d in self.decisions) / len(self.decisions)

    def test_surface_ratio(self) -> float:
        """Test lines over test + production *files* — a stand-in until `e2.9` does lines."""
        tests = sum(1 for d in self.decisions if d.cls == C.PathClass.TEST)
        source = sum(1 for d in self.decisions if d.cls == C.PathClass.SOURCE)
        return (tests / (tests + source)) if (tests + source) else 0.0

    # -- the honesty signals ----------------------------------------------
    def mean_confidence(self) -> float:
        return (sum(d.confidence for d in self.decisions) / len(self.decisions)) \
            if self.decisions else 0.0

    def band_counts(self) -> dict[str, int]:
        counts = {BAND_CONFIRMED: 0, BAND_LIKELY: 0, BAND_WEAK: 0, BAND_AMBIGUOUS: 0}
        for decision in self.decisions:
            counts[decision.band] = counts.get(decision.band, 0) + 1
        return {k: v for k, v in counts.items() if v}

    def unclassified(self) -> list[Classification]:
        """The `other` answers — a missing rule in the table, printed rather than hidden."""
        return [d for d in self.decisions if d.cls == C.PathClass.OTHER]

    def low_confidence(self, threshold: float = 0.70) -> list[Classification]:
        return sorted((d for d in self.decisions if d.confidence < threshold),
                      key=lambda d: (d.confidence, d.path))

    def overridden(self) -> list[Classification]:
        """Paths where the content contradicted the path rule. Rare, and worth a look."""
        return [d for d in self.decisions if d.overrode_path]

    def is_noise_only(self) -> bool:
        """Whether every path in the episode is noise — `e2.2`'s collapse precondition."""
        return bool(self.decisions) and all(d.is_noise for d in self.decisions)

    # -- output ------------------------------------------------------------
    def summary_line(self) -> str:
        """The deterministic one-liner. Same episode in, same bytes out, always."""
        counts = self.class_counts()
        body = ", ".join("%d %s" % (count, cls) for cls, count in counts.items()) or "empty"
        total = "%d path%s" % (len(self.decisions), "" if len(self.decisions) == 1 else "s")
        parts = ["%s · %s · %s" % (self.episode_id, total, body) if self.episode_id
                 else "%s · %s" % (total, body),
                 "mean confidence %.2f" % self.mean_confidence(),
                 "noise share %.2f" % self.noise_share()]
        unclassified = len(self.unclassified())
        if unclassified:
            parts.append("%d path%s with no rule" % (unclassified, "" if unclassified == 1 else "s"))
        return " · ".join(parts)

    def as_dict(self) -> dict:
        return {"episode_id": self.episode_id, "title": self.title,
                "files": len(self.decisions), "classes": self.class_counts(),
                "noise": self.noise_counts(), "noise_share": round(self.noise_share(), 4),
                "test_surface_ratio": round(self.test_surface_ratio(), 4),
                "mean_confidence": round(self.mean_confidence(), 3),
                "bands": self.band_counts(), "noise_only": self.is_noise_only(),
                "unclassified": sorted(d.path for d in self.unclassified()),
                "overridden": sorted(d.path for d in self.overridden()),
                "decisions": [d.as_dict() for d in self.decisions]}


def classify_episode(episode: Episode, classifier: ChangeClassifier | None = None
                     ) -> EpisodeClassification:
    """Classify every changed path in one episode. The ticket's title, in one call."""
    clf = classifier or default_classifier()
    decisions = [clf.classify(change) for change in episode.changes]
    decisions.sort(key=lambda d: d.path)
    return EpisodeClassification(episode_id=episode.id, decisions=decisions,
                                  title=episode.title)
