"""What a file *exposes*: the public symbols an episode added, and whether any of them has logic.

This is half of the ticket's hardest acceptance criterion — *"'New public logic, no new
test' has a documented and tunable definition rather than a guess"*. The definition lives in
[docs/concepts/test-surface.md](../../../docs/concepts/test-surface.md) §5; this module is
its first clause, **"is this symbol public, and is it logic?"**, executed:

| Language | Public means | Logic means |
| --- | --- | --- |
| Rust | a `pub` `fn`/`struct`/`enum`/`trait`/`type`/`const`/`static`, or a `pub fn` inside an `impl` (qualified `Type::method`) | ≥ 1 body line that is a call, an assignment, a control-flow keyword or a `return` |
| Python | a module-level `def`/`class` whose name does not start with `_`, and a non-`_` method of one of those classes (qualified `Class.method`) | same test, walked over the method body, so a `@dataclass` or a pure-constant class is exposed but not "logic" |
| Go | an exported (initial-capital) top-level `func`, method (`Type.Method`), `type` or `const`/`var` | same |
| TypeScript / JavaScript | an `export`ed `function`/`class`/`const`/`abstract class`, or a `module.exports.X` / `exports.X` assignment; a bare `export { a, b }` re-export is public but has no logic | same |
| Java | a `public` class and its `public` methods (`Class.method`) | same |

Nothing here decides whether a symbol *is covered by a test* — that is `policy.py`'s and
`delta.py`'s job, and every threshold that clause depends on is a config key. Keeping the
two apart is what makes the finding auditable: the card can say "these five public symbols
are new, these two of them have logic, these are the three ways the episode could have
counted as covering them", and a reviewer can disagree with exactly one of those steps.

A note on `pub(crate)` and friends: Rust visibility is `pub`, `pub(crate)`, `pub(super)` and
`pub(in path)`. Only plain `pub` counts as public here, because the crate-internal ones are
invisible outside the crate and a missing test for one is a smaller claim; a `pub(crate)`
symbol is still *reported* (with `visibility="pub(crate)"`) so a repository that wants the
stricter rule can ask for it in `test.untested.visibility` without the default being a guess
about what "public" meant to whoever wrote the module.
"""

from __future__ import annotations

import ast
import re
from dataclasses import dataclass

from . import scan as S
from . import parsers as P

PUBLIC_VERSION = 1

KIND_FN = "fn"
KIND_METHOD = "method"
KIND_CLASS = "class"
KIND_TYPE = "type"
KIND_CONST = "const"
KIND_INTERFACE = "interface"
KIND_TRAIT = "trait"
KIND_ENUM = "enum"
KIND_STRUCT = "struct"

#: One line counts as *logic* when it does something. A `}` or a field declaration does not.
LOGIC = re.compile(r"(?:"
                   r"\b(?:return|if|elif|else|for|while|match|loop|try|except|finally|with|"
                   r"await|yield|raise|throw|defer|go|switch|case|do|begin|rescue|ensure)\b"
                   r"|[A-Za-z_)\]]\s*\("
                   r"|[<>!=]=|[-+*/%&|^]|\s[-+*/%&^|]\s|=>|\b(?:&&|\|\||and|or|not)\b"
                   r")")


@dataclass(frozen=True)
class Symbol:
    """One exposed declaration, with the facts the tunable definition reads."""

    name: str
    language: str
    kind: str
    line: int
    end_line: int = 0
    body_lines: int = 0
    logic_lines: int = 0
    visibility: str = "public"

    @property
    def has_logic(self) -> bool:
        return self.logic_lines > 0

    def key(self) -> str:
        return "%s/%s" % (self.language, self.name.lower())

    def leaf(self) -> str:
        return self.name.rsplit(":", 1)[-1].rsplit(".", 1)[-1]

    def describe(self) -> str:
        return "%s %s (%d body line(s), %d logic)" % (self.name, self.kind, self.body_lines,
                                                      self.logic_lines)

    def as_dict(self) -> dict:
        return {"name": self.name, "language": self.language, "kind": self.kind,
                "line": self.line, "end_line": self.end_line, "body_lines": self.body_lines,
                "logic_lines": self.logic_lines, "has_logic": self.has_logic,
                "visibility": self.visibility}


