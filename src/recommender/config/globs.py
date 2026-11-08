"""Glob matching for config-supplied path patterns — and nothing else.

`e0.9` (`reco-5vz`) owns the *taxonomy*: what a path **is**. This module only answers
the config-side question `fathom config explain --path P` needs: does this
user-supplied pattern match this path, and what did it come from. Keeping the matcher
here rather than inventing a taxonomy here is the difference between feeding e0.9 and
duplicating it, and duplicated glob knowledge is exactly the thing e0.9's ticket exists
to make impossible ("classifiers and detectors never encode their own glob lists").

Syntax, which is the syntax the glossary's *Noise suppression* entry implies
-----------------------------------------------------------------------------------

* `*` within one segment, `**` across segments, `?` one character;
* `{a,b}` alternation of literal alternatives;
* `[abc]` and `[a-z]` character classes, `[!…]` for the complement;
* a leading `!` marks the pattern as a **demotion** — e0.9's engine and `explain`
  render those differently, and a negated match still counts as "this key is relevant
  to this path", which is the whole point of the `--path` view;
* matching is against the *repository-relative, forward-slashed* path only. Absolute
  paths are refused rather than guessed at, because a matcher that accepts an absolute
  path can be pointed outside the repository by a cloned config value.

Two engines, one language, and the pattern decides
-------------------------------------------------

The obvious implementation translates a glob into an `re.Pattern` — four lines, and what
this file used to do. It is also a denial of service whose input a clone controls:
`noise.globs`, `test.globs`, `test.untested.exclude` and `taxonomy.classes.*` are all keys
a **repository file** may set, `**` translates to `.*`, and Python's `re` backtracks, so
`**a**a**a**a**a**a**a**a` against `aaaaaaaaaaaaaaaaab` takes exponential time. Both halves
of the pair — pattern and path — are attacker-shaped, and a matcher that hangs is as
effective as a parser that crashes, with better deniability
(`e7.8`, [threat model F-4](../../../docs/security/threat-model.md)).

So there are two engines behind one `Glob` object, chosen per pattern:

* **`_regex`** — the legacy translation into one anchored `re.Pattern`, kept because it is
  an order of magnitude faster than anything written in Python and because its worst case
  is polynomial *as long as the number of unbounded wildcards is bounded*: k wildcards give
  O(path^k). Two is the budget (`WILDCARD_BUDGET`), which covers every pattern in the
  shipped tables (`**/vendor/**`, `*.min.js`, `**/*_test.py`) and gives quadratic-at-worst
  on a path no longer than `REGEX_PATH_LIMIT`.
* **`_machine`** — a state-set simulation of the same language, with nothing to backtrack:
  O(pattern × path), always. Patterns with more than `WILDCARD_BUDGET` unbounded wildcards
  get it, and so does any path longer than `REGEX_PATH_LIMIT`, which is what keeps the
  regex engine's O(path²) bound a bound rather than a hope.

The two engines accept the same language. That is not assumed:
`tests/test_config_contract.py` replays the legacy translator against both engines over
every pattern in the shipped tables plus hostile ones, and names the one place the new
code deliberately differs (`[!a]`, which the old translation handed to `re` and `re` read
as the two-character set `!` and `a` instead of the glob complement).

Before either engine runs, a third and dumber test runs: every maximal run of literal characters a
pattern insists on must be present in the path (`Glob.required`). That is what keeps the *cost* claim
above true for a pattern like `**/*.egg-info/**` — three unbounded wildcards, so the linear engine,
and the taxonomy's broad bucket tries it for every path in the repository, so the Python state machine
was answering "no" for every one of them (four fifths of a cold lookup; `reco-ghao`). It cannot change
which paths match — a literal run has to appear, whatever the wildcards do around it — and the test
suite checks that it does not.
"""

from __future__ import annotations

import re

_CACHE: dict[str, "Glob"] = {}

#: How many unbounded wildcards (`*`, `*`, `**`) this module will hand to a backtracking
#: regex engine. k wildcards mean O(path^k) in the worst case; two covers every pattern the
#: shipped tables use and keeps the worst case quadratic.
WILDCARD_BUDGET = 2

#: The longest path the regex engine is used for. The quadratic worst case is only a bound
#: if the thing it is quadratic in is bounded, and a path is repository content.
REGEX_PATH_LIMIT = 4096


class BadPattern(ValueError):
    """A glob that cannot be compiled: reported per key, not as a crash."""


