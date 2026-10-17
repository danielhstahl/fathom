"""What a test *is*, per language: named cases, their skip state, and their assertion census.

The ticket's phrase is "the testing contract, not coverage percentages", and a contract is
made of named things. So this module does not count lines: it reads a source file and
answers three questions a reviewer can check by name —

1. **Which test cases exist**, each with a name, a declaring line, a suite where suites
   exist, and a kind (`test`, `bench`, `fuzz`, `todo`);
2. **Which of them are disabled** — `#[ignore]`, `@pytest.mark.skip`, `t.Skip`, `it.skip`,
   `@Disabled` — with the marker that said so; and separately which are *focused*
   (`it.only`, `fit`), because a focused test silently disables every other test in the run;
3. **What each case asserts**, as a census of `(kind, strength, subject)` triples, where
   strength is `exact` (a value compared with an expected value), `presence` (it exists /
   is truthy / is a boolean / was called) or `message` (an assertion carrying only text,
   which can never be matched subject-to-subject).

That census is what makes a *weakened* assertion detectable rather than a vibe:
`assert_eq!(x.total, 42)` and `assert!(x.total > 0)` are both "an assertion", but only the
triples `("assert_eq", exact, "x.total")` versus `("assert", presence, "x.total")` say the
second is the first with the expected value thrown away.

| Language | Cases found by | Assertions found by | Disabled / focused |
| --- | --- | --- | --- |
| Rust | `#[test]`, `#[tokio::test]`, one case per `#[test_case(...)]`, in any `mod` — Rust tests usually live *inside* the file they test | `assert!`, `assert_eq!`, `assert_ne!`, `assert_matches!`, `debug_assert*` | `#[ignore]`; `#[should_panic]` kept as evidence |
| Python | `test_*` functions and `Test*`/`TestCase` methods, via the stdlib `ast`, so decorators are facts | `assert` by operator, `self.assert*`, `pytest.raises`, `pytest.approx` | `@pytest.mark.skip/skipif/xfail`, `@unittest.skip*`, `expectedFailure`, runtime `pytest.skip()`/`self.skipTest()`; a class-level skip disables every method in it |
| Go | `TestXxx`/`BenchmarkXxx`/`FuzzXxx`/`ExampleXxx` | `if got != want`, `reflect.DeepEqual`, `errors.Is`, testify/require matchers, `t.Errorf`/`t.Fatalf` as `message` | `t.Skip`/`t.Skipf`/`t.SkipNow`, `testing.Short()` guards; `t.Run` sub-tests recorded as evidence on the parent |
| TypeScript / JavaScript | `test`/`it`/`specify`/`bench`/`example` with `describe`/`context` suites — jest, vitest, playwright, mocha, node:test, tape | `expect(x).matcher`, chai chains, node `assert.*`, with a matcher strength table | `.skip`/`.todo`/`x`-prefixes; `.only`/`fit`/`fdescribe` are **focus**, a different finding |
| Java | `@Test`/`@ParameterizedTest` methods | JUnit `assert*`, AssertJ `assertThat(x).matcher`, Truth | `@Disabled`, `@Ignore` |
| anything else | *(no parser)* | *(no parser)* | — |

The last row is a feature. A language with no parser is not silently a file with zero tests:
`parse()` returns `state="unsupported"` with a reason, and `delta.py` degrades that episode
to a line-count proxy with a stated confidence instead of reporting "0 tests removed",
which would be a lie with a number on it.

Every parser here is pure: text in, structure out. No clock, no filesystem, no store.
"""

from __future__ import annotations

import ast
import re
from dataclasses import dataclass, field

from . import scan as S

#: Bumped with any change to case extraction, assertion kinds or strength classes. It is a
#: cache-key component for the store and the parity test the Rust port will run.
CASES_VERSION = 1

#: Assertion strengths. Two comparable ones plus `message`, which is explicitly *not*
#: comparable: an assertion with no subject cannot be matched to another one.
EXACT = "exact"
PRESENCE = "presence"
MESSAGE = "message"
STRENGTHS: tuple[str, ...] = (EXACT, PRESENCE, MESSAGE)

RUST = "rust"
PYTHON = "python"
GO = "go"
TS = "typescript"
JAVA = "java"

KIND_TEST = "test"
KIND_BENCH = "bench"
KIND_FUZZ = "fuzz"
KIND_TODO = "todo"
KIND_EXAMPLE = "example"
KINDS: tuple[str, ...] = (KIND_TEST, KIND_BENCH, KIND_FUZZ, KIND_TODO, KIND_EXAMPLE)

#: Parse outcomes. `empty` means the parser ran and found no case (a fixture module in a
#: `tests/` directory: a true answer), `unsupported` means no parser exists for the
#: language, `failed` means a parser exists and refused this file (unparseable Python).
STATE_PARSED = "parsed"
STATE_EMPTY = "empty"
STATE_UNSUPPORTED = "unsupported"
STATE_FAILED = "failed"
STATES: tuple[str, ...] = (STATE_PARSED, STATE_EMPTY, STATE_UNSUPPORTED, STATE_FAILED)


@dataclass(frozen=True)
class Assertion:
    """One assertion, reduced to what the weakening comparison needs."""

    line: int
    kind: str
    strength: str
    subject: str = ""
    text: str = ""

    @property
    def comparable(self) -> bool:
        """Whether this assertion can be matched against one in the other revision."""
        return bool(self.subject) and self.strength in (EXACT, PRESENCE)

    def key(self) -> str:
        return "%s|%s" % (self.strength, self.subject)

    def describe(self) -> str:
        return "%s:%d %s(%s)" % (self.kind, self.line, self.strength, self.subject or "—")

    def as_dict(self) -> dict:
        return {"line": self.line, "kind": self.kind, "strength": self.strength,
                "subject": self.subject, "text": self.text}


@dataclass(frozen=True)
class TestCase:
    """One named test case, with its location and its assertion census."""

    name: str
    language: str
    framework: str = ""
    line: int = 0
    end_line: int = 0
    kind: str = KIND_TEST
    suite: str = ""
    skipped: bool = False
    skip_kind: str = ""                  # ignore | skip | skipif | xfail | todo | runtime
    skip_reason: str = ""
    focused: bool = False
    variants: int = 1                    # parametrisation rows, when they are countable
    evidence: tuple[str, ...] = ()
    assertions: tuple[Assertion, ...] = ()

    @property
    def qualified(self) -> str:
        return "%s::%s" % (self.suite, self.name) if self.suite else self.name

    def key(self) -> str:
        """Identity across two revisions: the suite-qualified name, lower-cased.

        Deliberately *not* the line number — a test that moved is the same test, and
        reporting every moved test as removed-then-added would bury the one deletion the
        reviewer cares about.
        """
        return "%s/%s" % (self.language, self.qualified.lower())

    def location(self, path: str) -> str:
        return "%s:%d" % (path, self.line)

    @property
    def exact(self) -> int:
        return sum(1 for a in self.assertions if a.strength == EXACT)

    @property
    def presence(self) -> int:
        return sum(1 for a in self.assertions if a.strength == PRESENCE)

    def as_dict(self) -> dict:
        return {"name": self.qualified, "kind": self.kind, "line": self.line,
                "language": self.language, "framework": self.framework,
                "skipped": self.skipped, "skip_kind": self.skip_kind,
                "skip_reason": self.skip_reason, "focused": self.focused,
                "variants": self.variants, "assertions": len(self.assertions),
                "exact": self.exact, "presence": self.presence,
                "evidence": list(self.evidence),
                "assertion_list": [a.as_dict() for a in self.assertions]}


