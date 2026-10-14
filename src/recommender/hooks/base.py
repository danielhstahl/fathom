"""Adapter contract (ADR-005 §2). One module per harness; `provenance` is auditable."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Iterable, List

from ..harness import harness


@dataclass
class Adapter:
    """A vendor adapter: `decode` turns a vendor record into the wire shape, and the
    shared ingest layer does everything else. Keeping ingest out of the adapters is
    what makes the [observed]/[external] distinction meaningful — a mapping table can
    be reviewed, an ingest path cannot."""

    name: str
    wire_protocol: str
    provenance: str            # "observed" | "external" — see ADR-005 §1.2
    event_keys: List[str]
    installed_version: str = "not-installed"

    def describe(self) -> Dict[str, Any]:
        return {"name": self.name, "wire_protocol": self.wire_protocol,
                "provenance": self.provenance, "installed_version": self.installed_version,
                "event_keys": sorted(set(self.event_keys))}

    def decode(self, raw: Dict[str, Any]) -> Dict[str, Any]:
        raise NotImplementedError

    def ingest(self, events_path, records: Iterable[Dict[str, Any]],
               stats: "harness.IngestStats | None" = None) -> int:
        stats = stats or harness.IngestStats()
        n = 0
        for raw in records:
            try:
                wire = self.decode(raw)
            except Exception:
                stats.reject("decode_error")
                continue
            if harness.record(events_path, wire, stats):
                n += 1
        return n

    def round_trip(self, fixtures: Iterable[Dict[str, Any]]) -> List[Dict[str, Any]]:
        """Decode fixtures and project them. This proves the adapter decodes **our
        fixtures**; it does not prove the vendor emits them (ADR-005 §9)."""
        return [self.decode(f) for f in fixtures]