# ---------------------------------------------------------------------------
# the tokens
# ---------------------------------------------------------------------------
# One token list feeds both engines, which is what makes "two engines, one language" a
# statement that can be checked rather than hoped for.
#
# A state is `(epsilon successors, character transitions)`; the transition ops below are
# shared with the state machine.
LIT = "lit"              # one exact character:    (LIT, ch)
CLS = "cls"              # one character in a set: (CLS, frozenset, negated)
SEG = "seg"              # one character that is not `/`
SLASH = "slash"          # exactly `/`
SEGSTAR = "segstar"      # zero or more characters that are not `/`   (`*`)
ANYSTAR = "anystar"      # zero or more of anything                    (bare `**`)
SEGTHENSLASH = "segthenslash"   # `(?:[^/]+/)*`                        (`**/`)
ALT = "alt"              # one of several literal strings              (`{a,b}`)

#: The tokens that are unbounded quantifiers, and so are what `WILDCARD_BUDGET` counts.
#: `**/` counts as well: one `(?:[^/]+/)*` is unambiguous on its own, but two of them side
#: by side — `**/**/x`, or the `**/vendor/**/*` shape — can divide a path between themselves
#: in exponentially many ways, which is exactly the ambiguity that makes a backtracker hang.
_UNBOUNDED = (SEGSTAR, ANYSTAR, SEGTHENSLASH)


def _fires(test: tuple, char: str) -> bool:
    op = test[0]
    if op == LIT:
        return char == test[1]
    if op == CLS:
        return (char in test[1]) != test[2]
    if op == SEG or op == SEGSTAR:
        return char != "/"
    if op == SLASH:
        return char == "/"
    if op == ANYSTAR:
        return True
    raise AssertionError("unknown transition test %r" % (test,))


def _tokenize(pat: str) -> list[tuple]:
    """Split one glob body (no leading `!`) into tokens both engines understand."""
    tokens: list[tuple] = []
    i, n = 0, len(pat)
    while i < n:
        ch = pat[i]
        if ch == "*":
            if pat.startswith("**", i):
                if pat.startswith("/", i + 2):
                    # `**/` spans zero or more segments (so `**/dist/**` matches `dist/app.js`
                    # as well as `a/dist/app.js`); a bare `**` spans anything.
                    tokens.append((SEGTHENSLASH,))
                    i += 3
                    continue
                tokens.append((ANYSTAR,))
                i += 2
                continue
            tokens.append((SEGSTAR,))
            i += 1
            continue
        if ch == "?":
            tokens.append((SEG,))
            i += 1
            continue
        if ch == "{":
            end = pat.find("}", i)
            if end == -1:
                raise BadPattern("unbalanced { in %r" % pat)
            tokens.append((ALT, pat[i + 1:end].split(",")))
            i = end + 1
            continue
        if ch == "[":
            end = pat.find("]", i + 1)
            if end == -1:
                raise BadPattern("unbalanced [ in %r" % pat)
            body = pat[i + 1:end]
            # `[!…]` is the *glob* complement, which is what the syntax summary promises.
            # The translator this file replaces emitted the body straight into a regex, where
            # `[!a]` is the two-character set `!` and `a` — a silent misreading of every
            # negated class anybody ever wrote one. The differential test names it as the one
            # intended behaviour change.
            negated = body.startswith("!")
            tokens.append((CLS, _class_chars(body[1:] if negated else body, pat), negated))
            i = end + 1
            continue
        tokens.append((LIT, ch))
        i += 1
    return tokens


def _class_chars(body: str, pattern: str) -> frozenset:
    """The characters a `[...]` body stands for, with `a-z` ranges expanded."""
    chars: set[str] = set()
    index = 0
    while index < len(body):
        if (index + 2 < len(body) and body[index + 1] == "-"
                and body[index + 2] != "]"):
            start, stop = body[index], body[index + 2]
            if start > stop:
                raise BadPattern("empty range %r in %r" % (body, pattern))
            chars.update(chr(code) for code in range(ord(start), ord(stop) + 1))
            index += 3
            continue
        chars.add(body[index])
        index += 1
    if not chars:
        raise BadPattern("empty character class in %r" % pattern)
    return frozenset(chars)


# ---------------------------------------------------------------------------
# engine 1: the legacy regular-expression translation
# ---------------------------------------------------------------------------
def _to_regex(tokens: list[tuple]) -> re.Pattern:
    """Translate tokens into one anchored regular expression."""
    out: list[str] = ["\\A"]
    for token in tokens:
        op = token[0]
        if op == LIT:
            out.append(re.escape(token[1]))
        elif op == CLS:
            body = "".join(re.escape(ch) for ch in sorted(token[1]))
            out.append("[^%s]" % body if token[2] else "[%s]" % body)
        elif op == SEG:
            out.append("[^/]")
        elif op == SEGSTAR:
            out.append("[^/]*")
        elif op == ANYSTAR:
            out.append(".*")
        elif op == SEGTHENSLASH:
            out.append("(?:[^/]+/)*")
        elif op == ALT:
            out.append("(?:%s)" % "|".join(re.escape(alt) for alt in token[1]))
        else:
            raise BadPattern("cannot compile token %r" % (token,))
    out.append("\\Z")
    try:
        return re.compile("".join(out))
    except re.error as exc:                     # a class that only `re` can find is bad
        raise BadPattern(str(exc)) from exc