@dataclass
class ParsedFile:
    """One file's test surface, for one revision."""

    path: str
    language: str = ""
    parser: str = ""
    state: str = STATE_PARSED
    frameworks: tuple[str, ...] = ()
    cases: list[TestCase] = field(default_factory=list)
    #: 1-based inclusive line spans that hold test code inside a file the path taxonomy
    #: cannot call a test — Rust's `#[cfg(test)] mod tests`, above all.
    test_regions: list[tuple[int, int]] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    evidence: list[str] = field(default_factory=list)

    @property
    def has_cases(self) -> bool:
        return bool(self.cases)

    @property
    def parsed(self) -> bool:
        """Whether this revision's case names are known (as opposed to merely counted)."""
        return self.state in (STATE_PARSED, STATE_EMPTY)

    @property
    def unsupported(self) -> bool:
        return self.state in (STATE_UNSUPPORTED, STATE_FAILED)

    def case_keys(self) -> dict[str, TestCase]:
        out: dict[str, TestCase] = {}
        for case in self.cases:
            out.setdefault(case.key(), case)
        return out

    def counts(self) -> dict:
        return {"cases": len(self.cases),
                "skipped": sum(1 for case in self.cases if case.skipped),
                "focused": sum(1 for case in self.cases if case.focused),
                "assertions": sum(len(case.assertions) for case in self.cases),
                "exact": sum(case.exact for case in self.cases),
                "presence": sum(case.presence for case in self.cases),
                "by_kind": _by_kind(self.cases)}

    def as_dict(self) -> dict:
        return {"path": self.path, "language": self.language, "parser": self.parser,
                "state": self.state, "frameworks": list(self.frameworks),
                "counts": self.counts(),
                "test_regions": [list(span) for span in self.test_regions],
                "notes": list(self.notes), "evidence": list(self.evidence),
                "cases": [case.as_dict() for case in self.cases]}


# ---------------------------------------------------------------------------
# the language table
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Language:
    """One row of the parser table: what this parser can say about a file."""

    id: str
    title: str
    extensions: tuple[str, ...]
    parser: str
    syntax: S.Syntax
    frameworks: tuple[str, ...] = ()
    notes: str = ""


LANGUAGES: dict[str, Language] = {
    RUST: Language(RUST, "Rust", (".rs",), "rust", S.RUST_SYNTAX,
                   frameworks=("std::test", "tokio::test", "test-case"),
                   notes="tests live inside the file they test, under `#[cfg(test)] mod`, so "
                         "a source path can carry test lines and named cases"),
    PYTHON: Language(PYTHON, "Python", (".py", ".pyi"), "python(ast)", S.PY_SYNTAX,
                     frameworks=("pytest", "unittest", "hypothesis"),
                     notes="parsed with the stdlib `ast`, so decorators and nesting are "
                           "facts; a file that will not parse refuses rather than guessing"),
    GO: Language(GO, "Go", (".go",), "go", S.GO_SYNTAX,
                 frameworks=("testing", "testify"),
                 notes="`TestXxx`/`BenchmarkXxx`/`FuzzXxx`/`ExampleXxx`; `t.Run` sub-tests "
                       "are recorded as evidence on the parent case, not as separate cases"),
    TS: Language(TS, "TypeScript / JavaScript",
                 (".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"), "ts",
                 S.TS_SYNTAX,
                 frameworks=("jest", "vitest", "playwright", "mocha", "node:test", "tape"),
                 notes="jest/vitest/playwright/mocha/node:test/tape spellings of `it`/`test`, "
                       "plus chai chains and node `assert`"),
    JAVA: Language(JAVA, "Java", (".java",), "java", S.JAVA_SYNTAX,
                   frameworks=("junit5", "junit4", "assertj"),
                   notes="`@Test`/`@ParameterizedTest` methods with JUnit, AssertJ and Truth"),
}

EXTENSIONS: dict[str, str] = {}
for _lang in LANGUAGES.values():
    for _ext in _lang.extensions:
        EXTENSIONS.setdefault(_ext, _lang.id)


def language_for(path: str) -> str:
    """The language id for a path, `""` when there is no parser for it.

    The *longest* known extension wins, because `cart.test.ts` has two dots and the one
    that names the language is the last one.
    """
    stem = path.rsplit("/", 1)[-1]
    best = ""
    best_length = 0
    for extension, language in EXTENSIONS.items():
        if stem.endswith(extension) and len(extension) > best_length:
            best, best_length = language, len(extension)
    return best or EXTENSIONS.get(stem, "")


# ---------------------------------------------------------------------------
# the entry point
# ---------------------------------------------------------------------------
def parse(path: str, text: str | None) -> ParsedFile:
    """Read one revision of one file. Never raises: a refusal is a state, not an error."""
    language = language_for(path)
    if not language:
        return ParsedFile(path=path, language="", state=STATE_UNSUPPORTED,
                          notes=["no test-case parser for this file type"])
    if text is None:
        return ParsedFile(path=path, language=language, state=STATE_UNSUPPORTED,
                          notes=["this revision of the file is not available"])
    lang = LANGUAGES[language]
    masked = S.mask(text, lang.syntax).splitlines()
    raw = text.splitlines()
    try:
        if lang.parser == "python(ast)":
            return _parse_python(path, text, raw)
        if lang.parser == "rust":
            return _parse_rust(path, raw, masked)
        if lang.parser == "go":
            return _parse_go(path, raw, masked)
        if lang.parser == "ts":
            return _parse_ts(path, raw, masked)
        if lang.parser == "java":
            return _parse_java(path, raw, masked)
    except SyntaxError as exc:
        return ParsedFile(path=path, language=language, parser=lang.parser,
                          state=STATE_FAILED,
                          notes=["%s does not parse: %s" % (path, exc.msg)])
    except Exception as exc:                        # noqa: BLE001 — a refusal, never a crash
        return ParsedFile(path=path, language=language, parser=lang.parser,
                          state=STATE_FAILED,
                          notes=["%s parser refused this file: %s" % (language, exc)])
    return ParsedFile(path=path, language=language, state=STATE_UNSUPPORTED,
                      notes=["no parser is wired for %s" % language])


