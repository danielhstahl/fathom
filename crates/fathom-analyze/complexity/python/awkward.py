"""The shapes ADR-007 §4.2 was written for.

Decorated definitions are units (the extractor unwraps `decorated_definition`, so this pass reads
them); a nested `def` is not a unit and its `if` belongs to the function that lexically encloses it;
a `class` body is a container, and control flow in one is no function's business.
"""
from dataclasses import dataclass


@dataclass
class Box:
    """`@dataclass` writes `__init__`: the scope is marked `decorated`, so figures beside it are floors."""

    width: int
    height: int

    @property
    def area(self) -> int:
        return self.width * self.height

    def scale(self, factor):
        if factor <= 0 or self.width == 0:
            return Box(0, 0)
        elif factor > 1 and self.height > 0:
            return Box(self.width * factor, self.height)
        return self


def outer(items):
    def inner(value):
        if value:
            return [v for v in value if v]
        return []

    return [inner(v) for v in items if v is not None]


pick = lambda xs: xs[0] if xs else None


class Registry:
    TABLE = {}

    for name in ("a", "b"):
        TABLE[name] = 1 if name else 0

    def lookup(self, key):
        try:
            return self.TABLE[key]
        except KeyError:
            return None
        except TypeError as exc:
            raise ValueError("unusable key") from exc
        finally:
            self.last = key

    def matched(self, command):
        match command.split():
            case [action]:
                return action
            case [action, value] if value.isdigit():
                return action * int(value)
            case _:
                return None

    def each(self, rows):
        for row in rows:
            with open(row, encoding="utf-8") as handle:
                assert handle is not None
                yield handle


def spooled(handle):
    """`while` is a test like any other (§4.2), and a `while … else` is a loop that ran to exhaustion:
    the `else` is worth nothing, because it is not a test the reviewer has to check (§3.4).

    This exists because C-KIND (ADR-007 §4) requires every declared node kind to be produced by some
    corpus file, and the first pass of this corpus declared `while_statement` and had none.
    """
    while handle.more():
        if handle.bad():
            break
    else:
        handle.close()
    return handle.tail