# ---------------------------------------------------------------------------
# engine 2: the state-set simulation
# ---------------------------------------------------------------------------
class _Machine:
    """The same language, simulated as a set of active states: nothing to backtrack."""

    __slots__ = ("states", "start", "accept")

    def __init__(self, states: list[tuple[list[int], list[tuple[tuple, int]]]], start: int,
                 accept: int):
        self.states = states
        self.start = start
        self.accept = accept

    def _closure(self, seed: set[int]) -> set[int]:
        """Every state reachable through epsilon transitions, iteratively, cycle-safe."""
        seen = set(seed)
        stack = list(seen)
        while stack:
            state = stack.pop()
            for target in self.states[state][0]:
                if target not in seen:
                    seen.add(target)
                    stack.append(target)
        return seen

    def match(self, text: str) -> bool:
        """True when all of `text` is consumed.

        The accept state can be reachable long before the end of `text` (that is what `**`
        means), so the answer is decided once, after the last character: this is a
        *fullmatch*. A version that returned early on "accept is reachable" would match the
        pattern `b` against `b0.aa0`, and every bucketed caller would quietly start matching
        too much.
        """
        states = self.states
        active = self._closure({self.start})
        for char in text:
            reached = set()
            for state in active:
                for test, target in states[state][1]:
                    if _fires(test, char):
                        reached.add(target)
            if not reached:
                return False
            active = self._closure(reached)
        return self.accept in active


def _to_machine(tokens: list[tuple]) -> _Machine:
    """Compile tokens into the state machine, back to front.

    Built back-to-front: a state means "this token and everything after it", so the two
    epsilon cases — a wildcard that may match nothing, and `**/` which may match zero
    segments — are one line each.
    """
    states: list[tuple[list[int], list[tuple[tuple, int]]]] = []

    def new(eps: tuple[int, ...] = (), transitions: tuple = ()) -> int:
        states.append((list(eps), list(transitions)))
        return len(states) - 1

    accept = new()
    nxt = accept
    for token in reversed(tokens):
        op = token[0]
        if op == LIT or op == CLS:
            nxt = new((), [(token, nxt)])
        elif op == SEG:
            nxt = new((), [((SEG,), nxt)])
        elif op == SEGSTAR:
            state = new((nxt,), [])
            states[state][1].append(((SEGSTAR,), state))
            nxt = state
        elif op == ANYSTAR:
            state = new((nxt,), [])
            states[state][1].append(((ANYSTAR,), state))
            nxt = state
        elif op == SEGTHENSLASH:
            # `(?:[^/]+/)*`: epsilon to `nxt` is the "zero segments" case; one or more
            # segments are one-or-more non-slash characters and then a `/` back here.
            first = new((nxt,), [])
            second = new()
            states[first][1].append(((SEG,), second))
            states[second][1].extend([((SEG,), second), ((SLASH,), first)])
            nxt = first
        elif op == ALT:
            # An alternation of literal strings: epsilon out to one character chain per
            # alternative, so no alternative can shadow another by backtracking. An empty
            # alternative (`{,x}`) contributes the epsilon to `nxt` itself, which is what
            # the legacy `(?:|x)` translation did.
            entry = new()
            for literal in token[1]:
                target = nxt
                for char in reversed(literal):
                    target = new((), [((LIT, char), target)])
                states[entry][0].append(target)
            nxt = entry
        else:
            raise BadPattern("cannot compile token %r" % (token,))
    return _Machine(states, nxt, accept)


# ---------------------------------------------------------------------------
# the object callers hold
# ---------------------------------------------------------------------------
def _required_literals(tokens: list[tuple]) -> tuple[str, ...]:
    """The literal runs no text can match without containing, longest first.

    A maximal run of `LIT` tokens consumes exactly those characters in exactly that order,
    whatever the wildcards around it do — so a text missing one of them cannot match, and
    `needle not in text` is a C-level substring test against a needle the pattern chose.
    It is worth a line of its own because of the shape it catches: the one machine-engine
    pattern in the shipped table is `**/*.egg-info/**`, and the broad bucket tries that
    pattern against *every* path, so without this the Python state machine ran twice per
    file to say "no" to a table that only a `*.egg-info` path can answer yes to
    (`reco-ghao`: the taxonomy's cold-lookup budget was 87 µs against an 80 µs bar, and
    this pattern was most of the difference).

    `CLS`, `ALT` and every wildcard break a run: none of them says which character it will
    take. A pattern made only of wildcards demands nothing and yields `()`.
    """
    runs: list[str] = []
    current: list[str] = []
    for token in tokens:
        if token[0] == LIT:
            current.append(token[1])
        elif current:
            runs.append("".join(current))
            current = []
    if current:
        runs.append("".join(current))
    return tuple(sorted(set(runs), key=len, reverse=True))


