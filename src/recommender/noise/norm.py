"""Token-normalised comparison: is a diff *shape*, or is it *content*?

`e0.9`'s noise axis is a **path** fact, and its `reformat` level is deliberately
unreachable from a glob ([paths.md §9](../../../docs/concepts/paths.md)): "the agent
reindented 40 files" is a fact about a *diff*. This module is where that level earns its
keep. It answers one question per file pair:

    do these two revisions contain the same tokens?

not "do they contain the same characters once you squash whitespace", which is the
whitespace flag the ticket refuses (and which `e1.7` already ships as `=`). A formatter
does four things a whitespace comparison does not see: it **re-wraps** statements, it
**flips quote delimiters**, it **adds or removes trailing commas**, and — with
`goimports`, `isort` and rustfmt's `reorder_imports` — it **sorts import blocks**. Token
streams are the cheapest representation all four are invisible to, and they still *catch*
the change a whitespace flag waves through: `x = 1` next to `x = 2` differs by one token,
not by whitespace.

Per language, because "same tokens" needs a definition of a token:

| language group | how the stream is built | what is normalised away |
|---|---|---|
| C-family (rust, c, cpp, java, cs, kt, swift, scala, php, css) | hand-written scanner: identifiers, numbers, literals, operators, comments | all whitespace, line breaks, trailing commas, comment placement |
| JavaScript / TypeScript (js, jsx, ts, tsx, mjs, cjs) | the C scanner plus template literals, regex literals and JS literal rules | the above plus quote delimiters and `(x) =>` arrow parens — prettier's two non-whitespace edits |
| Go | the C scanner (gofmt touches whitespace, comment alignment and import order and nothing else) | the above plus import-block order |
| Python | the standard library's `tokenize`, with INDENT/DEDENT/NEWLINE/NL dropped | the above plus quote delimiters and string prefixes (black's two non-whitespace edits) |
| JSON | a token scan of the text, validated by the parser | indentation and any whitespace between tokens |
| YAML / TOML / INI | line-token streams, with block scalars and multi-line strings verbatim | indentation, padding, blank lines, comment placement |
| Markdown | word streams per block, fenced code kept line by line | paragraph re-wrapping, list markers, table padding, hard-break spaces |
| shell / ruby | the C scanner with `#` comments | the above |
| anything else | nothing — :func:`normalize` refuses | (the caller gets the weaker line-level claim, never a fake strong one) |

Three rules hold this together:

1. **A comment is content.** Formatters move comments; they do not rewrite them. Comment
   text stays in the stream (in order, position-independent), so a diff that edits a
   comment is *not* a reformat. Dropping comments would have been the cheapest way to
   pass every fixture here and the surest way to hide a change of meaning.
2. **A scanner that cannot finish refuses.** An unterminated literal, a Python file that
   does not parse, a truncated blob: the result carries a `refusal`, the strong claim is
   off the table, and the caller says which. A truncation-shaped token stream is exactly
   what a "0 semantic changes" line must never be derived from.
3. **The stream is a digest, not a copy.** `sha256` over the joined tokens is what the
   evidence line prints (`tokens 7e1c…2b`), so a claim about a file pair can be cited
   without shipping either file. Two pairs with equal digests make the same claim, which
   is what makes the number cacheable (`e1.4`'s key) and reviewable.
"""

from __future__ import annotations

import difflib
import hashlib
import json
import re
import tokenize as _tk
from dataclasses import dataclass, field
from io import StringIO

#: Version of the token model. Part of any cached key: a scanner fix changes answers.
NORM_VERSION = 1

#: Marks a comment token, so comment text is compared in order and never confused with
#: code. A NUL cannot appear in source text, so the marker cannot collide.
COMMENT = "\x00c\t"

#: A blank line / paragraph boundary in the line and word strategies.
BREAK = "\x00para"

#: A line kept exactly (block scalar, fenced code, template literal text).
VERBATIM = "\x00vb\t"

#: The multi-line string delimiters, spelled without colliding with this file's quoting.
TRIPLE_DOUBLE = chr(34) * 3
TRIPLE_SINGLE = chr(39) * 3


# ---------------------------------------------------------------------------
# the language table
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Language:
    """One entry of the table: what the extension is, and how its tokens are found."""

    id: str
    title: str
    tokenizer: str                       # "c" | "js" | "py" | "json" | "lines" | "words"
    extensions: tuple[str, ...]
    formatter: str                       # the formatter whose edits this normaliser erases
    notes: str = ""


