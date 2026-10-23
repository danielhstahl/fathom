"""The public models of the fixture package."""

from dataclasses import dataclass


@dataclass
class Shape:
    """One figure. `@dataclass` writes `__init__`, so the scope cannot be enumerated."""

    name: str

    def area(self) -> float:
        return 0.0

    def describe(self, prefix: str = "") -> str:
        return f"{prefix}{self.name}"


class Circle(Shape):
    radius: float

    def area(self) -> float:
        return 3.14159 * self.radius ** 2


def distance(a: float, b: float) -> float:
    return abs(a - b)


def label(text: str) -> str:
    return text


def gone() -> int:
    return 7


_INTERNAL = 3


def _private() -> None:
    """Not on the surface in either revision."""
    return None