class Glob:
    """One compiled glob: an accept test over the whole path, whichever engine runs it."""

    __slots__ = ("pattern", "_tokens", "_fast", "_regex", "_machine", "_needs")

    def __init__(self, pattern: str, tokens: list[tuple]):
        self.pattern = pattern
        self._tokens = tokens
        counted = sum(1 for token in tokens if token[0] in _UNBOUNDED)
        self._fast = counted <= WILDCARD_BUDGET
        self._needs = _required_literals(tokens)
        self._regex: re.Pattern | None = None
        self._machine: _Machine | None = None

    @property
    def required(self) -> tuple[str, ...]:
        """The literal substrings a match must contain; `()` for a pattern that demands none."""
        return self._needs

    def match(self, text: str) -> bool:
        """True when `text` is matched in full — what the old anchored translation meant."""
        for needle in self._needs:
            if needle not in text:
                return False
        if self._fast and len(text) <= REGEX_PATH_LIMIT:
            if self._regex is None:
                self._regex = _to_regex(self._tokens)
            return self._regex.match(text) is not None
        if self._machine is None:
            self._machine = _to_machine(self._tokens)
        return self._machine.match(text)

    def fullmatch(self, text: str) -> bool:
        return self.match(text)

    @property
    def engine(self) -> str:
        """`regex` or `machine`, for `config explain` and for the differential test."""
        return "regex" if self._fast else "machine"

    def __repr__(self) -> str:
        return "Glob(%r, %s)" % (self.pattern, self.engine)


def compile_glob(pattern: str) -> Glob:
    """Compile one glob (without its `!`) into a matcher.

    Raises :class:`BadPattern` for a pattern this syntax cannot mean; every caller reports
    that per key rather than crashing (see `layer._check_globs`).
    """
    pat = pattern[1:] if pattern.startswith("!") else pattern
    cached = _CACHE.get(pat)
    if cached is not None:
        return cached
    compiled = Glob(pat, _tokenize(pat))
    _CACHE[pat] = compiled
    return compiled


def is_negated(pattern: str) -> bool:
    return pattern.startswith("!")


def matches(pattern: str, path: str) -> bool:
    """Whether `pattern` matches `path`. Directory-ish patterns also match their contents.

    `**/dist/**` is written as "everything under any `dist`", and `dist/**` should mean
    the same thing whether or not the caller wrote the trailing slash, so a pattern
    ending in `/` or `/**` additionally matches the directory itself.
    """
    path = normalise(path)
    try:
        compiled = compile_glob(pattern)
    except BadPattern:
        return False
    if compiled.match(path):
        return True
    if pattern.endswith("/**") and compiled.match(path + "/"):
        return True
    if pattern.endswith("/") and path.startswith(pattern):
        return True
    return False


def matching(patterns, path: str) -> list[str]:
    """The subset of `patterns` that speak to `path`, in declaration order.

    Order is preserved and reported because e0.9's precedence and a user's mental model
    both depend on it: a `!keep-me.txt` *after* a broad `**/dist/**` means something
    different from one before it.
    """
    return [p for p in patterns if matches(p, path)]


def normalise(path: str) -> str:
    """Repo-relative, forward-slashed, no leading `./`. Refuses absolute paths."""
    text = str(path).replace("\\", "/").strip()
    if text.startswith("./"):
        text = text[2:]
    return text.lstrip("/")


def inside(child: str, parent: str) -> bool:
    """Containment test used by the repo-file path rule (ADR-004 §12)."""
    c, p = normalise(child), normalise(parent)
    if not p:
        return True
    return c == p or c.startswith(p.rstrip("/") + "/")


def escapes_root(value: str) -> bool:
    """True if `value` cannot be resolved *inside* the repository root.

    A *cloned* `fathom.toml` pointing `paths.store` at `$HOME/.ssh/authorized_keys`
    is the exact threat ADR-001 §4.6 names ("a clone must never be able to make Fathom
    … write outside `.fathom/`"). The check is textual on purpose: the path need not
    exist to be refused, and resolving symlinks here would make the check itself
    raceable. Absolute and `~` paths are refused from a repo file, which is stricter
    than is strictly necessary and is the right trade for a value that names a
    database a process will open for write.
    """
    raw = str(value)
    if raw.startswith("~") or raw.startswith("/") or re.match(r"^[A-Za-z]:[\\/]", raw):
        return True
    return any(part == ".." for part in normalise(raw).split("/"))