def public_symbols(path: str, text: str | None) -> list[Symbol]:
    """Every public symbol in one revision. Never raises."""
    language = P.language_for(path)
    if not language or text is None:
        return []
    try:
        if language == P.PYTHON:
            return _python(text)
        masked = S.mask(text, P.LANGUAGES[language].syntax).splitlines()
        if language == P.RUST:
            return _rust(masked)
        if language == P.GO:
            return _go(masked)
        if language == P.TS:
            return _ts(masked)
        if language == P.JAVA:
            return _java(masked)
    except Exception:                                # noqa: BLE001 — no symbols, not a crash
        return []
    return []


def name_tokens(name: str) -> set[str]:
    """`GraphSnapshot::materialise` → `{graph, snapshot, materialise}`.

    The coverage clause that matches a symbol against a test case's name needs a comparison
    that survives `snake_case`, `camelCase`, `PascalCase` and `kebab-case` at once, because a
    Rust `pub fn load_snapshot` is the same claim as a Go `TestSnapshotLoading`. Splitting
    on case *and* punctuation is the only rule that works in five languages at once, and the
    overlap count it feeds is `test.untested.name_overlap`.
    """
    leaf = re.sub(r"[:./]", "_", name).rsplit("(", 1)[0]
    parts = re.split(r"[^0-9A-Za-z]+|(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])", leaf)
    return {part.lower() for part in parts if len(part) > 1}


# ---------------------------------------------------------------------------
# shared
# ---------------------------------------------------------------------------
def count_logic(lines: list[str], start: int, end: int) -> tuple[int, int]:
    """`(body_lines, logic_lines)` for a line span, ignoring blanks and lone brackets."""
    body = 0
    logic = 0
    for index in range(start, min(end + 1, len(lines))):
        stripped = lines[index].strip()
        if not stripped or stripped in ("{", "}", "};", "):", ")", "else {"):
            continue
        body += 1
        if LOGIC.search(stripped):
            logic += 1
    return body, logic


# ---------------------------------------------------------------------------
# Rust
# ---------------------------------------------------------------------------
RUST_PUB_ITEM = re.compile(
    r"^pub(?:\([^)]*\))?\s+(?:(?:const|async|unsafe|extern)\b[\w\"()]*\s+)*"
    r"(?P<kind>fn|struct|enum|trait|type|const|static|union|mod)\s+(?P<name>[A-Za-z_]\w*)")
RUST_IMPL = re.compile(r"^impl\b(?:\s*<[^>]*>)?\s*(?:(?P<trait>[\w:]+)\s+for\s+)?"
                       r"(?P<type>[\w:]+)")
RUST_VIS = re.compile(r"^pub(?:\((?P<narrow>[^)]*)\))?")
RUST_ATTR_LINE = re.compile(r"^#\[")
RUST_ITEM_MOD = re.compile(r"^(?:pub\s+)?mod\s+(?P<name>[A-Za-z_]\w*)\s*\{")
RUST_CFG_TEST = re.compile(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]")


