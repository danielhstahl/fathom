"""Every number is a button: the link model, the view registry, and the resolver a test can walk.

The ticket's last sentence is the one that turns a summary into a contract:

> Every number is a button into its evidence.

The glossary (thesis §4.4) spells the same rule as "every finding, card figure and narrative
sentence cites evidence ids that resolve to a view". Two things have to be true for that to be
falsifiable rather than decorative, and both live here:

1. **A figure with no link does not exist.** `Figure.__post_init__` raises `UnlinkedFigure`
   (declared in `policy.py`), so a section cannot be assembled out of unclickable numbers.
2. **A link that resolves to nothing is a failure, not an empty pane.** Every link names a view
   in `VIEWS`, and every view has a verb that opens it and a statement of whether that verb
   exists *now*, exists in the Rust product, or belongs to a ticket that has not landed.
   `resolve()` answers with the evidence rows or with a reason, and `walk()` runs that over a
   whole card so a contract test can fail on the one figure whose evidence went missing.

The honest part is the status column. Three of these views are shipped Python verbs today
(`fathom structure`, `fathom testsurface`, `fathom noncode`, `fathom noise`, `fathom change`),
the diff pane is the TUI's (`reco-0q7`), and two are named by tickets that have not landed —
`e2.6`'s dependency view is Rust-side and the graph views are `e3.2`'s. A card that printed a
plausible `fathom deps …` for a verb that does not exist would be a button wired to nothing,
which is the failure this whole module exists to prevent; so the registry says
`planned (e2.11)` out loud and the card prints the ticket number beside those links.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .policy import UnresolvableLink

#: View ids, so nobody hand-types a string twice.
VIEW_DIFF = "diff"
VIEW_NOISE = "noise"
VIEW_CLASSES = "classes"
VIEW_STRUCTURE = "structure"
VIEW_DEPS = "deps"
VIEW_SURFACE = "surface"
VIEW_CHECKLIST = "checklist"
VIEW_TESTS = "tests"
VIEW_BLAST = "blast"
VIEW_FINDING = "finding"
VIEW_GRAPH = "graph"
VIEW_POLICY = "policy"

#: What a link's verb is, as a claim about the world.
STATUS_SHIPPED = "shipped"           # this verb runs today, in this repository
STATUS_RUST = "in the Rust product"  # compiled verb, not this Python prototype
STATUS_PLANNED = "planned"           # owned by a ticket that has not landed yet


@dataclass(frozen=True)
class View:
    """One evidence view: what it shows, what opens it, and whether that thing exists."""

    id: str
    title: str
    verb: str                       # command prefix, e.g. "fathom structure dirs"
    status: str
    shows: str
    doc: str = ""
    owner: str = ""                 # ticket / crate that owns the view

    @property
    def available(self) -> bool:
        return self.status == STATUS_SHIPPED

    @property
    def suffix(self) -> str:
        """What the card appends when the verb is not something to run today."""
        if self.status == STATUS_SHIPPED:
            return ""
        return " (%s, %s)" % (self.status, self.owner) if self.owner \
            else " (%s)" % self.status


VIEWS: tuple[View, ...] = (
    View(VIEW_DIFF, "the diff for one file", "fathom show --patch", STATUS_RUST,
         "the file's hunks, with taxonomy class and per-file counts",
         "The pane `reco-0q7` owns. Every line count on this card is a claim about these hunks.",
         owner="reco-0q7 / e1.7"),
    View(VIEW_NOISE, "what was folded and why", "python3 -m recommender.noise episode",
         STATUS_SHIPPED,
         "per-file folds, each with its rule, its formatter and the key that expands it",
         "The `e2.2` view: a fold is a view, never a deletion, so the raw content is one keypress "
         "away in this pane.", owner="reco-zqf / e2.2"),
    View(VIEW_CLASSES, "what each path is", "python3 -m recommender.change episode",
         STATUS_SHIPPED, "path-class mix, per-path confidence and the rule that classified it",
         "The `e2.1` receipts behind the class mix on the card.", owner="reco-dsh / e2.1"),
    View(VIEW_STRUCTURE, "the directory table", "python3 -m recommender.structure dirs",
         STATUS_SHIPPED, "per-directory rows, the structural stories, the churn components",
         "The `e2.3` view, including its reconciliation line.", owner="reco-5fn / e2.3"),
    View(VIEW_DEPS, "the dependency delta", "fathom analyze --deps", STATUS_PLANNED,
         "the per-episode dependency table, each row with the hunk that proves it",
         "`e2.6`'s table, exposed by `e2.11`'s insight API. Not a Python verb, and the card says "
         "so rather than inventing one.", owner="e2.11"),
    View(VIEW_SURFACE, "the non-code findings", "python3 -m recommender.noncode findings",
         STATUS_SHIPPED, "env, flag, migration, CI, IaC and container findings with their anchors",
         "The `e2.8` view.", owner="reco-bjd / e2.8"),
    View(VIEW_CHECKLIST, "the operator-action checklist",
         "python3 -m recommender.noncode checklist", STATUS_SHIPPED,
         "who has to do what, outside this review, in which order",
         "The second artefact `e2.8` produces, and the one an operator reads.",
         owner="reco-bjd / e2.8"),
    View(VIEW_TESTS, "the test surface delta", "python3 -m recommender.testsurface card",
         STATUS_SHIPPED, "line attribution, named cases, coverage findings with `path:line`",
         "The `e2.9` view.", owner="reco-6pl / e2.9"),
    View(VIEW_BLAST, "the blast-radius rows", "python3 -m recommender.card blast",
         STATUS_SHIPPED, "every counted module, the rule or edge that counted it, and the partials",
         "This package's own drill-down: the count on the card and the walk behind it.",
         owner="reco-4ey / e2.10"),
    View(VIEW_FINDING, "one finding, with its evidence", "python3 -m recommender.card finding",
         STATUS_SHIPPED, "the finding's own words, its anchor and its evidence rows",
         "The card's roll-up view of a finding raised by any analysis; the underlying pane is "
         "the analysis that raised it.", owner="reco-4ey / e2.10"),
    View(VIEW_GRAPH, "the module graph", "fathom graph show", STATUS_PLANNED,
         "the node, its edges, and what depends on it at the configured hop limit",
         "`e3.2`'s view. Until it lands, a graph link only exists when the caller lent a graph — "
         "and then it resolves against that graph.", owner="e3.2"),
    View(VIEW_POLICY, "the policy in force", "python3 -m recommender.config explain --key",
         STATUS_SHIPPED, "every key the card obeyed, the layer that won, and the file it came from",
         "Where a reviewer who disagrees with a threshold goes: `e0.8`'s own explanation of the "
         "winning layer, keyed by the key the figure was measured under.",
         owner="reco-dq3 / e0.8"),
)

VIEW_INDEX: dict[str, View] = {view.id: view for view in VIEWS}
VIEW_IDS: tuple[str, ...] = tuple(view.id for view in VIEWS)

#: Views a link may name. Kept separate from `VIEWS` because a view can be documented without
#: being linkable (and a linkable view had better be documented).
LINKABLE_VIEWS: frozenset[str] = frozenset(VIEW_INDEX)


@dataclass(frozen=True)
class Link:
    """One button: a view, the thing inside it, and the command that opens it."""

    view: str
    target: str
    verb: str = ""
    label: str = ""

    def __post_init__(self) -> None:
        if self.view not in LINKABLE_VIEWS:
            raise UnresolvableLink("no evidence view %r is registered; the card may not link to "
                                   "a view it cannot name" % self.view)
        if not self.target:
            raise UnresolvableLink("link to %s names nothing to open" % self.view)
        if not self.verb:
            self.__dict__["verb"] = VIEW_INDEX[self.view].verb

    @property
    def spec(self) -> View:
        return VIEW_INDEX[self.view]

    def key(self) -> str:
        return "%s:%s" % (self.view, self.target)

    def available(self) -> bool:
        return self.spec.available

    def render(self) -> str:
        """The button, as printed. Deterministic, and never clipped by the renderer."""
        return "%s %s%s" % (self.verb, self.target, self.spec.suffix)

    def as_dict(self) -> dict:
        out = {"view": self.view, "target": self.target, "verb": self.verb,
               "status": self.spec.status, "available": self.available()}
        if self.label:
            out["label"] = self.label
        if self.spec.owner:
            out["owner"] = self.spec.owner
        return out


def link(view: str, target: str, **kwargs) -> Link:
    return Link(view=view, target=target, **kwargs)


@dataclass(frozen=True)
class Resolution:
    """What a button actually opens: rows, or the reason it could not."""

    link: Link
    lines: tuple[str, ...] = ()
    reason: str = ""

    @property
    def found(self) -> bool:
        return bool(self.lines)

    @property
    def ok(self) -> bool:
        """A link is satisfied when it resolves, or when it is honestly marked not-here-yet
        **and** still carries rows. A planned verb with no evidence is still a broken button."""
        return self.found

    def as_dict(self) -> dict:
        return {"link": self.link.as_dict(), "found": self.found, "lines": list(self.lines),
                "reason": self.reason}


@dataclass
class EvidenceMap:
    """The evidence behind a card: link key -> rows, built as the figures are built.

    Attaching twice is fine and appends (two figures can point at the same file), attaching an
    empty row list is not: that is the moment a figure would have pointed at nothing, so it is
    an error at composition time instead of an empty pane at read time.
    """

    rows: dict[str, list[str]] = field(default_factory=dict)
    links: dict[str, Link] = field(default_factory=dict)

    def attach(self, link: Link, lines) -> None:
        if not link or not lines:
            raise UnresolvableLink(
                "no evidence behind %s — refusing to print a number whose button opens nothing"
                % link.key())
        bucket = self.rows.setdefault(link.key(), [])
        self.links[link.key()] = link
        for line in lines:
            text = str(line)
            if text and text not in bucket:
                bucket.append(text)

    def resolve(self, link: Link) -> Resolution:
        lines = self.rows.get(link.key())
        if lines is None:
            return Resolution(link=link, reason="no evidence was attached to %s" % link.key())
        if not lines:
            return Resolution(link=link, reason="the evidence view for %s is empty" % link.key())
        return Resolution(link=link, lines=tuple(lines))

    def resolve_all(self, links) -> list[Resolution]:
        return [self.resolve(item) for item in links]

    def keys(self) -> list[str]:
        return sorted(self.rows)

    def __contains__(self, key: object) -> bool:
        return str(key) in self.rows

    def as_dict(self) -> dict:
        return {key: {"view": self.links[key].view, "target": self.links[key].target,
                      "verb": self.links[key].verb,
                      "status": self.links[key].spec.status,
                      "available": self.links[key].available(),
                      "lines": list(lines)}
                for key, lines in sorted(self.rows.items())}


def walk(card) -> list[Resolution]:
    """Every link on a card, resolved, in card order. This is AC 2 as a loop."""
    out: list[Resolution] = []
    for section in card.sections:
        for figure in section.figures:
            for item in figure.links:
                out.append(card.evidence.resolve(item))
    return out


def broken_links(card) -> list[Resolution]:
    return [item for item in walk(card) if not item.ok]


def describe_views() -> list[str]:
    return ["%-11s %-30s %-38s %s" % (view.id, view.title, view.verb + view.suffix, view.shows)
            for view in VIEWS]
