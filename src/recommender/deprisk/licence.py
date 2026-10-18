"""Licence drift: what this episode made this repository ship, and whether policy allows it.

Two different questions wear the same word, and the module keeps them apart.

**Policy** — *may this repository carry this licence where it now carries it?* Each package's
expression is classified (permissive, weak copyleft, strong copyleft, network copyleft, unknown),
located (which layer asked for it, from `[layers]`, or from the caller), and answered by the matrix
in `policy`. An `OR` is a choice and is judged against the most permissive option, with the
alternatives kept so the row can name them; a `deny` list entry outranks the matrix; and a licence
nobody declared is *unknown*, which is a finding rather than a pass.

**Drift** — *did the answer change?* A package that was MIT last week and is GPL-3.0-only this week
has had its terms changed by somebody who is not the reviewer, and no lockfile-diff view shows it:
the version may not even have moved. That is a `license-drift` row, and it is the one these checks
exist for.

The layer question is why "deny copyleft in the library layer" is not the same as "deny copyleft".
A licence's obligations depend on who has to live with them: a crate under `crates/fathom-core` is
linked by everyone who installs it, while a binary under `crates/fathom-cli` reaches an end user
directly and is a different legal shape. The layer is found by matching the *manifest path that
declared the package* against `[layers]`, and when nothing matches, `deps.license.unknown_layer_policy`
decides whether to assume the stricter rule (default) or the looser one — an unmatched path is
usually an undeclared repository, not a permissive one.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .model import ADDED, REMOVED, Change, Package, edge_path
from .policy import (
    LICENSE_ADDED, LICENSE_DENIED, LICENSE_DRIFT, LICENSE_FLAGGED, LICENSE_UNKNOWN)

#: Why a row exists.
STATE_NEW = "new"                  # this episode introduced the package
STATE_DRIFT = "drift"              # the same package's declared licence moved
STATE_UNCHANGED = "unchanged"      # checked, and the licence is the one it already was
STATE_REMOVED = "removed"          # gone; the licence set shrank, no policy question remains
STATE_DEV_SKIPPED = "dev-skipped"  # `deps.license.ignore_dev` is on and the lockfile says dev


@dataclass(frozen=True)
class Row:
    """One licence check: a package, its verdict, and the path that brought it."""

    package: Package
    verdict: Verdict
    state: str = STATE_NEW
    kind: str = ADDED
    before_licence: str = ""
    #: The manifest path used for the layer match, kept because a reader must be able to re-check it.
    declared_in: str = ""
    evidence: list[str] = field(default_factory=list)

    @property
    def violation(self) -> bool:
        return self.verdict.violation and self.state in (STATE_NEW, STATE_DRIFT, STATE_UNCHANGED)

    @property
    def rule_id(self) -> str:
        return self.verdict.rule_id

    @property
    def edge(self) -> str:
        return edge_path(self.package.asked_by, self.package.text)

    @property
    def anchor(self) -> str:
        return self.package.scope or self.declared_in or "./"

    @property
    def licence_label(self) -> str:
        return self.verdict.label

    def says(self) -> str:
        """One line: what, where, and the rule that answered it."""
        parts = ["%s licence %s" % (self.package.text, self.licence_label)]
        if self.state == STATE_DRIFT:
            parts.append("was %s before this episode" % (self.before_licence or "nothing"))
        parts.append("[%s · %s]" % (self.verdict.cls, self.verdict.mode))
        if self.verdict.layer:
            parts.append("layer %s (%s)" % (self.verdict.layer, self.verdict.basis))
        parts.append("via %s" % self.edge)
        return " · ".join(parts)

    def rows(self, max_evidence: int | None = None) -> list[str]:
        pad = "  "
        lines = [pad + self.says()]
        lines.append(pad + "  - rule: %s%s" % (self.verdict.why,
                                               " (`%s`)" % self.verdict.knob
                                               if self.verdict.knob else ""))
        if self.verdict.expression.rejected:
            lines.append(pad + "  - alternatives not taken: %s"
                         % ", ".join(self.verdict.expression.rejected))
        for item in self.evidence[:max_evidence] if max_evidence else self.evidence:
            lines.append(pad + "  - " + str(item))
        return lines

    def as_dict(self) -> dict:
        return {"package": self.package.text, "name": self.package.name,
                "version": self.package.version, "ecosystem": self.package.ecosystem,
                "kind": self.kind, "state": self.state, "violation": self.violation,
                "rule": self.rule_id, "licence": self.licence_label,
                "declared_licence": self.package.licence, "before_licence": self.before_licence,
                "class": self.verdict.cls, "mode": self.verdict.mode,
                "library": self.verdict.library, "layer": self.verdict.layer,
                "basis": self.verdict.basis, "why": self.verdict.why, "knob": self.verdict.knob,
                "edge": self.edge, "anchor": self.anchor, "dev": self.package.dev,
                "verdict": self.verdict.as_dict(), "evidence": list(self.evidence)}


def evaluate(delta, policy) -> list[Row]:
    """One row per package the delta touched, with a verdict wherever a licence can be asked.

    Order is the delta's order, and the delta's order is `(kind, ecosystem, name)`, so the rows are
    stable without this function having to sort anything — the same rule every golden file here
    depends on.
    """
    rows: list[Row] = []
    for change in delta.changes:
        rows.extend(_of(change, policy))
    return rows


def _of(change: Change, policy) -> list[Row]:
    if change.kind == REMOVED or change.after is None:
        return [Row(package=change.before or change.after, verdict=_gone(policy, change),
                    state=STATE_REMOVED, kind=change.kind,
                    before_licence=(change.before.licence if change.before else ""),
                    declared_in=change.scope,
                    evidence=["removed by this episode: its licence left the shipped set"])]
    package = change.after
    before_licence = change.before.licence if change.before else ""
    layer, basis = policy.layer_of(package.scope, package.layer)
    verdict = policy.verdict(package.licence, layer, basis)
    state = _state_of(change, before_licence, package, policy)
    evidence = ["licence %s as declared in %s" % (package.licence or "nothing",
                                                 package.scope or "no recorded manifest path")]
    if package.source:
        evidence.append("resolved from %s" % package.source)
    if change.proof:
        evidence.append("proof: %s" % change.proof)
    if state == STATE_DEV_SKIPPED:
        verdict = policy.verdict(package.licence, layer, basis)
        evidence.append("`deps.license.ignore_dev` is on and the lockfile marks this package "
                        "development-only, so policy was not applied")
    return [Row(package=package, verdict=verdict, state=state, kind=change.kind,
                before_licence=before_licence, declared_in=package.scope, evidence=evidence)]


def _state_of(change: Change, before_licence: str, package: Package, policy) -> str:
    if policy.flag("lic_ignore_dev") and package.dev:
        return STATE_DEV_SKIPPED
    if change.before is not None and str(package.licence or "") != str(before_licence or ""):
        return STATE_DRIFT
    if change.kind == ADDED or change.before is None:
        return STATE_NEW
    return STATE_UNCHANGED


def _gone(policy, change: Change):
    """A removal is not a policy question; the empty verdict keeps the row shape uniform."""
    return policy.verdict("", "", "unmatched")


def drift_rows(rows: list[Row]) -> list[Row]:
    return [row for row in rows if row.state == STATE_DRIFT]


def violation_rows(rows: list[Row]) -> list[Row]:
    return [row for row in rows if row.violation]


def class_counts(rows: list[Row]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in rows:
        if row.state == STATE_REMOVED:
            continue
        counts[row.verdict.cls] = counts.get(row.verdict.cls, 0) + 1
    return counts


def newly_seen_ids(rows: list[Row], delta, policy) -> list[tuple[str, str]]:
    """`(licence id, package that brought it)` for ids newly in what the repository ships.

    The delta model already computes the shipped licence-set delta (`deps::LicenseDelta`) and hands
    it over, so that is believed when present. What this adds is the case where the caller only
    passed the per-package rows: then an id on an added package that no before-side carried is
    newly seen. Neither path guesses a licence that was never declared.
    """
    out: list[tuple[str, str]] = []
    for item in delta.licences_added:
        out.append((str(item), "reported by the delta model"))
    before_ids: set[str] = set()
    for row in rows:
        for change_id in _ids_of_before(row, policy):
            before_ids.add(change_id)
    for row in rows:
        if row.state != STATE_NEW or (row.package.dev and policy.flag("lic_ignore_dev")):
            continue
        expression = row.verdict.expression
        # Only the option actually taken enters the shipped set: `MIT OR GPL-3.0-only` taken as MIT
        # ships MIT, and the alternative it did not use is not a licence this repository carries.
        chosen = expression.chosen or expression.expression
        for atom in (list(policy.classify(chosen).ids) or list(expression.ids)):
            if atom in before_ids:
                continue
            if any(existing == atom for existing, _who in out):
                continue
            out.append((atom, row.package.text))
    seen: list[tuple[str, str]] = []
    for pair in out:
        if pair not in seen:
            seen.append(pair)
    return seen


def _ids_of_before(row: Row, policy) -> list[str]:
    if not row.before_licence:
        return []
    return list(policy.classify(row.before_licence).ids)


#: The rules this module can raise, for the docs' benefit.
RULES_RAISED: tuple[str, ...] = (LICENSE_DENIED, LICENSE_FLAGGED, LICENSE_UNKNOWN, LICENSE_DRIFT,
                                 LICENSE_ADDED)