def _rust(masked: list[str]) -> list[Symbol]:
    out: list[Symbol] = []
    stack: list[dict] = []
    depth = 0
    pending_cfg_test = False
    for index, line in enumerate(masked):
        stripped = line.strip()
        delta = S.brace_delta(line)
        if not stripped:
            depth += delta
            continue
        if RUST_CFG_TEST.search(stripped):
            pending_cfg_test = True
        elif RUST_ATTR_LINE.match(stripped):
            pending_cfg_test = False
        mod = RUST_ITEM_MOD.match(stripped)
        if mod:
            stack.append({"kind": "mod", "name": mod.group("name"), "depth": depth,
                          "test": pending_cfg_test})
            pending_cfg_test = False
            depth += delta
            continue
        pending_cfg_test = False
        impl = RUST_IMPL.match(stripped)
        if impl:
            stack.append({"kind": "impl", "name": (impl.group("type") or "").split("::")[-1],
                          "depth": depth})
            depth += delta
            continue
        item = RUST_PUB_ITEM.match(stripped)
        if item:
            kind = item.group("kind")
            name = item.group("name")
            narrow = RUST_VIS.match(stripped)
            visibility = "public"
            if narrow and narrow.group("narrow"):
                narrow_name = narrow.group("narrow").strip()
                if narrow_name == "crate":
                    visibility = "crate"
                elif narrow_name.startswith("super"):
                    visibility = "super"
                else:
                    visibility = "restricted"
            if any(frame.get("test") for frame in stack if frame["kind"] == "mod"):
                visibility = "test-module"
            inside_impl = [frame for frame in stack if frame["kind"] == "impl"]
            if inside_impl and kind == "fn":
                name = "%s::%s" % (inside_impl[-1]["name"], name)
                kind = KIND_METHOD
            elif kind == "fn":
                kind = KIND_FN
            end = S.block_end(masked, index) if "{" in line else index
            body, logic = count_logic(masked, index, end)
            out.append(Symbol(name=name, language=P.RUST, kind=kind, line=index + 1,
                              end_line=end + 1, body_lines=body,
                              logic_lines=logic if kind in (KIND_FN, KIND_METHOD) else 0,
                              visibility=visibility))
        depth += delta
        while stack and depth <= stack[-1]["depth"]:
            stack.pop()
    return out


# ---------------------------------------------------------------------------
# Python
# ---------------------------------------------------------------------------
def _python(text: str) -> list[Symbol]:
    tree = ast.parse(text)
    out: list[Symbol] = []

    def emit(node, prefix: str = "", kind: str = KIND_FN) -> Symbol:
        body_lines = getattr(node, "end_lineno", node.lineno) - node.lineno
        logic = 0
        for child in ast.walk(node):
            if isinstance(child, (ast.If, ast.For, ast.While, ast.Try, ast.With, ast.Return,
                                  ast.Yield, ast.YieldFrom, ast.Await, ast.Call, ast.Match)):
                logic += 1
            elif isinstance(child, ast.AugAssign):
                logic += 1
            elif isinstance(child, ast.Assign) \
                    and not isinstance(child.value, ast.Constant):
                logic += 1
        name = "%s.%s" % (prefix, node.name) if prefix else node.name
        return Symbol(name=name, language=P.PYTHON, kind=kind, line=node.lineno,
                      end_line=getattr(node, "end_lineno", node.lineno),
                      body_lines=max(0, body_lines),
                      logic_lines=0 if kind == KIND_CLASS else logic)

    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if not node.name.startswith("_"):
                out.append(emit(node))
        elif isinstance(node, ast.ClassDef):
            if node.name.startswith("_"):
                continue
            out.append(emit(node, kind=KIND_CLASS))
            for child in node.body:
                if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)) \
                        and not child.name.startswith("_") \
                        and child.name not in ("__init__", "__post_init__"):
                    out.append(emit(child, prefix=node.name, kind=KIND_METHOD))
    return out


# ---------------------------------------------------------------------------
# Go
# ---------------------------------------------------------------------------
GO_FUNC = re.compile(r"^func\s+(?:\(\s*\w*\s*\*?(?P<type>\w+)\s*\)\s*)?"
                     r"(?P<name>[A-Za-z_]\w*)\s*\(")
GO_TYPE = re.compile(r"^type\s+(?P<name>[A-Z]\w*)\s+(?P<kind>struct|interface|type|enum)\b")
GO_VALUE = re.compile(r"^var\s+(?P<name>[A-Z]\w*)|^const\s+(?P<cname>[A-Z]\w*)")