def describe_languages() -> list[str]:
    """The parser table as printed text (`fathom testsurface languages`)."""
    out: list[str] = ["test-case parser v%d — %d languages with a parser"
                      % (CASES_VERSION, len(LANGUAGES))]
    for lang in LANGUAGES.values():
        out.append("  %-16s %-13s %-34s %s"
                   % (lang.title, lang.parser, ", ".join(lang.frameworks),
                      ", ".join(lang.extensions)))
        if lang.notes:
            out.append("  %-16s %s" % ("", lang.notes))
    out.append("  anything else: no parser, so the episode degrades to a line-count proxy")
    return out


# ---------------------------------------------------------------------------
# scanner helpers
# ---------------------------------------------------------------------------
def _by_kind(cases) -> dict[str, int]:
    out: dict[str, int] = {}
    for case in cases:
        out[case.kind] = out.get(case.kind, 0) + 1
    return dict(sorted(out.items()))


def call_body(lines: list[str], index: int, position: int, max_lines: int = 10) -> str:
    """The text inside a call opened at `lines[index][position]` (just past its `(`).

    Brackets are kept (they are part of the subject a reviewer would recognise), and the
    scan joins downward until the brackets balance: a multi-line `assert_eq!(` is the same
    assertion as a one-line one and must produce the same subject key.
    """
    chunks: list[str] = []
    depth = 0
    for offset in range(max_lines):
        if index + offset >= len(lines):
            break
        line = lines[index + offset]
        start = position if offset == 0 else 0
        for char in line[start:]:
            if char in "([{":
                depth += 1
            elif char in ")]}":
                if depth == 0:
                    return "".join(chunks)
                depth -= 1
            chunks.append(char)
            if len(chunks) > 400:
                return "".join(chunks)
        chunks.append(" ")
        if depth <= 0 and chunks:
            break
    return "".join(chunks)


def split_arguments(text: str) -> list[str]:
    """Top-level arguments of a call body, whitespace-trimmed."""
    out: list[str] = []
    depth = 0
    current: list[str] = []
    for char in text:
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        if char == "," and depth <= 0:
            out.append("".join(current).strip())
            current = []
            continue
        current.append(char)
    if "".join(current).strip():
        out.append("".join(current).strip())
    return out


def _reason_from(marker: str) -> str:
    """The human reason inside `#[ignore = "flaky on windows"]` / `@Disabled("…")`."""
    match = re.search(r"[=(]\s*[\"']?([^\"')\]]{2,})", marker)
    return match.group(1).strip() if match else ""


def _tail(lines: list[str], index: int, position: int, span: int = 3) -> str:
    """Text following a position, across up to `span` lines — for `.not.toEqual` chains."""
    chunks = [lines[index][position:]] if index < len(lines) else []
    for offset in range(1, span):
        if index + offset >= len(lines):
            break
        chunks.append(lines[index + offset])
    return " ".join(chunks)


# ---------------------------------------------------------------------------
# Rust
# ---------------------------------------------------------------------------
RUST_ATTR = re.compile(r"^#\[")
RUST_TEST_ATTR = re.compile(r"^#\[\s*(?:test\b|tokio::test\b|async_std::test\b|test_case\b|"
                            r"rstest\b|case\b|golden\b)")
RUST_CASE_ATTR = re.compile(r"^#\[\s*(?:test_case|rstest|case)\(")
RUST_IGNORE = re.compile(r"^#\[\s*ignore\b")
RUST_PANIC = re.compile(r"^#\[\s*should_panic\b")
RUST_FN = re.compile(r"^(?:(?:pub|default|const|async|unsafe|extern)\b[\w\"()]*\s+)*"
                     r"fn\s+(?P<name>[A-Za-z_]\w*)")
RUST_MOD = re.compile(r"^(?:pub\s+)?(?:unsafe\s+)?mod\s+(?P<name>[A-Za-z_]\w*)\b")
RUST_ASSERT = re.compile(r"\b(?P<kind>debug_assert_matches|debug_assert_eq|debug_assert_ne|"
                         r"debug_assert|assert_matches|assert_eq|assert_ne|assert)!\s*\(")
RUST_PRESENCE = re.compile(r"\b(is_some|is_ok|is_err|is_some_and|is_none|is_none_or|contains|"
                           r"starts_with|ends_with|matches|any|all|is_empty)\s*[(!]")
RUST_COMPARE = re.compile(r"(==|!=|<=|>=|<|>)")


def _parse_rust(path: str, raw: list[str], masked: list[str]) -> ParsedFile:
    parsed = ParsedFile(path=path, language=RUST, parser="rust")
    attrs: list[str] = []
    attr_start = 0
    stack: list[dict] = []
    depth = 0
    index = 0
    frameworks: list[str] = []
    while index < len(masked):
        line = masked[index]
        stripped = line.strip()
        delta = S.brace_delta(line)
        if not stripped:
            depth += delta
            index += 1
            continue
        if RUST_ATTR.match(stripped):
            if not attrs:
                attr_start = index
            # the *masked* line says this really is an attribute and not a doc comment
            # quoting one; the raw line is where a human reason for `#[ignore]` still lives
            attrs.append(raw[index].strip() if index < len(raw) else stripped)
            depth += delta
            index += 1
            continue
        mod = RUST_MOD.match(stripped)
        if mod and "{" in stripped:
            is_test = any("cfg(test)" in attr.replace(" ", "") for attr in attrs)
            stack.append({"kind": "mod", "name": mod.group("name"), "depth": depth,
                          "start": (attr_start if is_test and attrs else index) + 1,
                          "is_test": is_test})
            if is_test:
                parsed.evidence.append(
                    "#[cfg(test)] mod %s at %s:%d is test code inside a file the path "
                    "taxonomy may not call a test" % (mod.group("name"), path,
                                                     stack[-1]["start"]))
            attrs = []
            depth += delta
            index += 1
            _close_blocks(parsed, stack, depth, index)
            continue
        fn = RUST_FN.match(stripped)
        if fn and attrs:
            test_attrs = [attr for attr in attrs if RUST_TEST_ATTR.match(attr)]
            ignored = [attr for attr in attrs if RUST_IGNORE.match(attr)]
            panics = [attr for attr in attrs if RUST_PANIC.match(attr)]
            if test_attrs:
                end = S.block_end(masked, index)
                suite = "::".join(item["name"] for item in stack if item["kind"] == "mod")
                groups = [attr for attr in attrs if RUST_CASE_ATTR.match(attr)]
                variants = max(1, len(groups))
                assertions = rust_assertions(masked, raw, index, end)
                framework = _rust_framework(test_attrs)
                evidence = (["#[should_panic]"] if panics else []) \
                    + (["#[%s] group of %d cases" % (groups[0][2:].split("(")[0].strip(),
                                                     variants)] if groups else [])
                for offset in range(variants):
                    label = fn.group("name")
                    if variants > 1:
                        label = "%s#%d" % (label, offset + 1)
                        evidence = evidence + ["one of %d generated cases" % variants]
                    parsed.cases.append(TestCase(
                        name=label, language=RUST, framework=framework, line=index + 1,
                        end_line=end + 1, suite=suite,
                        skipped=bool(ignored), skip_kind="ignore" if ignored else "",
                        skip_reason=_reason_from(ignored[0]) if ignored else "",
                        variants=variants, evidence=tuple(evidence), assertions=assertions))
                frameworks.append(framework)
            attrs = []
            depth += delta
            index += 1
            _close_blocks(parsed, stack, depth, index)
            continue
        attrs = []
        depth += delta
        index += 1
        _close_blocks(parsed, stack, depth, index)
    while stack:
        _close_blocks(parsed, stack, -1, len(masked) - 1)
    parsed.frameworks = tuple(dict.fromkeys(frameworks))
    parsed.state = STATE_PARSED if parsed.cases else STATE_EMPTY
    if not parsed.cases:
        parsed.notes = ["no `#[test]` attribute in this file"]
    return parsed


