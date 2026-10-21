"""Small helpers: module-level data, a private name, a conditional definition."""

from __future__ import annotations

import math
from typing import TYPE_CHECKING, Any, Callable

DEFAULT_TIMEOUT = 30
UNITS = ("m", "s")
_REGISTRY: dict[str, Callable[..., Any]] = {}


def label(value: object) -> str:
    return str(value)


def public_pair(a, b):
    """Two-tuple, built by a closure that is not a symbol of this file."""

    def inner(x):
        return x

    return inner(a), inner(b)


def _private() -> None:
    return None


def register(fn=None, *, name=None):
    """Decorator factory used by `registry.py`."""

    def deco(func):
        _REGISTRY[name or func.__name__] = func
        return func

    if fn is None:
        return deco
    return deco(fn)


def units_to_base(value: float, unit: str = "km") -> float:
    return value * _UNIT_FACTORS[unit]


def hypotenuse(a: float, b: float) -> float:
    return math.hypot(a, b)


_UNIT_FACTORS = {"km": 1000.0, "m": 1.0}

if TYPE_CHECKING:

    from .models import Shape

    def stub(shape: Shape) -> None:
        ...
