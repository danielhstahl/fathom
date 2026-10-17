"""The things a parser cannot see, kept in one file on purpose."""

from typing import NamedTuple, TypedDict

from .models import Shape


class Box(NamedTuple):
    w: int
    h: int


class Movie(TypedDict):
    name: str
    year: int


def __getattr__(name: str):
    """Module-level `__getattr__`: `demo.dynamic.Slate` resolves at runtime."""
    if name == "Legacy":
        return Shape
    raise AttributeError(name)


def _factory(cls):
    cls.__name__ = f"decorated_{cls.__name__}"
    return cls


@_factory
class Slate:
    def make(self):
        return Box(1, 2)


Shape.describe = lambda self: self.name
Slate.extra = property(lambda self: 1)
globals()["Dynamic"] = type("Dynamic", (Shape,), {})

__all__ = ["Box", "Movie", "Slate"]