LANGUAGES: dict[str, Language] = {
    "rust": Language("rust", "Rust", "c", (".rs",), "rustfmt",
                     "rustfmt re-wraps, re-indents, adds trailing commas and reorders `use` "
                     "lines; all four are invisible to a token stream"),
    "c": Language("c", "C", "c", (".c", ".h"), "clang-format"),
    "cpp": Language("cpp", "C++", "c", (".cc", ".cpp", ".cxx", ".hpp", ".hh", ".hxx", ".inl"),
                    "clang-format"),
    "java": Language("java", "Java", "c", (".java",), "google-java-format"),
    "csharp": Language("csharp", "C#", "c", (".cs",), "dotnet format"),
    "kotlin": Language("kotlin", "Kotlin", "c", (".kt", ".kts"), "ktfmt"),
    "swift": Language("swift", "Swift", "c", (".swift",), "swift-format"),
    "scala": Language("scala", "Scala", "c", (".scala",), "scalafmt"),
    "php": Language("php", "PHP", "c", (".php",), "php-cs-fixer"),
    "css": Language("css", "CSS", "c", (".css", ".scss", ".less"), "prettier"),
    "javascript": Language("javascript", "JavaScript", "js",
                           (".js", ".jsx", ".mjs", ".cjs"), "prettier",
                           "prettier flips quote delimiters and adds `(x) =>` parentheses; "
                           "both are normalised here, so the claim survives them"),
    "typescript": Language("typescript", "TypeScript", "js", (".ts", ".mts", ".cts"), "prettier"),
    "tsx": Language("tsx", "TypeScript/JSX", "js", (".tsx",), "prettier"),
    "go": Language("go", "Go", "c", (".go",), "gofmt",
                   "gofmt aligns and re-indents; goimports additionally sorts the import "
                   "block, which the import-run sort below erases"),
    "python": Language("python", "Python", "py", (".py", ".pyi"), "black",
                       "black re-wraps, adds magic trailing commas, prefers `\"` and "
                       "lowercases string prefixes"),
    "json": Language("json", "JSON", "json", (".json",), "prettier",
                     "scanned, not re-serialised: `1.0` staying `1.0` is still a difference"),
    "yaml": Language("yaml", "YAML", "lines", (".yaml", ".yml"), "prettier",
                     "block scalars (`|`, `>`) are kept verbatim inside: whitespace there "
                     "is content"),
    "toml": Language("toml", "TOML", "lines", (".toml",), "taplo",
                     "multi-line strings (`\"\"\"`) are kept verbatim inside"),
    "ini": Language("ini", "INI/properties", "lines", (".ini", ".cfg", ".properties"),
                    "prettier"),
    "markdown": Language("markdown", "Markdown", "words", (".md", ".markdown", ".mdx"),
                         "prettier",
                         "fenced code blocks are kept line-by-line: reflowing a code sample "
                         "is a content change"),
    "shell": Language("shell", "Shell", "c", (".sh", ".bash", ".zsh"), "shfmt",
                      "`${x}` vs `$x` is a token difference, so shfmt's quoting edits are "
                      "reported as change rather than folded"),
    "ruby": Language("ruby", "Ruby", "c", (".rb", ".rake", ".gemspec"), "standardrb"),
    "lock": Language("lock", "package-manager lockfile", "lines", (".lock",),
                     "the package manager",
                     "lock formats are line-oriented: padding and blank lines are erased and "
                     "a changed value is a changed line"),
    "snapshot": Language("snapshot", "Jest snapshot", "js", (".snap",), "jest",
                         "a snapshot file is JavaScript whose literals are data, so a "
                         "reflowed snapshot is a changed assertion, not a reflow"),
}

_BY_EXT: dict[str, str] = {}
for _lang in LANGUAGES.values():
    for _ext in _lang.extensions:
        _BY_EXT[_ext] = _lang.id

#: Line prefixes that open an import statement, per tokenizer family. A run of two or more
#: is a *block*, and sorting import blocks is a formatter's prerogative and nobody else's.
IMPORT_PREFIXES: dict[str, tuple[str, ...]] = {
    "c": ("use ", "use{", "import ", "#include ", "using "),
    "js": ("import ",),
    "py": ("import ", "from "),
    "ruby": ("require ", "require_relative "),
}

_LIST_MARKER = re.compile(r"^(?:[-*+]|\d+[.)])\s+")
_HEADING = re.compile(r"^(#{1,6})\s+")
_HARD_BREAK = re.compile(r" {2,}$")


def language_for(path: str) -> Language | None:
    """The table entry for a path, or ``None`` when no token model covers it.

    The **last** dot wins, because `api.pb.go` is Go and `button.component.ts` is
    TypeScript; a first-dot rule would call both unknown and quietly downgrade the strong
    claim to the line-level one.
    """
    base = path.lower().rsplit("/", 1)[-1]
    ext_at = base.rfind(".")
    if ext_at <= 0:
        return None
    lang_id = _BY_EXT.get(base[ext_at:])
    return LANGUAGES[lang_id] if lang_id else None


def languages_sorted() -> list[Language]:
    return sorted(LANGUAGES.values(), key=lambda lang: lang.id)


