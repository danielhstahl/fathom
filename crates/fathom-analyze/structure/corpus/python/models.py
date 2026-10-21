"""Dataclasses, properties, and the decorators that write code."""

from __future__ import annotations

import collections
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Optional

from .util import DEFAULT_TIMEOUT, label

if TYPE_CHECKING:
    from .service import Report


@dataclass
class Shape:
    name: str
    tags: list[str] = field(default_factory=list)
    timeout: int = DEFAULT_TIMEOUT

    def area(self) -> float:
        raise NotImplementedError

    @property
    def perimeter(self) -> float:
        return 0.0

    @staticmethod
    def origin() -> Shape:
        return Circle(0.0)

    @classmethod
    def named(cls, name: str) -> Shape:
        return cls(name=name)

    def as_row(self) -> tuple[str, float]:
        return (label(self.name), self.area())


@dataclass
class Circle(Shape):
    radius: float = 1.0

    def area(self) -> float:
        return 3.14159 * self.radius**2

    def scaled(self, k: float) -> Circle:
        return Circle(self.radius * k)


class Registry:
    """Not a dataclass: an ordinary class with a nested class."""

    items: dict[str, Shape] = {}

    class Entry:
        def __init__(self, shape: Shape) -> None:
            self.shape = shape

    def __init__(self) -> None:
        self._memo: dict[str, float] = {}

    def add(self, item: Shape) -> None:
        Registry.items[item.name] = item

    def report(self) -> Optional[Report]:
        return None

    def __len__(self) -> int:
        return len(Registry.items)


Record = collections.namedtuple("Record", ["a", "b"])

MAX_SHAPES: int = 10
DEFAULTS = {}
