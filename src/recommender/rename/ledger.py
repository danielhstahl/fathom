"""Where a reviewer's judgement about a guess is kept, and the rule about when it stops counting.

Accept and reject are the difference between "the tool guessed" and "the reviewer decided". For that to
be more than a keypress, four things have to hold, and each one is a failure mode this module exists to
prevent:

* **a decision outlives the run.** It is written to `.fathom/rename-review.json` (or `--state`), so
  closing the TUI does not delete the reviewer's afternoon.
* **a decision binds to bytes, not to paths.** Each entry stores the pairing's
  :attr:`~recommender.rename.model.Pairing.binding` digest. When the same paths come back with
  different evidence — a squash rebase, an amend, a rebase across a delete, all of which `s4` measured
  changing a pair's evidence from R100 to R012 — the old verdict is **re-opened** rather than
  inherited, and the card says so. A ledger that inherited would be asserting a pairing it never saw.
* **a rejection is never a deletion.** A rejected pairing is not shown as a pairing and is still
  counted on the card ("1 pairing suppressed by your earlier rejection"), because a reviewer who
  forgets they suppressed three pairings has been misled by their own tool. The ADR-001 rule that
  suppression is a view, not a deletion, applies here exactly as it does to noise folding.
* **a decision carries who made it.** `accepted by dan@example.invalid` on the row is what makes it a
  review act and not a change in the tool's mood.

`e4.x`'s durable review ledger will own this storage; until then the file is this package's, and it is
the same shape so the migration is a move and not a redesign.
"""

from __future__ import annotations

import json
import os
import tempfile
from dataclasses import asdict, dataclass, field
from pathlib import Path

from . import model as M

#: Default location, relative to the repository root.
DEFAULT_PATH = Path(".fathom") / "rename-review.json"

SCHEMA = "fathom.rename-review/1"


class LedgerError(Exception):
    """The file is not ours to read: wrong schema, wrong shape, or not a file at all."""


@dataclass(frozen=True)
class Entry:
    """One recorded decision about one pairing."""

    key: str
    binding: str
    decision: str                     # accepted | rejected
    reviewer: str
    window: str = ""
    kind: str = ""
    sources: tuple[str, ...] = ()
    destinations: tuple[str, ...] = ()
    confidence: float = 0.0
    tier: str = ""
    when: str = ""
    note: str = ""

    def as_dict(self) -> dict:
        row = asdict(self)
        row["sources"] = list(self.sources)
        row["destinations"] = list(self.destinations)
        return row