# ---------------------------------------------------------------------------
# the result
# ---------------------------------------------------------------------------
@dataclass
class Normal:
    """The comparable form of one file revision. Never raises: it refuses instead."""

    path: str
    language: str = "unknown"
    strategy: str = "none"               # tokens | json | lines | words | none
    stream: tuple[str, ...] | None = None
    notes: list[str] = field(default_factory=list)
    refusals: list[str] = field(default_factory=list)
    lines: int = 0
    norm_version: int = NORM_VERSION

    @property
    def claimable(self) -> bool:
        """Whether the strong (token-level) claim may be made from this."""
        return self.stream is not None and not self.refusals

    @property
    def tokens(self) -> int:
        return 0 if self.stream is None else len(self.stream)

    @property
    def comment_tokens(self) -> int:
        if self.stream is None:
            return 0
        return sum(1 for token in self.stream if token.startswith(COMMENT))

    @property
    def code_tokens(self) -> int:
        return self.tokens - self.comment_tokens

    def digest(self) -> str:
        """`sha256` over the stream — the evidence handle, never the stream itself."""
        if self.stream is None:
            return ""
        joined = "\x01".join(self.stream).encode("utf-8", "surrogatepass")
        return hashlib.sha256(joined).hexdigest()

    def reason_not_claimable(self) -> str:
        if self.refusals:
            return "; ".join(self.refusals)
        if self.language == "unknown":
            return "no token model for this path"
        return "no token stream was produced"

    def as_dict(self) -> dict:
        return {"path": self.path, "language": self.language, "strategy": self.strategy,
                "tokens": self.tokens, "code_tokens": self.code_tokens,
                "comments": self.comment_tokens, "digest": self.digest(), "lines": self.lines,
                "notes": list(self.notes), "refusals": list(self.refusals),
                "claimable": self.claimable, "norm_version": self.norm_version}


class _Refuse(Exception):
    """This revision cannot be tokenised. Say why, and let the caller report it."""


def normalize(path: str, text: str | None) -> Normal:
    """Token-normalise one revision. Never raises; refuses instead.

    `text=None` is "the bytes are not here" (the deleted side of a rename, a blob the
    index has not streamed, a read that failed) and is a refusal with its own reason,
    because "I could not look" and "I looked and it changed" must never collapse.
    """
    if text is None:
        return Normal(path=path, refusals=["this revision's content is not available"])
    lang = language_for(path)
    if lang is None:
        return Normal(path=path, refusals=["no token model for %s" % _suffix(path)])
    try:
        if lang.tokenizer == "py":
            stream, notes, refusals = _tokenize_python(text)
        elif lang.tokenizer == "json":
            stream, notes, refusals = _tokenize_json(text)
        elif lang.tokenizer == "lines":
            stream, notes, refusals = _tokenize_lines(text, lang)
        elif lang.tokenizer == "words":
            stream, notes, refusals = _tokenize_words(text)
        else:
            stream, notes, refusals = _tokenize_c(text, js=(lang.tokenizer == "js"))
    except _Refuse as exc:
        return Normal(path=path, language=lang.id, lines=line_count(text),
                      refusals=[str(exc)])
    moved = 0
    stream, moved = _sort_import_runs(text, lang, stream)
    if moved:
        notes.append("%d import line(s) re-sorted (formatter import order)" % moved)
    strategy = {"py": "tokens", "c": "tokens", "js": "tokens", "json": "json",
                "lines": "lines", "words": "words"}[lang.tokenizer]
    return Normal(path=path, language=lang.id, strategy=strategy, stream=tuple(stream),
                  notes=notes, refusals=refusals, lines=line_count(text))


def same_tokens(a: Normal, b: Normal) -> bool:
    """The strong claim: identical token streams, comments included."""
    if not a.claimable or not b.claimable:
        return False
    if a.language != b.language or a.strategy != b.strategy:
        return False
    return a.stream == b.stream


def digest_pair(a: Normal, b: Normal) -> str:
    """The short handle that cites one whole file-pair claim."""
    joined = "%s|%s|%s" % (a.digest(), b.digest(), NORM_VERSION)
    return hashlib.sha256(joined.encode()).hexdigest()[:12]


def _suffix(path: str) -> str:
    dot = path.rfind(".")
    return path[dot:] if dot > 0 else "this path"


def line_count(text: str) -> int:
    return text.count("\n") + (0 if not text or text.endswith("\n") else 1)


# ---------------------------------------------------------------------------
# the C-family scanner (and its JS dialect)
# ---------------------------------------------------------------------------
_PUNCT = [
    "...", "<<=", ">>=", "->>", "=>>", "===", "!==", "**=", "&&=", "||=", "??=",
    "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "**", "++", "--",
    "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "::", ":=", "&&",
    "|>", "::<",
]

#: Single-character operators and delimiters, checked only after the multi-character list
#: above has failed to match, so `<=` never tokenises as `<` then `=`.
_SINGLE = "{}()[]<>,;:.+-*/%&|^!~=?@"


def _is_ident_start(ch: str) -> bool:
    return ch.isalpha() or ch in "_$" or ord(ch) > 127


def _is_ident(ch: str) -> bool:
    return ch.isalnum() or ch in "_$" or ord(ch) > 127


def _at_line_start(text: str, i: int) -> bool:
    j = i - 1
    while j >= 0 and text[j] in " \t":
        j -= 1
    return j < 0 or text[j] == "\n"


def _scan_string(text: str, i: int, quote: str, *, raw: bool = False,
                 closer: str | None = None) -> tuple[str, int]:
    """(literal source, index past it). Raises :class:`_Refuse` when unterminated."""
    closing = closer if closer is not None else quote
    j = i + 1
    while j < len(text):
        if not raw and text[j] == "\\":
            j += 2
            continue
        if text.startswith(closing, j):
            return text[i:j + len(closing)], j + len(closing)
        if text[j] == "\n":
            raise _Refuse("unterminated string literal")
        j += 1
    raise _Refuse("unterminated string literal")

