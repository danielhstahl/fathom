"""The public models of the fixture package — after the episode."""

from dataclasses import dataclass
from ._deprecation import deprecated


@dataclass
class Shape:
    """One figure. `@dataclass` writes `__init__`, so the scope cannot be enumerated."""

    name: str
    colour: str = "white"

    def area(self) -> float:
        return 0.0

    def describe(self, prefix: str) -> str:
        return f"{prefix}{self.name}"


class Circle(Shape):
    radius: float


def distance(a: float, b: float, eps: float = 0.0) -> float:
    return max(abs(a - b), eps)


def label(text: str, *, upper: bool = False) -> str:
    return text.upper() if upper else text


def perimeter(w: float, h: float) -> float:
    """A new name: additive."""
    return 2.0 * (w + h)


@deprecated("use label")
def old_label(text: str) -> str:
    return text


_INTERNAL = 4


def _private() -> None:
    """Not on the surface in either revision."""
    return None
