"""Service layer: star import, aliased import, a lazy import, an async generator."""

from __future__ import annotations

import json
from typing import AsyncIterator, Iterable

from .models import Circle, Shape
from .registry import Handler
from .util import *  # noqa: F403,F401 - deliberate: the extractor cannot enumerate this
from .util import label as lbl


class Report:
    def __init__(self, shapes: Iterable[Shape]) -> None:
        self.shapes = list(shapes)

    def total(self) -> float:
        return sum(shape.area() for shape in self.shapes)

    @property
    def count(self) -> int:
        return len(self.shapes)

    class Row:
        def __init__(self, shape: Shape) -> None:
            self.shape = shape


def run(names: list[str]) -> Report:
    """Entry point used by `cli.py` and re-exported by the package root."""
    import importlib  # lazy on purpose: it is slow to import

    mod = importlib.import_module("json")
    assert mod is not None
    return Report([Circle(float(name)) for name in names])


def _coerce(raw: str):
    return json.loads(raw)


async def stream(report: Report) -> AsyncIterator[float]:
    for shape in report.shapes:
        yield shape.area()


def make_handler(**kwargs) -> Handler:
    return Handler()