def _scan_char(text: str, i: int) -> tuple[str, int] | None:
    """A C/Rust/Java character literal (`'a'`, `'\\''`, `'\\n'`, `'\\u{1f600}'`).

    Returns ``None`` when the quote is a Rust lifetime or an apostrophe in a comment we
    have already consumed: the caller then treats it as identifier material.
    """
    j = i + 1
    if j >= len(text):
        return None
    if text[j] == "\\":
        k = j + 1
        if text[k:k + 1] == "{":                     # '\u{...}'
            end = text.find("}", k)
            if end < 0:
                return None
            k = end + 1
        else:
            k += 1
        if text[k:k + 1] == "'":
            return text[i:k + 1], k + 1
        return None
    if text[j] == "'":
        return None
    k = j + 1
    if text[k:k + 1] == "'":
        return text[i:k + 1], k + 1
    return None


def _normalize_js_literal(literal: str) -> str:
    """Erase the *delimiter spelling* of a JS string; never touch what is between them."""
    quote = literal[0]
    if quote not in "'\"":
        return literal
    body = literal[1:-1].replace("\\" + quote, quote).replace('\\"', '"')
    return '"' + body + '"'


def _tokenize_c(text: str, *, js: bool = False) -> tuple[list[str], list[str], list[str]]:
    out: list[str] = []
    notes: list[str] = []
    i = 0
    n = len(text)
    prev = ""

    while i < n:
        ch = text[i]
        if ch in " \t\r\n\f\v":
            i += 1
            continue
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
            out.append(COMMENT + " ".join(text[i:end].split()))
            i = end
            continue
        if text.startswith("/*", i):
            end = text.find("*/", i + 2)
            if end < 0:
                raise _Refuse("unterminated block comment")
            out.append(COMMENT + " ".join(text[i:end + 2].split()))
            i = end + 2
            continue
        if js and ch == "`":
            out.extend(_scan_template(text, i))
            i = _template_end(text, i)
            continue
        if ch == "#" and not js and _at_line_start(text, i):
            # A preprocessor line: `#include <x>` becomes one token, so an angle-bracket
            # include compares the same way a quoted one does.
            end = text.find("\n", i)
            end = n if end < 0 else end
            out.append(" ".join(text[i:end].split()))
            i = end
            continue
        if ch in "'\"":
            if ch == "'" and not js:
                maybe = _scan_char(text, i)            # `'a'` / `'\\n'` is a literal
                if maybe is None:                      # …anything else is a lifetime
                    k = i + 1
                    while k < n and _is_ident(text[k]):
                        k += 1
                    out.append(text[i:k])
                    prev = text[i:k]
                    i = k
                    continue
                literal, i = maybe
                out.append(literal)
                prev = literal
                continue
            if ch == "'" and js:
                # An apostrophe inside JSX text is not a string. Try, and fall back to
                # treating it as punctuation rather than refusing the whole file.
                try:
                    literal, j = _scan_string(text, i, "'")
                except _Refuse:
                    out.append("\x00apos")
                    prev = "'"
                    i += 1
                    continue
                out.append(_normalize_js_literal(literal))
                prev = literal
                i = j
                continue
            literal, i = _scan_string(text, i, ch)
            out.append(_normalize_js_literal(literal) if js else literal)
            prev = literal
            continue
        if ch.isdigit() or (ch == "." and text[i + 1:i + 2].isdigit()):
            j = i
            while j < n:
                cur = text[j]
                if cur.isalnum() or cur == "_":
                    j += 1
                    continue
                if cur == "." or (cur in "+-" and text[j - 1:j] in ("e", "E", "p", "P")):
                    j += 1
                    continue
                break
            word = text[i:j]
            out.append(word)
            prev = word
            i = j
            continue
        if _is_ident_start(ch) or (ch == "'" and not js):
            if ch == "'":
                k = i + 1
                while k < n and _is_ident(text[k]):
                    k += 1
                out.append(text[i:k])
                prev = text[i:k]
                i = k
                continue
            j = i
            while j < n and _is_ident(text[j]):
                j += 1
            word = text[i:j]
            out.append(word)
            prev = word
            i = j
            continue
        if js and ch == "/" and _regex_allowed(prev):
            try:
                literal, i = _scan_regex(text, i)
            except _Refuse:
                out.append(ch)
                i += 1
                continue
            out.append(literal)
            prev = "/"
            continue
        operator = ""
        for candidate in _PUNCT:
            if text.startswith(candidate, i):
                operator = candidate
                break
        if not operator and ch in _SINGLE:
            operator = ch
        if operator:
            out.append(operator)
            prev = operator
            i += len(operator)
            continue
        out.append(ch)                                        # punctuation stays visible
        prev = ch
        i += 1
    out, commas = _drop_trailing_commas(out)
    if commas:
        notes.append("%d trailing comma(s) dropped before a closing bracket" % commas)
    if js:
        out, dropped = _drop_arrow_parens(out)
        if dropped:
            notes.append("%d single-parameter arrow paren(s) normalised" % dropped)
        out, jsx = _drop_jsx_parens(out)
        if jsx:
            notes.append("%d redundant JSX wrapper paren(s) normalised" % jsx)
    return out, notes, []


