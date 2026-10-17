"""Text scanning primitives shared by every language half of this package.

Nothing here knows what a test *is*; it knows how to look at a line of code without being
tricked by one. Two problems are load-bearing for the whole ticket:

**Masking.** A parser that reads a raw source line will find `#[test]` inside a doc
comment, `assert.Equal` inside a string literal used as test data, and an unbalanced `{`
inside a template literal — and will then report a test case that does not exist. `mask()`
replaces comment and string *contents* with spaces while keeping every line and column
offset identical, so a regex run over the masked text reports positions that are still
correct against the raw text, and cannot see inside a literal at all.

**Blocks.** Case bodies, assertion spans and `#[cfg(test)]` module ranges all need "where
does this block end", which is brace counting over masked text. `_block_end()` returns the
line a block closes on, given the line it opens on, and refuses to lie: an unbalanced file
returns the last line and a note, because "the file did not balance" is evidence the card
should be able to print rather than a silently wrong extent.

The scanner is a scanner, not a compiler. It does not resolve macros, does not know that a
`#[cfg_attr(...)]` expands into an attribute, and treats a Rust raw string containing four
braces as four braces. Every place that limitation can change an answer, the parser says so
in `notes` and the confidence ladder in `delta.py` goes down a step rather than up.
"""

from __future__ import annotations

from dataclasses import dataclass

#: What `mask()` replaces, per language family. A delimiter list rather than one universal
#: set, because `#` starts a comment in Python and is an attribute marker in Rust, and
#: because a backtick string is multiline in TypeScript and arithmetic in Rust.
@dataclass(frozen=True)
class Syntax:
    """The comment and literal delimiters a language uses."""

    line: tuple[str, ...] = ("//",)
    block: tuple[tuple[str, str], ...] = (("/*", "*/"),)
    strings: tuple[str, ...] = ('"', "'")
    #: Delimiters whose contents may span lines even though they are not block comments.
    multiline: tuple[str, ...] = ()
    escapes: bool = True
    #: Line prefixes that make the rest of the line inert *only* at the start of a line
    #: (Python's `#`, shell's `#`).
    hash_line: bool = False


RUST_SYNTAX = Syntax(line=("//",), block=(("/*", "*/"),), strings=('"',), escapes=True)
GO_SYNTAX = Syntax(line=("//",), block=(("/*", "*/"),), strings=('"', "`"), escapes=True)
TS_SYNTAX = Syntax(line=("//",), block=(("/*", "*/"),), strings=('"', "'", "`"),
                   multiline=("`",), escapes=True)
JAVA_SYNTAX = Syntax(line=("//",), block=(("/*", "*/"),), strings=('"', "'"), escapes=True)
PY_SYNTAX = Syntax(line=("#",), block=(), strings=('"', "'"), multiline=('"""', "'''"),
                   escapes=True)


def mask(text: str, syntax: Syntax) -> str:
    """Blank out comments and string contents; keep offsets, newlines and code identical.

    Same length as the input, same line structure, so a match position in the masked text
    is the same position in the raw text.
    """
    out = list(text)
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        # block comments
        hit = False
        for opener, closer in syntax.block:
            if text.startswith(opener, i):
                end = text.find(closer, i + len(opener))
                end = n if end < 0 else end + len(closer)
                _blank(out, i, end)
                i = end
                hit = True
                break
        if hit:
            continue
        for opener in syntax.line:
            if text.startswith(opener, i):
                end = text.find("\n", i)
                end = n if end < 0 else end
                _blank(out, i, end)
                i = end
                hit = True
                break
        if hit:
            continue
        for quote in sorted(syntax.multiline, key=len, reverse=True):
            if text.startswith(quote, i):
                end = _find_close(text, i + len(quote), quote, syntax.escapes)
                _blank(out, i, end)
                i = end
                hit = True
                break
        if hit:
            continue
        for quote in syntax.strings:
            if ch == quote:
                end = _find_close(text, i + 1, quote, syntax.escapes)
                _blank(out, i, end)
                i = end
                hit = True
                break
        if hit:
            continue
        i += 1
    return "".join(out)


def _blank(out: list[str], start: int, end: int) -> None:
    for index in range(start, min(end, len(out))):
        if out[index] != "\n":
            out[index] = " "


def _find_close(text: str, start: int, quote: str, escapes: bool) -> int:
    """Index just past the closing `quote`, or end of text for an unterminated literal."""
    i = start
    n = len(text)
    length = len(quote)
    while i < n:
        if escapes and text[i] == "\\":
            i += 2
            continue
        if text.startswith(quote, i):
            return i + length
        i += 1
    return n


def lines_of(text: str) -> list[str]:
    """Split keeping nothing but content, 1-based indexing handled by the caller."""
    return text.splitlines()


def brace_delta(line: str) -> int:
    return line.count("{") - line.count("}")


def block_end(masked_lines: list[str], start: int, depth_at_start: int | None = None) -> int:
    """The 0-based index of the line that closes the block opened on `start`.

    Returns the last line of the file when the braces never balance — the caller prints
    that as a note rather than pretending to know where the block ended.
    """
    depth = depth_at_start if depth_at_start is not None else 0
    depth += brace_delta(masked_lines[start]) if start < len(masked_lines) else 0
    index = start
    while depth > 0:
        index += 1
        if index >= len(masked_lines):
            return len(masked_lines) - 1
        depth += brace_delta(masked_lines[index])
    return index


def block_start_depth(masked_lines: list[str], start: int) -> int:
    """Depth just before the line that opens a block, so a caller can detect its close."""
    depth = 0
    for index in range(start):
        depth += brace_delta(masked_lines[index])
    return depth


def first_argument(text: str, limit: int = 60) -> str:
    """The first top-level argument of a call whose body starts at `text`.

    Used to build an assertion's *subject* — the thing being asserted about — which is what
    lets "the same claim, made less precisely" be recognised at all (§weaken). Stops at a
    top-level comma, and at a closing bracket depth; never crosses a line.
    """
    depth = 0
    out: list[str] = []
    for ch in text:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif ch == "," and depth == 0:
            break
        out.append(ch)
        if len(out) >= limit:
            break
    return "".join(out)


def subject(text: str) -> str:
    """A stable key for "what was asserted about", tolerant of formatting.

    `result.total`, `self.svc.fetch(x)` and `svc.fetch( y )` all normalise to a comparable
    string, which is the only reason a `==` claim and an existence claim about the same
    value can be matched to each other. Lower-cased, whitespace-free, trailing `?`/`.clone()`
    dropped, clipped: it is a *key*, not a display string — the card prints the raw line.
    """
    if not text:
        return ""
    text = text.strip().lower()
    for noise in (" ", "\t", "\n"):
        text = text.replace(noise, "")
    while text.startswith("!"):
        text = text[1:]
    while text.endswith(("?", ".", ":")):
        text = text[:-1]
    for suffix in (".clone()", ".tostring()", ".tointostring()", ".unwrap()"):
        if text.endswith(suffix):
            text = text[: -len(suffix)]
    return text[:64]