@dataclass
class Ledger:
    """The recorded decisions, addressable by pairing key."""

    path: Path | None = None
    entries: dict[str, Entry] = field(default_factory=dict)
    unreadable: list[str] = field(default_factory=list)

    # -- construction ---------------------------------------------------------
    @classmethod
    def load(cls, path: Path | None, *, reviewer: str = "") -> "Ledger":
        """Read the ledger, tolerating a missing file and quarantining a corrupt one.

        A missing file is the normal case. A file that is not valid JSON, or not this schema, is an
        event: the reviewer's decisions are on the line, so the ledger says it is starting without them
        and leaves the bytes where they are rather than overwriting them on the next save.
        """
        ledger = cls(path=path)
        if path is None or not Path(path).exists():
            return ledger
        try:
            doc = json.loads(Path(path).read_text())
        except (OSError, ValueError) as exc:
            ledger.unreadable.append("%s: unreadable (%s): starting with no recorded decisions and "
                                     "leaving the file alone" % (path, exc))
            return ledger
        if doc.get("schema") != SCHEMA:
            ledger.unreadable.append("%s: schema %r is not %r: ignoring it rather than guessing"
                                     % (path, doc.get("schema"), SCHEMA))
            return ledger
        for raw in doc.get("entries", []):
            try:
                entry = Entry(key=raw["key"], binding=raw["binding"], decision=raw["decision"],
                              reviewer=raw.get("reviewer", reviewer), window=raw.get("window", ""),
                              kind=raw.get("kind", ""), sources=tuple(raw.get("sources", ())),
                              destinations=tuple(raw.get("destinations", ())),
                              confidence=float(raw.get("confidence", 0.0)),
                              tier=raw.get("tier", ""), when=raw.get("when", ""),
                              note=raw.get("note", ""))
            except (KeyError, TypeError, ValueError) as exc:
                ledger.unreadable.append("%s: one entry is unreadable (%s) and was skipped"
                                         % (path, exc))
                continue
            ledger.entries[entry.key] = entry
        return ledger

    # -- the questions --------------------------------------------------------
    def __contains__(self, key: str) -> bool:
        return key in self.entries

    def get(self, key: str) -> Entry | None:
        return self.entries.get(key)

    def as_mapping(self) -> dict[str, Entry]:
        """The shape :func:`recommender.rename.pair.pair` takes as its ledger."""
        return dict(self.entries)

    @property
    def accepted(self) -> list[Entry]:
        return [e for e in self.entries.values() if e.decision == M.DISPOSITION_ACCEPTED]

    @property
    def rejected(self) -> list[Entry]:
        return [e for e in self.entries.values() if e.decision == M.DISPOSITION_REJECTED]

    # -- the acts -----------------------------------------------------------
    def record(self, pairing: M.Pairing, decision: str, *, reviewer: str,
               when: str = "", note: str = "") -> Entry:
        """Record a decision about this exact pairing, overwriting an earlier one."""
        if decision not in (M.DISPOSITION_ACCEPTED, M.DISPOSITION_REJECTED):
            raise LedgerError("a decision is accepted or rejected, not %r" % decision)
        if not reviewer:
            raise LedgerError("a decision must name who made it — that is the point of recording it")
        entry = Entry(key=pairing.key(), binding=pairing.binding, decision=decision,
                      reviewer=reviewer, window=pairing.window, kind=pairing.kind,
                      sources=tuple(pairing.sources), destinations=tuple(pairing.destinations),
                      confidence=round(pairing.confidence, 4), tier=pairing.tier,
                      when=when, note=note)
        self.entries[entry.key] = entry
        pairing.disposition = decision
        pairing.reviewer = reviewer
        return entry

    def forget(self, key: str) -> bool:
        return self.entries.pop(key, None) is not None

    # -- persistence --------------------------------------------------------
    def as_document(self) -> dict:
        return {"schema": SCHEMA,
                "entries": [self.entries[key].as_dict() for key in sorted(self.entries)]}

    def save(self, path: Path | None = None) -> Path:
        """Write the ledger atomically, sorted, so two runs produce the same bytes.

        Sorted by key and no clock in the payload (the `when` field is whatever the caller supplied):
        a review file that churned on every save would be a diff-noise machine, and a golden test
        could not read it.
        """
        target = Path(path if path is not None else self.path)
        if self.unreadable:
            raise LedgerError("refusing to overwrite a ledger this program could not read: %s"
                              % "; ".join(self.unreadable))
        target.parent.mkdir(parents=True, exist_ok=True)
        payload = json.dumps(self.as_document(), indent=1, sort_keys=True) + "\n"
        handle = tempfile.NamedTemporaryFile("w", dir=target.parent, prefix=".rename-review.",
                                             delete=False)
        try:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        finally:
            handle.close()
        os.replace(handle.name, target)
        self.path = target
        return target

    # -- display ----------------------------------------------------------
    def describe(self) -> list[str]:
        out = ["rename review ledger%s" % (" (%s)" % self.path if self.path else " (unsaved)")]
        if self.unreadable:
            out.extend("  ! %s" % line for line in self.unreadable)
        if not self.entries:
            out.append("  no decisions recorded: every guess is still just a guess")
            return out
        for key in sorted(self.entries):
            entry = self.entries[key]
            out.append("  %-9s %-7s %s -> %s  [%s, %s, bound %s]%s"
                       % (entry.decision, entry.kind, ", ".join(entry.sources),
                          ", ".join(entry.destinations), entry.tier or "tier?",
                          entry.reviewer or "reviewer?", entry.binding[:8],
                          " — " + entry.note if entry.note else ""))
        out.append("  %d accepted, %d rejected" % (len(self.accepted), len(self.rejected)))
        return out

    def audit_against(self, result) -> list[str]:
        """Entries that match nothing in the window(s) just paired — reported, never auto-deleted."""
        return ["%s (%s -> %s): no pairing in this window matches this decision any more"
                % (entry.decision, ", ".join(entry.sources), ", ".join(entry.destinations))
                for key, entry in sorted(self.entries.items())
                if key not in {p.key() for p in result.pairings}]