def _bracket_pairs(tokens: list[str]) -> dict[int, int]:
    """closer index → opener index, for a well-formed token stream."""
    pairs: dict[int, int] = {}
    stack: list[int] = []
    openers = {"(": 1, "[": 1, "{": 1}
    closers = {")": "(", "]": "[", "}": "{"}
    for index, token in enumerate(tokens):
        if token in openers:
            stack.append(index)
        elif token in closers and stack:
            pairs[index] = stack.pop()
    return pairs


def _drop_trailing_commas(tokens: list[str]) -> tuple[list[str], int]:
    """Drop a comma that sits directly before its closing bracket, with one exception.

    `(a,)` is **not** `(a)`: in Rust, Python and C++ a one-element parenthesised tuple
    changes type when the comma goes, so a comma closing a *parenthesised* group is dropped
    only when the group held more than one element or the bracket is a call. Everything else
    is layout — `[1, 2,]`, `{a: 1,}`, `f(a, b,)` — and black, rustfmt, clang-format and
    prettier all add those commas on a wrap and take them back on a join.
    """
    pairs = _bracket_pairs(tokens)
    drop: set[int] = set()
    for index, token in enumerate(tokens):
        if token != ",":
            continue
        following = tokens[index + 1] if index + 1 < len(tokens) else ""
        if following not in (")", "]", "}"):
            continue
        opener = pairs.get(index + 1)
        if opener is None:
            continue
        if following == ")":
            inner = tokens[opener + 1:index]
            before = tokens[opener - 1] if opener else ""
            if len(inner) <= 1 and not _calls_like(before):
                continue
        drop.add(index)
    if not drop:
        return tokens, 0
    return [tok for index, tok in enumerate(tokens) if index not in drop], len(drop)


def _calls_like(token: str) -> bool:
    """Does a `(` standing after `token` open a call or an index rather than a tuple?"""
    if not token:
        return False
    return token in (")", "]", "}") or token[0].isalpha() or token[0] in "_$"


def _regex_allowed(prev: str) -> bool:
    if not prev:
        return True
    return prev in ("(", ",", "=", ":", "[", "!", "&", "|", "?", "{", "}", ";", "return",
                    "typeof", "instanceof", "in", "of", "new", "case", "=>", "&&", "||",
                    "await", "&&")


def _scan_regex(text: str, i: int) -> tuple[str, int]:
    j = i + 1
    in_class = False
    while j < len(text):
        ch = text[j]
        if ch == "\\":
            j += 2
            continue
        if ch == "[":
            in_class = True
        elif ch == "]":
            in_class = False
        elif ch == "/" and not in_class:
            k = j + 1
            while k < len(text) and text[k].isalpha():
                k += 1
            return text[i:k], k
        elif ch == "\n":
            break
        j += 1
    raise _Refuse("unterminated regular expression")


def _template_end(text: str, i: int) -> int:
    """Index just past a JS template literal, with `${}` nesting honoured."""
    j = i + 1
    while j < len(text):
        if text[j] == "\\":
            j += 2
            continue
        if text.startswith("${", j):
            depth = 0
            while j < len(text):
                if text[j] == "{":
                    depth += 1
                elif text[j] == "}":
                    depth -= 1
                    if depth == 0:
                        j += 1
                        break
                elif text[j] in "\"'`":
                    quote = text[j]
                    j += 1
                    while j < len(text) and text[j] != quote:
                        j += 1 if text[j] != "\\" else 2
                    j += 1
                    continue
                j += 1
            continue
        if text[j] == "`":
            return j + 1
        j += 1
    raise _Refuse("unterminated template literal")


def _scan_template(text: str, i: int) -> list[str]:
    """A template literal's tokens: text lines verbatim, interpolations tokenised."""
    end = _template_end(text, i)
    body = text[i + 1:end - 1]
    out: list[str] = ["`"]
    buf = ""
    j = 0
    while j < len(body):
        if body.startswith("${", j):
            if buf:
                out.append(VERBATIM + buf)
                buf = ""
            stop = _interp_end(body, j)
            inner = body[j + 2:stop - 1]
            try:
                tokens, _notes, _refusals = _tokenize_c(inner, js=True)
            except _Refuse:
                tokens = [VERBATIM + inner]
            out.extend("\x00interp\t" + token for token in tokens)
            j = stop
            continue
        if body[j] == "\n":
            out.append(VERBATIM + buf)
            buf = ""
            j += 1
            continue
        buf += body[j]
        j += 1
    if buf:
        out.append(VERBATIM + buf)
    out.append("`")
    return out


def _interp_end(body: str, i: int) -> int:
    depth = 0
    j = i
    while j < len(body):
        if body[j] == "{":
            depth += 1
        elif body[j] == "}":
            depth -= 1
            if depth == 0:
                return j + 1
        j += 1
    raise _Refuse("unterminated template interpolation")


def _drop_jsx_parens(tokens: list[str]) -> tuple[list[str], int]:
    """`( <jsx> )` and `<jsx>` are the same tree; prettier adds the parens when it wraps.

    Scoped to a parenthesis whose first enclosed token opens a JSX element, because there
    the parentheses can never carry meaning. Redundant parentheses in general are *not*
    dropped: `(a + b) * c` and `a + b * c` are different programs.
    """
    dropped = 0
    current = tokens
    for _round in range(4):
        pairs = _bracket_pairs(current)
        drop: set[int] = set()
        for closer, opener in pairs.items():
            if current[opener + 1] == "<":
                drop.add(opener)
                drop.add(closer)
        if not drop:
            break
        current = [tok for index, tok in enumerate(current) if index not in drop]
        dropped += len(drop) // 2
    return current, dropped


