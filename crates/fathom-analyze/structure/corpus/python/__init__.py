"""The demo package root: re-exports, `__all__`, and a conditional import."""

from .models import Shape, Circle as Round
from .service import run

try:  # pragma: no cover - the fast json is optional
    import ujson as json
except ImportError:
    import json

__all__ = ["Shape", "run", "Round"]