def _close_blocks(parsed: ParsedFile, stack: list[dict], depth: int, index: int) -> None:
    """Pop closed `mod` scopes and record the extent of a `#[cfg(test)]` module."""
    while stack and depth <= stack[-1]["depth"]:
        top = stack.pop()
        if top["is_test"]:
            parsed.test_regions.append((top["start"], index + 1))


def _rust_framework(attrs) -> str:
    joined = " ".join(attrs)
    if "tokio::test" in joined:
        return "tokio::test"
    if "test_case" in joined or "rstest" in joined:
        return "test-case"
    if "golden" in joined:
        return "expect-test"
    return "std::test"


def rust_assertions(masked: list[str], raw: list[str], start: int, end: int
                    ) -> tuple[Assertion, ...]:
    """Every Rust assertion in a line span, with strength and subject."""
    out: list[Assertion] = []
    for index in range(start, min(end + 1, len(masked))):
        line = masked[index]
        text = raw[index].strip() if index < len(raw) else line.strip()
        for match in RUST_ASSERT.finditer(line):
            kind = match.group("kind")
            body = call_body(masked, index, match.end())
            if kind in ("assert", "debug_assert"):
                comparison = RUST_COMPARE.search(body)
                if comparison:
                    out.append(Assertion(line=index + 1, kind=kind, strength=EXACT,
                                         subject=S.subject(body[:comparison.start()]),
                                         text=text))
                elif RUST_PRESENCE.search(body):
                    head = RUST_PRESENCE.split(body)[0]
                    out.append(Assertion(line=index + 1, kind=kind, strength=PRESENCE,
                                         subject=S.subject(head), text=text))
                else:
                    out.append(Assertion(line=index + 1, kind=kind, strength=PRESENCE,
                                         subject=S.subject(body), text=text))
            else:
                args = split_arguments(body)
                out.append(Assertion(line=index + 1, kind=kind, strength=EXACT,
                                     subject=S.subject(args[0] if args else body), text=text))
    return tuple(out)


# ---------------------------------------------------------------------------
# Python — the stdlib AST, so decorators are facts and not guesses
# ---------------------------------------------------------------------------
PY_EXACT_CALLS = {
    "assertEqual", "assertNotEqual", "assertAlmostEqual", "assertNotAlmostEqual",
    "assertIs", "assertIsNot", "assertIn", "assertNotIn", "assertListEqual",
    "assertTupleEqual", "assertSetEqual", "assertDictEqual", "assertCountEqual",
    "assertSequenceEqual", "assertRaises", "assertRaisesRegex", "assertWarns",
    "assertLogs", "assert_close", "approx", "array_equal", "assert_allclose",
    "assert_array_equal", "assert_called_with", "assert_called_once_with",
}
PY_PRESENCE_CALLS = {
    "assertTrue", "assertFalse", "assertIsNone", "assertIsNotNone", "assertEmpty",
    "assertNotEmpty", "assert_called", "assert_called_once", "assert_any_call",
}
PY_SKIP_DECORATORS = {"skip": "skip", "skipif": "skipif", "xfail": "xfail",
                      "skipTest": "skip", "skipIf": "skipif", "skipUnless": "skipif",
                      "expectedFailure": "xfail"}
PY_RUNTIME_SKIP = {"skip": "skip", "skipTest": "skip", "xfail": "xfail"}


def _parse_python(path: str, text: str, raw: list[str]) -> ParsedFile:
    parsed = ParsedFile(path=path, language=PYTHON, parser="python(ast)")
    tree = ast.parse(text)                            # raises → caller records the refusal
    frameworks: list[str] = []
    imports = _python_imports(tree)
    for name in ("pytest", "unittest", "hypothesis"):
        if name in imports:
            frameworks.append(name)

    def case_of(node, suite: str, inherited: tuple[str, str]) -> TestCase:
        decorators = _python_decorators(node)
        skip_kind, skip_reason = inherited
        for marker in decorators:
            if marker[0] in PY_SKIP_DECORATORS:
                skip_kind = PY_SKIP_DECORATORS[marker[0]]
                skip_reason = marker[1] or marker[0]
        if not skip_kind:
            skip_kind, skip_reason = _python_runtime_skip(node)
        evidence = ["@%s" % marker[0] for marker in decorators]
        variants = _python_parametrize(node)
        if variants > 1:
            evidence.append("@parametrize ×%d cases" % variants)
        kind = KIND_TEST
        if any(marker[0] in ("benchmark", "bench") for marker in decorators):
            kind = KIND_BENCH
        if any(marker[0] == "example" for marker in decorators):
            kind = KIND_EXAMPLE
        return TestCase(name=node.name, language=PYTHON,
                        framework=("hypothesis" if any(m[0] == "given" for m in decorators)
                                   else ("unittest" if suite else "pytest")),
                        line=node.lineno, end_line=getattr(node, "end_lineno", node.lineno),
                        suite=suite, kind=kind, skipped=bool(skip_kind),
                        skip_kind=skip_kind, skip_reason=skip_reason, variants=variants,
                        evidence=tuple(evidence), assertions=_python_assertions(node))

    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) \
                and node.name.startswith("test"):
            parsed.cases.append(case_of(node, "", ("", "")))
        elif isinstance(node, ast.ClassDef):
            class_skip: tuple[str, str] = ("", "")
            for marker in _python_decorators(node):
                if marker[0] in PY_SKIP_DECORATORS:
                    class_skip = (PY_SKIP_DECORATORS[marker[0]], marker[1] or marker[0])
            if class_skip[0]:
                parsed.evidence.append(
                    "class %s is disabled by @%s%s — every test method in it stops running"
                    % (node.name, class_skip[0],
                       " (%s)" % class_skip[1] if class_skip[1] else ""))
            if not (node.name.startswith("Test") or _inherits_test_case(node)):
                continue
            for child in node.body:
                if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)) \
                        and child.name.startswith("test"):
                    parsed.cases.append(case_of(child, node.name, class_skip))
    parsed.frameworks = tuple(dict.fromkeys(frameworks)) or ("pytest",)
    parsed.state = STATE_PARSED if parsed.cases else STATE_EMPTY
    if not parsed.cases:
        parsed.notes = ["no `test_*` function or `Test*` method in this file"]
    return parsed