def _drop_arrow_parens(tokens: list[str]) -> tuple[list[str], int]:
    """`(x) =>` and `x =>` are the same function to prettier; erase the parentheses."""
    out: list[str] = []
    dropped = 0
    i = 0
    while i < len(tokens):
        if (tokens[i] == "(" and i + 3 < len(tokens) and tokens[i + 3] == "=>"
                and tokens[i + 2] == ")" and _is_ident(tokens[i + 1])
                and not tokens[i + 1][0].isdigit()):
            out.append(tokens[i + 1])
            out.append("=>")
            dropped += 1
            i += 4
            continue
        out.append(tokens[i])
        i += 1
    return out, dropped


# ---------------------------------------------------------------------------
# Python
# ---------------------------------------------------------------------------
def _tokenize_python(text: str) -> tuple[list[str], list[str], list[str]]:
    """The standard library's tokenizer, minus everything a formatter may change freely.

    INDENT/DEDENT/NEWLINE/NL/ENDMARKER are dropped — that is the whole of black's whitespace
    vocabulary. `;` stays, so joining two statements with a semicolon is *not* a reformat:
    Python's formatter does not do that, and a claim that it did would hide a change.
    """
    out: list[str] = []
    notes: list[str] = []
    comments = 0
    try:
        tokens = list(_tk.generate_tokens(StringIO(text).readline))
    except (SyntaxError, IndentationError) as exc:
        raise _Refuse("the Python revision does not parse (%s)" % exc) from None
    except Exception as exc:                                # noqa: BLE001
        raise _Refuse("the Python revision could not be tokenised (%s)"
                      % type(exc).__name__) from None
    drop = {_tk.INDENT, _tk.DEDENT, _tk.NL, _tk.NEWLINE, _tk.ENDMARKER}
    for tok in tokens:
        if tok.type in drop:
            continue
        if tok.type == _tk.COMMENT:
            out.append(COMMENT + " ".join(tok.string[1:].split()))
            comments += 1
            continue
        if tok.type == _tk.STRING:
            out.append(_normalize_py_string(tok.string))
            continue
        out.append(tok.string)
    out, commas = _drop_trailing_commas(out)
    if commas:
        notes.append("%d magic trailing comma(s) dropped (black adds them on a wrap)"
                     % commas)
    if comments:
        notes.append("%d comment(s) kept in order, position ignored" % comments)
    return out, notes, []


def _normalize_py_string(literal: str) -> str:
    """black's two literal edits: lowercase the prefix, prefer `\"`."""
    prefix = ""
    rest = literal
    while rest and rest[0] not in "'\"":
        prefix += rest[0]
        rest = rest[1:]
    if not rest:
        return literal.lower()
    quote = rest[0]
    triple = rest[:3] in ("'''", '"""')
    body = rest[3:-3] if triple else rest[1:-1]
    body = body.replace("\\" + quote, quote)
    wrap = '"""' if triple else '"'
    return prefix.lower() + wrap + body + wrap


# ---------------------------------------------------------------------------
# JSON
# ---------------------------------------------------------------------------
def _tokenize_json(text: str) -> tuple[list[str], list[str], list[str]]:
    """Scanned, not re-serialised: the number text is part of the answer."""
    stripped = text.strip()
    if not stripped:
        raise _Refuse("the JSON revision is empty")
    try:
        json.loads(stripped)
    except ValueError as exc:
        raise _Refuse("the JSON revision does not parse (%s)" % exc) from None
    out: list[str] = []
    i = 0
    while i < len(stripped):
        ch = stripped[i]
        if ch in " \t\r\n":
            i += 1
            continue
        if ch == '"':
            literal, i = _scan_string(stripped, i, '"')
            out.append(literal)
            continue
        if ch in "{}[],:":
            out.append(ch)
            i += 1
            continue
        j = i
        while j < len(stripped) and stripped[j] not in " \t\r\n{}[],:":
            j += 1
        out.append(stripped[i:j])
        i = j
    return out, ["parsed as JSON, then scanned: only whitespace between tokens is ignored"], []


