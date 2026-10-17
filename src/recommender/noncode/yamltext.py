"""A tolerant, line-oriented YAML reader — the shared half of the CI, IaC and env detectors.

There is no YAML parser in the standard library, and this package is analysis code that has
to degrade honestly rather than throw: a workflow with an anchor, a merge key or a block
scalar is not an error, it is a file whose shape this reader can partly follow. So the
contract is deliberately small and it is stated in the output rather than assumed:

* :func:`scan` returns one :class:`Node` per meaningful line, with the **key path** that
  line sits at (`jobs.deploy.steps.1.run`) computed from indentation;
* anything the scanner cannot model — `&anchor`, `<<:`, `|`/`>` block scalars, a flow
  mapping spanning lines — is recorded on the node and in :attr:`Scan.notes`, and the
  caller turns that into "parsed line-wise, so nested anchors are not resolved";
* a node's `lineno` is real, because every finding this package produces has to point at a
  line, and a parser that loses line numbers cannot produce an anchored finding.

What it is good at is exactly what the three detectors need: "is there a `runs-on:` under
this job", "which `secrets.X` appear", "which `env:` items does this service declare",
"does some step set `permissions`". What it is not is a YAML implementation, and the scan
notes keep that honest in the output rather than only in this paragraph.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

#: Constructs that mean "the shape of this file is more than indentation can describe".
COMPLEX = re.compile(r"(&\w+|<<:|:\s*[|>][-+]?\s*$|\bvendors\b)")
KEY = re.compile(r"^(?P<indent>\s*)(?P<key>[A-Za-z_][\w.\- ]*?):(?P<value>\s.*)?$")
ITEM = re.compile(r"^(?P<indent>\s*)-\s*(?P<rest>.*)$")
FLOW = re.compile(r"^\[(?P<body>.*)\]$")

#: Maximum nesting depth tracked, so a pathological indent cannot grow the stack forever.
MAX_DEPTH = 24


@dataclass(frozen=True)
class Node:
    """One line of a YAML-ish file, placed in the tree by indentation."""

    lineno: int
    indent: int
    path: tuple[str, ...] = ()            #: the containing key path, keys as strings
    key: str = ""                          # "" for a bare sequence item
    value: str = ""                        # scalars, flow collections, `main`
    item: str = ""                         # a `- x` scalar item
    raw: str = ""
    complex: bool = False

    @property
    def location(self) -> tuple[str, ...]:
        """Where this node *is*: its block path plus its own key (items inherit the parent)."""
        return self.path + ((self.key,) if self.key else ())

    @property
    def path_string(self) -> str:
        return ".".join(self.location)

    def under(self, *tails: str) -> bool:
        """Is this node inside, or is it, one of these key paths (`env`, `jobs.deploy.env`)?"""
        for tail in tails:
            parts = [bit for bit in tail.split(".") if bit]
            if not parts:
                continue
            if list(self.path[-len(parts):]) == parts:
                return True
            if list(self.location[-len(parts):]) == parts:
                return True
        return False

    def list_value(self) -> list[str]:
        """The node's sequence value: `- a` items plus a `[a, b]` flow list, unified."""
        if self.item:
            return [self.item]
        flow = FLOW.match(self.value)
        if flow:
            return [bit.strip().strip("'\"") for bit in flow.group("body").split(",")
                    if bit.strip()]
        return [self.value.strip().strip("'\"")] if self.value else []


@dataclass
class Scan:
    """A file's nodes, plus the notes that say how much of it was understood."""

    nodes: list[Node] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    @property
    def partial(self) -> bool:
        return bool(self.notes)

    def where(self, *keys: str) -> list[Node]:
        return [node for node in self.nodes if node.key in keys]

    def under(self, *tails: str) -> list[Node]:
        return [node for node in self.nodes if node.under(*tails)]

    def value_at(self, path: str) -> str:
        for node in self.nodes:
            if node.path_string == path:
                return node.value
        return ""


def scan(text: str) -> Scan:
    """Read a YAML-ish document line by line, tracking key paths by indentation."""
    result = Scan()
    stack: list[tuple[int, str]] = []          # (indent, key) of open blocks
    for lineno, raw in enumerate((text or "").splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        is_complex = bool(COMPLEX.search(raw))
        if is_complex:
            result.notes.append("line %d uses a YAML construct this reader tracks by "
                                "indentation only (%s)" % (lineno, raw.strip()[:60]))
        item = ITEM.match(raw)
        match = KEY.match(raw)
        if match and not item:
            indent = len(match.group("indent"))
            key = match.group("key").strip()
            value = _scalar(match.group("value") or "")
            _pop(stack, indent)
            path = tuple(open_key for _, open_key in stack)
            result.nodes.append(Node(lineno=lineno, indent=indent, path=path, key=key,
                                     value=value, raw=raw, complex=is_complex))
            if len(stack) < MAX_DEPTH:
                stack.append((indent, key))
            continue
        if item:
            indent = len(item.group("indent"))
            _pop(stack, indent)
            rest = item.group("rest") or ""
            inner = KEY.match(rest)
            if inner:
                key = inner.group("key").strip()
                result.nodes.append(Node(lineno=lineno, indent=indent + 2,
                                         path=tuple(open_key for _, open_key in stack),
                                         key=key, value=_scalar(inner.group("value") or ""),
                                         raw=raw, complex=is_complex))
                if len(stack) < MAX_DEPTH:
                    stack.append((indent + 2, key))
            else:
                result.nodes.append(Node(lineno=lineno, indent=indent,
                                         path=tuple(open_key for _, open_key in stack),
                                         item=_scalar(rest), raw=raw, complex=is_complex))
            continue
        result.notes.append("line %d is neither a key nor a list item (%s)"
                            % (lineno, raw.strip()[:60]))
    return result


def values_under(text: str, *tails: str) -> dict[str, list[tuple[int, str]]]:
    """`{key: [(lineno, value)]}` for every node under the given key paths."""
    out: dict[str, list[tuple[int, str]]] = {}
    for node in scan(text).nodes:
        if node.under(*tails):
            for value in node.list_value() or ([node.value] if node.value else []):
                out.setdefault(node.key or node.path_string.rsplit(".", 1)[-1], []).append(
                    (node.lineno, value))
    return out


def flow_list(value: str) -> list[str]:
    """`[main, release]` and `- main` in one spelling."""
    flow = FLOW.match(value.strip())
    if flow:
        return [bit.strip().strip("'\"") for bit in flow.group("body").split(",")
                if bit.strip()]
    return [value.strip().strip("'\"")] if value.strip() else []


def _pop(stack: list[tuple[int, str]], indent: int) -> None:
    while stack and stack[-1][0] >= indent:
        stack.pop()


def _scalar(text: str) -> str:
    text = (text or "").strip()
    if len(text) >= 2 and text[0] in "\"'" and text[-1] == text[0]:
        return text[1:-1]
    # An unquoted ` #` starts a comment in YAML; a `#` inside a token does not.
    if " #" in text:
        text = text.split(" #", 1)[0].strip()
    return text