def _python_imports(tree: ast.Module) -> set[str]:
    out: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            out.update(alias.name.split(".")[0] for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module:
            out.add(node.module.split(".")[0])
    return out


def _inherits_test_case(node: ast.ClassDef) -> bool:
    return any("TestCase" in ast.unparse(base) or "TestSuite" in ast.unparse(base)
               for base in node.bases)


def _python_decorators(node) -> list[tuple[str, str]]:
    """`[(leaf_name, reason)]` for every decorator; `@a.b(...)` and `@a.b` both."""
    out: list[tuple[str, str]] = []
    for dec in getattr(node, "decorator_list", []):
        call = dec.func if isinstance(dec, ast.Call) else dec
        leaf = ast.unparse(call).rsplit(".", 1)[-1]
        reason = ""
        if isinstance(dec, ast.Call):
            for arg in dec.args:
                if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                    reason = arg.value
                    break
            if not reason:
                for keyword in dec.keywords:
                    if keyword.arg in ("reason", "raises", "condition") \
                            and isinstance(keyword.value, ast.Constant):
                        reason = str(keyword.value.value)
                        break
        out.append((leaf, reason))
    return out


def _python_parametrize(node) -> int:
    for dec in getattr(node, "decorator_list", []):
        call = dec.func if isinstance(dec, ast.Call) else dec
        if ast.unparse(call).rsplit(".", 1)[-1] != "parametrize":
            continue
        if isinstance(dec, ast.Call) and len(dec.args) >= 2:
            rows = dec.args[1]
            if isinstance(rows, (ast.List, ast.Tuple, ast.Set)):
                return max(1, len(rows.elts))
    return 1


def _python_assertions(node) -> tuple[Assertion, ...]:
    out: list[Assertion] = []
    for child in ast.walk(node):
        if isinstance(child, ast.Assert):
            test = child.test
            text = ast.unparse(child)[:120]
            if isinstance(test, ast.Compare) and not isinstance(test.ops[0], (ast.Is, ast.IsNot)):
                out.append(Assertion(line=child.lineno, kind="assert", strength=EXACT,
                                     subject=S.subject(ast.unparse(test.left)), text=text))
            elif isinstance(test, ast.Call):
                callee = ast.unparse(test.func)
                leaf = callee.rsplit(".", 1)[-1]
                if leaf in ("isinstance", "issubclass", "hasattr") and test.args:
                    out.append(Assertion(line=child.lineno, kind=leaf, strength=PRESENCE,
                                         subject=S.subject(ast.unparse(test.args[0])),
                                         text=text))
                else:
                    out.append(Assertion(line=child.lineno, kind="assert",
                                         strength=PRESENCE,
                                         subject=S.subject(ast.unparse(test)), text=text))
            else:
                out.append(Assertion(line=child.lineno, kind="assert", strength=PRESENCE,
                                     subject=S.subject(ast.unparse(test)), text=text))
            continue
        if isinstance(child, ast.Call):
            leaf = ast.unparse(child.func).rsplit(".", 1)[-1]
            if leaf in PY_SKIP_DECORATORS or leaf in ("fail", "skip", "xfail"):
                continue
            strength = ""
            if leaf in PY_EXACT_CALLS:
                strength = EXACT
            elif leaf in PY_PRESENCE_CALLS:
                strength = PRESENCE
            if not strength:
                continue
            args = [ast.unparse(arg) for arg in child.args]
            out.append(Assertion(line=child.lineno, kind=leaf, strength=strength,
                                 subject=S.subject(args[0] if args else ""),
                                 text=ast.unparse(child)[:120]))
    return tuple(sorted(out, key=lambda a: (a.line, a.kind, a.subject)))


def _python_runtime_skip(node) -> tuple[str, str]:
    for child in ast.walk(node):
        if not isinstance(child, ast.Call):
            continue
        leaf = ast.unparse(child.func).rsplit(".", 1)[-1]
        if leaf in PY_RUNTIME_SKIP:
            reason = ""
            for arg in child.args:
                if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                    reason = arg.value
                    break
            return PY_RUNTIME_SKIP[leaf], reason
    return ("", "")


# ---------------------------------------------------------------------------
# Go
# ---------------------------------------------------------------------------
GO_FUNC = re.compile(r"^func\s+(?P<name>[A-Z]\w*)\s*\(")
_GO_CASE_NAME = re.compile(r"^(?:Test|Benchmark|Fuzz|Example)(?:[A-Z_0-9]|$)")
GO_SUBTEST = re.compile(r"\bt\.Run\s*\(\s*\"(?P<name>[^\"]*)\"")
GO_SKIP = re.compile(r"\bt\.Skip(?P<form>f|Now)?\s*\(")
GO_SHORT = re.compile(r"testing\.Short\s*\(")
GO_COMPARE = re.compile(r"\b(?P<lhs>[\w.\[\]*()]+)\s*(?P<op>!=|==)\s*(?P<rhs>[\w.\[\]*()\"]+)")
GO_CALL = re.compile(r"\b(?P<lib>assert|require)\.(?P<matcher>[A-Z]\w*)\s*\(")
GO_MSG = re.compile(r"\bt\.(?P<kind>Errorf?|Fatalf?|Fatal|Error)\s*\(")
GO_EXACT_MATCHERS = {"Equal", "NotEqual", "Less", "Greater", "LessOrEqual",
                     "GreaterOrEqual", "ElementsMatch", "Contains", "NotContains", "Subset",
                     "Len", "JSONEq", "Regexp", "ErrorContains", "Exactly", "EqualValues"}
GO_PRESENCE_MATCHERS = {"True", "False", "Nil", "NotNil", "Empty", "NotEmpty", "Error",
                        "NoError", "Panics", "NotPanics", "Zero", "NotZero", "DirExists",
                        "FileExists", "NoDirExists", "NoFileExists"}


def _parse_go(path: str, raw: list[str], masked: list[str]) -> ParsedFile:
    parsed = ParsedFile(path=path, language=GO, parser="go")
    index = 0
    while index < len(masked):
        line = masked[index]
        if not line.strip():
            index += 1
            continue
        match = GO_FUNC.match(line.strip())
        if match and _GO_CASE_NAME.match(match.group("name")):
            name = match.group("name")
            kind = _go_kind(name)
            end = S.block_end(masked, index)
            body = "\n".join(masked[index:end + 1])
            raw_body = "\n".join(raw[index:end + 1])
            evidence: list[str] = []
            subtests = GO_SUBTEST.findall(body)
            if subtests:
                evidence.append("%d t.Run sub-test(s): %s"
                                % (len(subtests), ", ".join('"%s"' % s for s in subtests[:4])
                                   + (" …" if len(subtests) > 4 else "")))
            skipped = bool(GO_SKIP.search(body))
            if skipped and GO_SHORT.search(body):
                evidence.append("skipped only under testing.Short()")
            parsed.cases.append(TestCase(
                name=name, language=GO, framework=_go_framework(body), line=index + 1,
                end_line=end + 1, kind=kind, skipped=skipped,
                skip_kind="skip" if skipped else "",
                skip_reason="t.Skip" if skipped else "",
                evidence=tuple(evidence),
                assertions=go_assertions(masked, raw, index, end)))
            index = end + 1
            continue
        index += 1
    if parsed.cases:
        parsed.frameworks = tuple(sorted({case.framework for case in parsed.cases
                                          if case.framework})) or ("testing",)
    parsed.state = STATE_PARSED if parsed.cases else STATE_EMPTY
    if not parsed.cases:
        parsed.notes = ["no exported TestXxx/BenchmarkXxx/FuzzXxx function in this file"]
    return parsed


def _go_kind(name: str) -> str:
    if name.startswith("Benchmark"):
        return KIND_BENCH
    if name.startswith("Fuzz"):
        return KIND_FUZZ
    if name.startswith("Example"):
        return KIND_EXAMPLE
    return KIND_TEST


def _go_framework(body: str) -> str:
    return "testify" if re.search(r"\b(assert|require)\.[A-Z]", body) else "testing"


def go_assertions(masked: list[str], raw: list[str], start: int, end: int) -> tuple[Assertion, ...]:
    """Go assertions: comparison guards, testify matchers, and message-only failures."""
    out: list[Assertion] = []
    for index in range(start, min(end + 1, len(masked))):
        line = masked[index]
        text = raw[index].strip() if index < len(raw) else line.strip()
        stripped = line.strip()
        for match in GO_CALL.finditer(line):
            matcher = match.group("matcher")
            body = call_body(masked, index, match.end())
            args = split_arguments(body)
            # testify passes `t` first; the subject is the value under test
            value = args[1] if len(args) > 1 else (args[0] if args else "")
            strength = EXACT if matcher in GO_EXACT_MATCHERS else (
                PRESENCE if matcher in GO_PRESENCE_MATCHERS else "")
            if strength:
                out.append(Assertion(line=index + 1, kind="%s.%s" % (match.group("lib"), matcher),
                                    strength=strength, subject=S.subject(value), text=text))
        for pattern in ("reflect.DeepEqual", "errors.Is", "errors.As", "cmp.Diff",
                       "cmp.Equal"):
            leaf = pattern.rsplit(".", 1)[-1]
            for match in re.finditer(r"\b%s\s*\(" % re.escape(pattern), line):
                args = split_arguments(call_body(masked, index, match.end()))
                subject = S.subject(args[0] if args else "")
                strength = PRESENCE if leaf == "Diff" else EXACT
                out.append(Assertion(line=index + 1, kind=pattern, strength=strength,
                                     subject=subject, text=text))
        if stripped.startswith("if "):
            compare = GO_COMPARE.search(stripped)
            if compare:
                lhs, rhs, op = compare.group("lhs"), compare.group("rhs"), compare.group("op")
                if rhs == "nil" or lhs == "nil":
                    if "err" in lhs.lower() or "err" in rhs.lower():
                        out.append(Assertion(line=index + 1, kind="errcheck", strength=PRESENCE,
                                             subject=S.subject(lhs if "err" in lhs.lower()
                                                               else rhs), text=text))
                    else:
                        out.append(Assertion(line=index + 1, kind="nil-check", strength=PRESENCE,
                                             subject=S.subject(lhs if rhs == "nil" else rhs),
                                             text=text))
                elif op == "!=" or op == "==":
                    out.append(Assertion(line=index + 1, kind="compare", strength=EXACT,
                                         subject=S.subject(lhs), text=text))
        for match in GO_MSG.finditer(line):
            out.append(Assertion(line=index + 1, kind="t.%s" % match.group("kind"),
                                 strength=MESSAGE, subject="", text=text))
    return tuple(sorted(out, key=lambda a: (a.line, a.kind, a.subject)))


# ---------------------------------------------------------------------------
# TypeScript / JavaScript
# ---------------------------------------------------------------------------
TS_CALL = re.compile(r"\b(?P<fn>describe|context|suite|test|it|specify|bench|benchmark|"
                     r"example|fit|fdescribe|xtest|xit)(?P<hooks>(?:\.(?:skip|only|todo|each|"
                     r"concurrent|serial|fails|failing|serialOnly))*)\s*\(")
TS_NAME = re.compile(r"\s*[`\"'](?P<name>[^`\"']*)[`\"']")
TS_EXPECT = re.compile(r"\bexpect(?:\.soft)?(?:\.assert)?\s*\(")
TS_ASSERT_CALL = re.compile(r"\bassert(?P<path>(?:\.[A-Za-z]\w*)*)\s*\(")
TS_MATCHERS = {
    # jest / vitest / playwright
    "toBe": EXACT, "toEqual": EXACT, "toStrictEqual": EXACT, "toMatch": EXACT,
    "toMatchObject": EXACT, "toMatchSnapshot": EXACT, "toMatchInlineSnapshot": EXACT,
    "toContain": EXACT, "toContainEqual": EXACT, "toHaveBeenCalledWith": EXACT,
    "toHaveBeenNthCalledWith": EXACT, "toHaveBeenCalledTimes": EXACT, "toThrow": EXACT,
    "toThrowError": EXACT, "toBeGreaterThan": EXACT, "toBeGreaterThanOrEqual": EXACT,
    "toBeLessThan": EXACT, "toBeLessThanOrEqual": EXACT, "toBeCloseTo": EXACT,
    "toHaveLength": EXACT, "toHaveProperty": EXACT, "toHaveBeenLastCalledWith": EXACT,
    "toSatisfy": EXACT, "toRespond": EXACT, "toHaveTitle": EXACT, "toHaveURL": EXACT,
    "toBeVisible": PRESENCE, "toBeEnabled": PRESENCE, "toBeChecked": PRESENCE,
    "toBeDefined": PRESENCE, "toBeUndefined": PRESENCE, "toBeNull": PRESENCE,
    "toBeTruthy": PRESENCE, "toBeFalsy": PRESENCE, "toBeEmpty": PRESENCE,
    "toBeEmptyDOMElement": PRESENCE, "toHaveBeenCalled": PRESENCE, "toBeNaN": PRESENCE,
    "toBeInstanceOf": PRESENCE, "toExist": PRESENCE, "exist": PRESENCE, "exists": PRESENCE,
    # chai chains
    "equal": EXACT, "equals": EXACT, "eql": EXACT, "deep": EXACT, "include": EXACT,
    "includes": EXACT, "contain": EXACT, "members": EXACT, "throw": EXACT, "throws": EXACT,
    "rejects": EXACT, "match": EXACT, "oneOf": EXACT, "lengthOf": EXACT, "property": EXACT,
    "above": EXACT, "below": EXACT, "closeTo": EXACT, "instanceOf": PRESENCE, "ok": PRESENCE,
    "true": PRESENCE, "false": PRESENCE, "null": PRESENCE, "undefined": PRESENCE,
    "empty": PRESENCE, "a": PRESENCE, "an": PRESENCE, "finite": PRESENCE,
    # node:assert
    "strictEqual": EXACT, "deepStrictEqual": EXACT, "notStrictEqual": EXACT,
    "notDeepEqual": EXACT, "notEqual": EXACT, "doesNotThrow": EXACT, "ifError": PRESENCE,
    "ok": PRESENCE, "strict": EXACT, "fail": MESSAGE,
}
TS_PRESENCE_ASSERTS = {"ok", "ifError"}


def _parse_ts(path: str, raw: list[str], masked: list[str]) -> ParsedFile:
    parsed = ParsedFile(path=path, language=TS, parser="ts")
    stack: list[dict] = []
    depth = 0
    index = 0
    frameworks: list[str] = []
    while index < len(masked):
        line = masked[index]
        stripped = line.strip()
        delta = S.brace_delta(line)
        if not stripped:
            depth += delta
            index += 1
            continue
        match = TS_CALL.search(line)
        if match and not stripped.startswith("//"):
            hook = match.group("hooks") or ""
            name_start = match.end()
            label = ""
            # the case name lives in a string literal, which masking deliberately blanks
            # out, so names and `.each` rows are read from the raw text at the same offsets
            search = "".join(raw[index:index + 3])
            tail = search[name_start:] if name_start <= len(search) else ""
            named = TS_NAME.match(tail)
            variants = 1
            if ".each" in hook:
                rows = call_body(masked, index, name_start)
                variants = max(1, len(re.findall(r"\],", rows)) + 1)
                after = _after_each(raw, index, name_start)
                named = TS_NAME.match(after)
            if named:
                label = named.group("name")
            end = S.block_end(masked, index)
            is_suite = match.group("fn") in ("describe", "context", "suite", "fdescribe")
            if is_suite:
                stack.append({"name": label or "(anonymous suite)", "depth": depth,
                              "start": index + 1})
                attrs = []
                depth += delta
                index += 1
                while stack and depth <= stack[-1]["depth"]:
                    stack.pop()
                continue
            kind = KIND_TEST
            if match.group("fn") in ("bench", "benchmark"):
                kind = KIND_BENCH
            if ".todo" in hook:
                kind = KIND_TODO
            skipped = (".skip" in hook or ".todo" in hook
                       or match.group("fn") in ("xtest", "xit"))
            focused = ".only" in hook or match.group("fn") in ("fit", "fdescribe")
            suite = "::".join(item["name"] for item in stack)
            framework = _ts_framework(raw[index] if index < len(raw) else "",
                                      "\n".join(raw[index:end + 1]))
            frameworks.append(framework)
            evidence = [hook.strip(".") or match.group("fn")]
            if variants > 1:
                evidence.append(".each over %d rows" % variants)
            skip_reason = ""
            if skipped:
                skip_reason = ("todo" if ".todo" in hook else
                               "x-prefix" if match.group("fn") in ("xtest", "xit")
                               else hook.strip(".") or match.group("fn"))
            parsed.cases.append(TestCase(
                name=label or "%s:%d" % (match.group("fn"), index + 1), language=TS,
                framework=framework, line=index + 1, end_line=end + 1, kind=kind, suite=suite,
                skipped=skipped, skip_kind="skip" if skipped else "",
                skip_reason=skip_reason,
                focused=focused,
                variants=variants, evidence=tuple(evidence),
                assertions=ts_assertions(masked, raw, index, end)))
            depth += delta
            index += 1
            while stack and depth <= stack[-1]["depth"]:
                stack.pop()
            continue
        depth += delta
        while stack and depth <= stack[-1]["depth"]:
            stack.pop()
        index += 1
    text = "\n".join(raw)                       # module specifiers are string literals,
    declared = _ts_imports(text)                # so import detection reads the raw text
    parsed.frameworks = tuple(dict.fromkeys(declared + frameworks)) or ("jest",)
    parsed.state = STATE_PARSED if parsed.cases else STATE_EMPTY
    if not parsed.cases:
        parsed.notes = ["no `test(...)`/`it(...)` call in this file"]
    return parsed


def _after_each(lines: list[str], index: int, position: int) -> str:
    """The text after `.each([...])`'s closing bracket, where the case name lives."""
    joined = "".join(lines[index:index + 8])[position:]
    depth = 1
    for offset, char in enumerate(joined):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
            if depth == 0:
                rest = joined[offset + 1:]
                paren = rest.find("(")
                return rest[paren + 1:] if paren >= 0 else ""
    return ""


def _ts_imports(text: str) -> list[str]:
    out: list[str] = []
    for module, name in (("@playwright/test", "playwright"), ("vitest", "vitest"),
                         ("@jest/globals", "jest"), ("jest", "jest"),
                         ("node:test", "node:test"), ("node:assert", "node:assert"),
                         ("mocha", "mocha"), ("tape", "tape"), ("bun:test", "bun:test"),
                         ("chai", "chai"), ("@bun/test", "bun:test")):
        if re.search(r"[\"']%s[\"']" % re.escape(module), text):
            out.append(name)
    return out


def _ts_framework(header: str, body: str) -> str:
    for module, name in (("playwright", "playwright"), ("vitest", "vitest"),
                         ("jest", "jest"), ("mocha", "mocha"), ("tape", "tape")):
        if module in header or module in body:
            return name
    return "jest"


def ts_assertions(masked: list[str], raw: list[str], start: int, end: int) -> tuple[Assertion, ...]:
    """`expect(x).matcher(y)`, chai chains and node `assert` calls, with matcher strengths."""
    out: list[Assertion] = []
    for index in range(start, min(end + 1, len(masked))):
        line = masked[index]
        text = raw[index].strip() if index < len(raw) else line.strip()
        for match in TS_EXPECT.finditer(line):
            subject_text = call_body(masked, index, match.end())
            rest = _tail(masked, index, match.end() + len(subject_text) + 1, span=2)
            matcher = _ts_matcher(rest)
            if not matcher:
                continue
            name, strength = matcher
            out.append(Assertion(line=index + 1, kind="expect.%s" % name, strength=strength,
                                 subject=S.subject(subject_text), text=text))
        for match in TS_ASSERT_CALL.finditer(line):
            leaf = (match.group("path") or "").lstrip(".")
            subject_text = call_body(masked, index, match.end())
            args = split_arguments(subject_text)
            head = args[0] if args else ""
            if not leaf:
                out.append(Assertion(line=index + 1, kind="assert", strength=PRESENCE,
                                     subject=S.subject(head), text=text))
                continue
            if leaf in ("equal", "strictEqual", "deepEqual", "deepStrictEqual", "notEqual",
                        "notStrictEqual", "notDeepEqual", "throws", "rejects", "match",
                        "doesNotThrow"):
                strength = PRESENCE if leaf == "doesNotThrow" else EXACT
                out.append(Assertion(line=index + 1, kind="assert.%s" % leaf, strength=strength,
                                     subject=S.subject(head), text=text))
            elif leaf in TS_PRESENCE_ASSERTS:
                out.append(Assertion(line=index + 1, kind="assert.%s" % leaf, strength=PRESENCE,
                                     subject=S.subject(head), text=text))
            elif leaf in ("fail",):
                out.append(Assertion(line=index + 1, kind="assert.fail", strength=MESSAGE,
                                     subject="", text=text))
    return tuple(sorted(out, key=lambda a: (a.line, a.kind, a.subject)))


def _ts_matcher(text: str) -> tuple[str, str] | None:
    """The first known matcher in a chain, and its strength."""
    for match in re.finditer(r"\.(?P<name>[A-Za-z]\w*)", text):
        name = match.group("name")
        if name in TS_MATCHERS:
            return name, TS_MATCHERS[name]
    return None


# ---------------------------------------------------------------------------
# Java
# ---------------------------------------------------------------------------
JAVA_ANNOT = re.compile(r"^@(?P<name>\w+)(?P<args>\([^)]*\))?")
JAVA_METHOD = re.compile(r"^(?:(?:public|private|protected|static|final|abstract|synchronized|"
                         r"default|native)\s+)*(?P<type>[\w<>,\[\]?]+)\s+(?P<name>\w+)\s*\(")
JAVA_CLASS = re.compile(r"^(?:(?:public|private|protected|static|final|abstract|sealed|open)\s+)*"
                        r"(?:class|interface|enum|record)\s+(?P<name>\w+)")
JAVA_ASSERT = re.compile(r"\bassert(?P<suffix>That\w*|\w*)\s*\(")
JAVA_ASSERTJ = re.compile(r"\.(?P<name>isEqualTo|isNotEqualTo|isSameAs|contains|containsExactly"
                          r"|containsExactlyElementsOf|startsWith|endsWith|matches|isGreaterThan"
                          r"|isLessThan|isIn|containsOnlyKeys|hasSize|isEqualToIgnoringCase"
                          r"|isEqualToComparingFieldByField|isInstanceOf|isNull|isNotNull|isTrue"
                          r"|isFalse|isPresent|isEmpty|isNotEmpty|hasMessage|failsBecause|"
                          r"succeeds|isBlank)\b")
JAVA_ASSERTJ_EXACT = {"isEqualTo", "isNotEqualTo", "isSameAs", "contains", "containsExactly",
                      "containsExactlyElementsOf", "startsWith", "endsWith", "matches",
                      "isGreaterThan", "isLessThan", "isIn", "containsOnlyKeys", "hasSize",
                      "isEqualToIgnoringCase", "isEqualToComparingFieldByField", "hasMessage"}
JAVA_ASSERT_METHODS = {
    "Equals": EXACT, "NotEquals": EXACT, "Same": EXACT, "NotSame": EXACT,
    "Nullable": "", "True": PRESENCE, "False": PRESENCE, "Null": PRESENCE,
    "NotNull": PRESENCE, "ArrayEquals": EXACT, "DeepEquals": EXACT,
    "LinesMatch": EXACT, "IterableEquals": EXACT, "Throws": EXACT, "ThrowsExactly": EXACT,
    "Timeout": "", "Fail": MESSAGE, "That": EXACT,
}


def _parse_java(path: str, raw: list[str], masked: list[str]) -> ParsedFile:
    parsed = ParsedFile(path=path, language=JAVA, parser="java")
    annotations: list[str] = []
    klass = ""
    for index, line in enumerate(masked):
        stripped = line.strip()
        if not stripped:
            continue
        if JAVA_ANNOT.match(stripped):
            annotations.append(raw[index].strip() if index < len(raw) else stripped)
            continue
        klass_match = JAVA_CLASS.match(stripped)
        if klass_match:
            klass = klass_match.group("name")
            annotations = []
            continue
        method = JAVA_METHOD.match(stripped)
        if method and any(ann.startswith(("@Test", "@ParameterizedTest", "@RepeatedTest"))
                          for ann in annotations):
            end = S.block_end(masked, index)
            disabled = [ann for ann in annotations
                        if ann.startswith(("@Disabled", "@Ignore", "@TestFactory"))]
            disabled = [ann for ann in disabled if ann.startswith(("@Disabled", "@Ignore"))]
            evidence = [ann.split("(")[0] for ann in annotations]
            parsed.cases.append(TestCase(
                name=method.group("name"), language=JAVA, framework=_java_framework(masked),
                line=index + 1, end_line=end + 1, suite=klass,
                skipped=bool(disabled), skip_kind="disabled" if disabled else "",
                skip_reason=_reason_from(disabled[0]) if disabled else "",
                evidence=tuple(evidence),
                assertions=java_assertions(masked, raw, index, end)))
        annotations = []
    parsed.frameworks = (_java_framework(masked),) if parsed.cases else ()
    parsed.state = STATE_PARSED if parsed.cases else STATE_EMPTY
    if not parsed.cases:
        parsed.notes = ["no @Test method in this file"]
    return parsed


def _java_framework(lines: list[str]) -> str:
    text = "\n".join(lines)
    if "org.junit.jupiter" in text:
        return "junit5"
    if "org.junit" in text:
        return "junit4"
    if "assertj" in text:
        return "assertj"
    return "junit5"


def java_assertions(masked: list[str], raw: list[str], start: int, end: int
                    ) -> tuple[Assertion, ...]:
    out: list[Assertion] = []
    for index in range(start, min(end + 1, len(masked))):
        line = masked[index]
        text = raw[index].strip() if index < len(raw) else line.strip()
        for match in JAVA_ASSERT.finditer(line):
            suffix = match.group("suffix")
            body = call_body(masked, index, match.end())
            args = split_arguments(body)
            head = args[0] if args else ""
            if suffix.startswith("That"):
                tail = _tail(masked, index, match.end() + len(body) + 1, span=2)
                chained = JAVA_ASSERTJ.search(tail)
                name = chained.group("name") if chained else "assertThat"
                strength = EXACT if name in JAVA_ASSERTJ_EXACT else (
                    PRESENCE if name else "")
                if strength:
                    out.append(Assertion(line=index + 1, kind="assertThat.%s" % name,
                                         strength=strength, subject=S.subject(head), text=text))
                continue
            strength = JAVA_ASSERT_METHODS.get(suffix, "")
            if not strength:
                continue
            kind = "assert%s" % (suffix or "")
            out.append(Assertion(line=index + 1, kind=kind, strength=strength,
                                 subject=S.subject(head) if strength != MESSAGE else "",
                                 text=text))
    return tuple(sorted(out, key=lambda a: (a.line, a.kind, a.subject)))