# ---------------------------------------------------------------------------
# line- and word-shaped formats
# ---------------------------------------------------------------------------
def _tokenize_lines(text: str, lang: Language) -> tuple[list[str], list[str], list[str]]:
    """Line-token streams for indentation-shaped formats (YAML, TOML, INI).

    Outside a verbatim region a line becomes its whitespace-separated words, so alignment,
    padding, blank lines and comment placement disappear. Inside one — a YAML block scalar,
    a TOML multi-line string — the line is kept exactly, because there the whitespace *is*
    the value.
    """
    out: list[str] = []
    notes: list[str] = []
    verbatim = ""
    verbatim_mode = ""
    verbatim_indent = -1
    blanks = 0
    verbatim_lines = 0
    for line in text.splitlines():
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" \t"))
        if verbatim:
            closed = bool(stripped) and verbatim in stripped
            inside = (indent > verbatim_indent or not stripped) if verbatim_mode == "indent" \
                else not closed
            if inside:
                out.append(VERBATIM + line)
                verbatim_lines += 1
                if closed:
                    verbatim, verbatim_mode = "", ""
                continue
            verbatim, verbatim_mode = "", ""
        if not stripped:
            blanks += 1
            continue
        if stripped.startswith("#"):
            out.append(COMMENT + " ".join(stripped.split()))
            continue
        verbatim, verbatim_mode, verbatim_indent = _opens_verbatim(stripped, lang, indent,
                                                                   verbatim)
        out.extend(_line_words(stripped))
    notes.append("line-token stream: one token per non-blank line, indentation and padding "
                 "ignored, %d blank line(s) dropped" % blanks)
    if verbatim_lines:
        notes.append("%d line(s) inside a verbatim block, compared exactly" % verbatim_lines)
    return out, notes, []


#: What splits a line into tokens in the line-shaped strategies: whitespace, and the
#: structural punctuation whose spacing a formatter (taplo, prettier) is free to change.
_LINE_SPLIT = re.compile(r"\s+|(?<=[\[\]{},:=])|(?=[\[\]{},:=])")


def _line_words(stripped: str) -> list[str]:
    return [token for token in _LINE_SPLIT.split(stripped) if token]


def _opens_verbatim(stripped: str, lang: Language, indent: int,
                    open_marker: str) -> tuple[str, str, int]:
    """(marker, mode, indent) when this line opens a region whose whitespace *is* content.

    Two modes, because two things end such a region: a YAML block scalar runs while the
    indentation holds (`indent`), a TOML or INI multi-line string runs until the closing
    quotes (`quote`). Getting this wrong is how a formatter re-indenting a literal block
    would be read as a value change — or a value change inside one read as a re-indent.
    """
    if open_marker:
        return "", "", -1
    if lang.id in ("yaml", "lock") and re.search(r"[|>][-+]?\d*$", stripped):
        return "block", "indent", indent
    if lang.id in ("toml", "ini"):
        for marker in (TRIPLE_DOUBLE, TRIPLE_SINGLE):
            if marker in stripped and stripped.count(marker) == 1:
                return marker, "quote", indent
    return "", "", -1


def _tokenize_words(text: str) -> tuple[list[str], list[str], list[str]]:
    """Word streams for Markdown: a re-wrapped paragraph is a formatter edit, not a change."""
    out: list[str] = []
    notes: list[str] = ["word stream per block: paragraph re-wrapping, list markers and table "
                        "padding are erased; fenced code is compared line by line"]
    fence = ""
    for raw in text.splitlines():
        line = raw.rstrip()
        stripped = line.strip()
        if stripped[:3] in ("```", "~~~"):
            fence = "" if fence else stripped[:3]
            out.append("\x00fence\t" + stripped[:3])
            continue
        if fence:
            out.append(VERBATIM + line)
            continue
        if not stripped:
            if out and out[-1] != BREAK:
                out.append(BREAK)
            continue
        body = stripped
        while body.startswith(">"):
            body = body[1:].lstrip()
        marker = _LIST_MARKER.match(body)
        if marker:
            body = body[marker.end():]
            out.append("\x00li\t-")
        if body.startswith("|") and body.endswith("|"):
            cells = [cell.strip() for cell in body.strip("|").split("|")]
            if all(set(cell) <= set("-: ") for cell in cells):
                continue                                    # an alignment row: formatting
            for cell in cells:
                out.extend(cell.split())
            out.append("\x00row")
            continue
        heading = _HEADING.match(body)
        if heading:
            out.append("\x00h%d" % len(heading.group(1)))
            body = body[heading.end():]
        body = body.replace("**", "\x00em").replace("__", "\x00em")
        body = _HARD_BREAK.sub("", body)
        out.extend(body.split())
    return out, notes, []


# ---------------------------------------------------------------------------
# import blocks
# ---------------------------------------------------------------------------
def _sort_import_runs(text: str, lang: Language,
                      tokens: list[str]) -> tuple[list[str], int]:
    """Erase formatter import order, and report how many lines it moved.

    The comparison must not be order-insensitive *everywhere*: reordering two statements is
    a content change and always will be. Import blocks are the one place where order is a
    formatter's to choose (`reorder_imports`, `goimports`, `isort`), so a maximal run of
    import lines is canonically sorted and everything else keeps its order. Both sides of
    the pair get the same treatment, so the only thing this can hide is an import re-order.

    Two shapes: a flat run of one-statement-per-line imports, and Go's (and JS's) parenthesised
    block, where the lines *inside* the brackets sort and the brackets do not move.
    """
    prefixes = IMPORT_PREFIXES.get(lang.tokenizer)
    if not prefixes:
        return tokens, 0
    lines = text.splitlines()
    runs: list[tuple[int, int]] = []
    index = 0
    while index < len(lines):
        stripped = lines[index].strip()
        if not stripped or stripped.startswith("//") or not stripped.startswith(prefixes):
            index += 1
            continue
        if stripped.endswith(("(", "{")):
            closer = ")" if stripped.endswith("(") else "}"
            end = index + 1
            while end < len(lines) and lines[end].strip() not in (closer, closer + ";"):
                end += 1
            if end > index + 1 and end < len(lines):
                runs.append((index + 1, end))
                index = end + 1
                continue
            index += 1
            continue
        start = index
        index += 1
        while index < len(lines) and lines[index].strip().startswith(prefixes):
            index += 1
        if index - start >= 2:
            runs.append((start, index))
    if not runs:
        return tokens, 0
    moved = 0
    for begin, end in runs:
        block = lines[begin:end]
        canonical = sorted(block, key=_import_sort_key)
        if canonical != block:
            moved += end - begin
            lines[begin:end] = canonical
    if not moved:
        return tokens, 0
    resorted = "\n".join(lines) + ("\n" if text.endswith("\n") else "")
    try:
        if lang.tokenizer == "py":
            fresh, _notes, _refusals = _tokenize_python(resorted)
        else:
            fresh, _notes, _refusals = _tokenize_c(resorted, js=(lang.tokenizer == "js"))
    except _Refuse:
        return tokens, 0
    return fresh, moved