def _go(masked: list[str]) -> list[Symbol]:
    out: list[Symbol] = []
    for index, line in enumerate(masked):
        stripped = line.strip()
        if not stripped:
            continue
        kind_match = GO_TYPE.match(stripped)
        if kind_match:
            end = S.block_end(masked, index) if "{" in line else index
            out.append(Symbol(name=kind_match.group("name"), language=P.GO,
                              kind=kind_match.group("kind"), line=index + 1, end_line=end + 1))
            continue
        match = GO_FUNC.match(stripped)
        if not match:
            continue
        name = match.group("name")
        if not name[:1].isupper():
            continue
        full = name
        end = S.block_end(masked, index)
        body, logic = count_logic(masked, index, end)
        out.append(Symbol(name=full, language=P.GO, kind=KIND_FN, line=index + 1,
                          end_line=end + 1, body_lines=body, logic_lines=logic))
    return out


# ---------------------------------------------------------------------------
# TypeScript / JavaScript
# ---------------------------------------------------------------------------
TS_EXPORT = re.compile(
    r"^export\s+(?:(?P<kind>async\s+function|function|abstract\s+class|class|const|let|var|"
    r"interface|type|enum|namespace|enum)\s+(?P<name>[A-Za-z_$][\w$]*)|(?P<star>\*)|"
    r"(?P<brace>\{))")
TS_ARROW = re.compile(r"=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*=>|=\s*(?:async\s*)?"
                      r"function\b")
TS_MODULE_EXPORTS = re.compile(r"^(?:module\.)?exports\.(?P<name>[A-Za-z_$][\w$]*)\s*=")


def _ts(masked: list[str]) -> list[Symbol]:
    out: list[Symbol] = []
    for index, line in enumerate(masked):
        stripped = line.strip()
        if not stripped:
            continue
        module = TS_MODULE_EXPORTS.match(stripped)
        if module:
            end = S.block_end(masked, index) if "{" in line else index
            body, logic = count_logic(masked, index, end)
            out.append(Symbol(name=module.group("name"), language=P.TS, kind=KIND_FN,
                              line=index + 1, end_line=end + 1, body_lines=body,
                              logic_lines=logic))
            continue
        match = TS_EXPORT.match(stripped)
        if not match or not match.group("kind"):
            continue
        kind = match.group("kind").split()[-1]
        name = match.group("name")
        end = S.block_end(masked, index) if "{" in line else index
        body, logic = count_logic(masked, index, end)
        if kind in ("function",):
            kind = KIND_FN
        elif kind == "class":
            kind = KIND_CLASS
        elif kind in ("interface",):
            kind = KIND_INTERFACE
        elif kind in ("const", "let", "var"):
            arrow = TS_ARROW.search(stripped)
            kind = KIND_FN if arrow else KIND_CONST
            logic = logic if arrow else 0
        out.append(Symbol(name=name, language=P.TS, kind=kind, line=index + 1,
                          end_line=end + 1, body_lines=body,
                          logic_lines=0 if kind in (KIND_CLASS, KIND_INTERFACE, KIND_CONST)
                          else logic))
    return out


# ---------------------------------------------------------------------------
# Java
# ---------------------------------------------------------------------------
JAVA_CLASS = re.compile(r"^public\s+(?:(?:final|abstract|sealed|static)\s+)*"
                        r"(?:class|interface|enum|record)\s+(?P<name>\w+)")
JAVA_METHOD = re.compile(r"^public\s+(?:(?:static|final|abstract|synchronized|default)\s+)*"
                         r"(?P<type>[\w<>?,\[\]]+)\s+(?P<name>\w+)\s*\(")


def _java(masked: list[str]) -> list[Symbol]:
    out: list[Symbol] = []
    klass = ""
    for index, line in enumerate(masked):
        stripped = line.strip()
        if not stripped:
            continue
        klass_match = JAVA_CLASS.match(stripped)
        if klass_match:
            klass = klass_match.group("name")
            out.append(Symbol(name=klass, language=P.JAVA, kind=KIND_CLASS, line=index + 1))
            continue
        method = JAVA_METHOD.match(stripped)
        if method and klass:
            end = S.block_end(masked, index)
            body, logic = count_logic(masked, index, end)
            out.append(Symbol(name="%s.%s" % (klass, method.group("name")), language=P.JAVA,
                              kind=KIND_METHOD, line=index + 1, end_line=end + 1,
                              body_lines=body, logic_lines=logic))
    return out