def _import_sort_key(line: str) -> str:
    body = line.strip().rstrip(";").strip()
    parts = body.replace("{", " ").replace("}", " ").replace(",", " ").split()
    return " ".join(parts).lower()


# ---------------------------------------------------------------------------
# the line-level fallback (language independent, weaker, always available)
# ---------------------------------------------------------------------------
@dataclass
class LineStats:
    """What the raw lines say, with no language knowledge at all."""

    same: bool = False
    changed_pairs: int = 0               # replaced line pairs
    ws_only_pairs: int = 0               # ... of which only whitespace moved
    blank_only: int = 0                  # added or removed lines that were blank
    inserted: int = 0
    deleted: int = 0
    crlf_before: int = 0
    crlf_after: int = 0
    samples: list[str] = field(default_factory=list)
    wrap_only: bool = False              # same words, different number of lines

    @property
    def changed_lines(self) -> int:
        return self.changed_pairs + self.inserted + self.deleted

    @property
    def carries_change(self) -> bool:
        """Whether at least one changed line carries something a formatter cannot write."""
        return (self.changed_pairs - self.ws_only_pairs) > 0 or \
            (self.inserted + self.deleted - self.blank_only) > 0

    def as_dict(self) -> dict:
        return {"changed_pairs": self.changed_pairs, "ws_only_pairs": self.ws_only_pairs,
                "blank_only": self.blank_only, "inserted": self.inserted,
                "deleted": self.deleted, "wrap_only": self.wrap_only,
                "carries_change": self.carries_change, "samples": list(self.samples[:4])}


def line_stats(before: str | None, after: str | None) -> LineStats:
    """Compare two revisions line by line and name what moved."""
    if before is None or after is None:
        return LineStats()
    a_lines = [line.rstrip("\r") for line in before.splitlines()]
    b_lines = [line.rstrip("\r") for line in after.splitlines()]
    stats = LineStats(crlf_before=before.count("\r\n"), crlf_after=after.count("\r\n"))
    matcher = difflib.SequenceMatcher(a=a_lines, b=b_lines, autojunk=False)
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            continue
        if tag == "insert":
            stats.inserted += j2 - j1
            stats.blank_only += sum(1 for line in b_lines[j1:j2] if not line.strip())
            continue
        if tag == "delete":
            stats.deleted += i2 - i1
            stats.blank_only += sum(1 for line in a_lines[i1:i2] if not line.strip())
            continue
        left, right = a_lines[i1:i2], b_lines[j1:j2]
        if len(left) == len(right):
            for old, new in zip(left, right):
                stats.changed_pairs += 1
                if old.split() == new.split():
                    stats.ws_only_pairs += 1
                elif len(stats.samples) < 8:
                    stats.samples.append("- %s" % old.strip()[:70])
                    stats.samples.append("+ %s" % new.strip()[:70])
            continue
        if " ".join(" ".join(left).split()) == " ".join(" ".join(right).split()):
            stats.wrap_only = True
            stats.changed_pairs += len(left)
            stats.ws_only_pairs += len(left)
            continue
        stats.changed_pairs += max(len(left), len(right))
        if len(stats.samples) < 8:
            stats.samples.append("- %d line(s) became %d line(s)" % (len(left), len(right)))
    if (stats.inserted or stats.deleted) and not stats.carries_change:
        if " ".join(" ".join(a_lines).split()) == " ".join(" ".join(b_lines).split()):
            stats.wrap_only = True
    stats.same = before == after
    return stats


def changed_lines(before: str | None, after: str | None) -> tuple[list[str], list[str]]:
    """The removed and added lines of a pair, in order, blank-stripped.

    `generated.py` needs the *text* of what moved, not just its size: whether the diff is a
    stamp line, a re-order, or a hand edit is a question about the lines.
    """
    if before is None or after is None:
        return [], []
    removed: list[str] = []
    added: list[str] = []
    a_lines = before.splitlines()
    b_lines = after.splitlines()
    for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(a=a_lines, b=b_lines,
                                                       autojunk=False).get_opcodes():
        if tag in ("delete", "replace"):
            removed.extend(line.strip() for line in a_lines[i1:i2])
        if tag in ("insert", "replace"):
            added.extend(line.strip() for line in b_lines[j1:j2])
    return removed, added
